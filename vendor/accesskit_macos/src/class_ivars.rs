// Copied from objc2 0.5.2 (MIT; see ../LICENSE-OBJC2).
// MUI modification: private namespaced class registration; objc2 lifecycle
// and macro expansion are otherwise retained. See ../MUI-PATCHES.md.
use crate::class::ClassBuilder;
use objc2::__macro_helpers::DeclaredIvarsHelper;
use objc2::{
    ClassType, DeclaredClass,
    encode::{Encode, Encoding},
    runtime::{AnyClass, AnyObject, MessageReceiver, Sel},
    sel,
};
use std::{
    borrow::Cow,
    mem,
    ptr::{self, NonNull},
};

/// A type representing the drop flags that may be set for a type.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(
    dead_code,
    reason = "flags are written by the pinned objc2 0.5.2 initializer"
)]
pub(crate) enum DropFlag {
    /// Set to zero to ensure that this is the default when created by the
    /// Objective-C runtime.
    ///
    /// Ivars are [documented][obj-init-zeroed] to be zero-initialized after
    /// allocation, and that has been true since at least [the Objective-C
    /// version shipped with Mac OS X 10.0][objc4-208-init].
    ///
    /// [obj-init-zeroed]: https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/WorkingwithObjects/WorkingwithObjects.html#//apple_ref/doc/uid/TP40011210-CH4-SW7
    /// [objc4-208-init]: https://github.com/apple-oss-distributions/objc4/blob/objc4-208/runtime/objc-class.m#L367
    #[allow(dead_code)]
    Allocated = 0x00,
    /// Used when `mem::needs_drop::<T::Ivars>()`, or with debug assertions enabled.
    InitializedIvars = 0x0f,
    /// Used when `mem::needs_drop::<T>()`, or with debug assertions enabled.
    Finalized = 0xff,
}

// SAFETY: The DropFlag is #[repr(u8)]
unsafe impl Encode for DropFlag {
    const ENCODING: Encoding = u8::ENCODING;
}

/// Helper function for getting a pointer to the instance variable.
///
/// # Safety
///
/// The pointer must be valid, and the instance variable offset (if it has
/// any) must have been initialized.
#[inline]
unsafe fn ptr_to_ivar<T: ?Sized + DeclaredClass>(ptr: NonNull<T>) -> NonNull<T::Ivars> {
    // This is called even when there is no ivars, but that's fine, since in
    // that case the ivar is zero-sized, and the offset will be zero, so we
    // can still compute a valid pointer to the ivar.
    //
    // debug_assert!(T::HAS_IVARS);

    // SAFETY: That an instance variable with the given type exists at the
    // specified offset is ensured by `DeclaredClass` trait implementor.
    unsafe { ivar_at_offset::<T::Ivars>(ptr.cast(), T::__ivars_offset()) }
}

/// Helper function for getting a pointer to the drop flag.
///
/// # Safety
///
/// The pointer must be valid and have an initialized drop flag.
#[inline]
unsafe fn ptr_to_drop_flag<T: DeclaredClass>(ptr: NonNull<T>) -> *mut DropFlag {
    debug_assert!(T::HAS_DROP_FLAG, "type did not have drop flag");
    // SAFETY: That a drop flag exists at the specified offset is ensured
    // by caller.
    unsafe { ivar_at_offset::<DropFlag>(ptr.cast(), T::__drop_flag_offset()).as_ptr() }
}

pub(crate) fn setup_dealloc<T: DeclaredClass>(builder: &mut ClassBuilder)
where
    T::Super: ClassType,
{
    // Add dealloc if the class or the ivars need dropping.
    if mem::needs_drop::<T>() || mem::needs_drop::<T::Ivars>() {
        let func: unsafe extern "C" fn(_, _) = dealloc::<T>;
        // SAFETY: The function signature is correct, and method contract is
        // upheld inside `dealloc`.
        unsafe { builder.add_method(sel!(dealloc), func) };
    } else {
        // Users should not rely on this ommision, it is only an optimization.
    }
}

/// The `dealloc` Objective-C method.
///
/// See the following links for more details about `dealloc`:
/// - <https://clang.llvm.org/docs/AutomaticReferenceCounting.html#dealloc>
/// - <https://developer.apple.com/documentation/objectivec/nsobject/1571947-dealloc>
/// - <https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmRules.html#//apple_ref/doc/uid/20000994-SW2>
///
/// TODO: Change this to `extern "C-unwind"`, unwinding in dealloc is allowed.
unsafe extern "C" fn dealloc<T: DeclaredClass>(this: NonNull<T>, cmd: Sel)
where
    T::Super: ClassType,
{
    /// Helper function for marking the cold path when branching.
    #[inline]
    #[cold]
    fn cold_path() {}

    // SAFETY: `dealloc` is only registered when there is a need for dropping,
    // and hence a need for a drop flag.
    let drop_flag = unsafe { *ptr_to_drop_flag(this) };

    if mem::needs_drop::<T>() {
        match drop_flag {
            // Don't deallocate the current instance if it has not been fully
            // initialized.
            //
            // Note that we still run the superclass deinitializer below.
            DropFlag::Allocated | DropFlag::InitializedIvars => cold_path(),
            // SAFETY: This is the `dealloc` method, so we know that the type
            // never needs to be deallocated again.
            //
            // Additionally, we know that the type was fully initialized, since
            // that's what the drop flag says.
            //
            // TODO: This can unwind, is it correct to just let that
            // propagate?
            DropFlag::Finalized => unsafe { ptr::drop_in_place(this.as_ptr()) },
        }
    }

    // TODO: Debug assertions that the retain count is still 1 here.

    // Note: This should be done inside `.cxx_destruct`, since if a superclass
    // calls an overwritten method in its `dealloc`, it can access
    // deinitialized instance variables; but we can't do that without
    // generating statics, so we have to do it in `dealloc` for now.
    //
    // It is very important that we do this after the `Drop` of the class
    // itself above, though.
    //
    // Another possibility would be to read the contents of the ivars onto the
    // stack here, and only deinitialize after the superclass' `dealloc`, but
    // that would break the pinning guarantee that ivars otherwise have.
    if mem::needs_drop::<T::Ivars>() {
        match drop_flag {
            // Do nothing if the ivars have not been initialized.
            DropFlag::Allocated => cold_path(),
            DropFlag::InitializedIvars | DropFlag::Finalized => {
                // SAFETY: The instance variable is initialized, so it is
                // valid to drop here.
                //
                // TODO: This can unwind, is it correct to just let that
                // propagate?
                unsafe { ptr::drop_in_place(ptr_to_ivar(this).as_ptr()) };
            }
        }
    }

    // The superclass' "marker" that this stores is wrapped in `ManuallyDrop`,
    // we drop it by calling the superclass' `dealloc` method instead.
    //
    // Note: ARC does this automatically, which means most Objective-C code in
    // the wild don't contain this call; but we _are_ ARC, so we must do this.
    //
    // SAFETY: The argument and return types are correct, and we make sure to
    // only call this once.
    unsafe {
        MessageReceiver::send_super_message(
            this,
            <T as ClassType>::Super::class(),
            cmd, // Reuse the selector
            (),  // No arguments
        )
    }
}

/// Register the class, and get the ivar offsets.
#[inline]
pub(crate) fn register_with_ivars<T: DeclaredClass>(
    mut builder: ClassBuilder,
) -> (&'static AnyClass, isize, isize) {
    // AccessKit's macOS-only adapter uses objc2's Apple ivar names. Its
    // GNUstep-only naming branch is intentionally not part of this adapter.
    let (ivar_name, drop_flag_name): (Cow<'static, str>, Cow<'static, str>) =
        ("ivars".into(), "drop_flag".into());

    if T::HAS_IVARS {
        // TODO: Consider not adding a encoding - Swift doesn't do it.
        let ivar_encoding = Encoding::Array(
            mem::size_of::<T::Ivars>() as u64,
            match mem::align_of::<T::Ivars>() {
                1 => &u8::ENCODING,
                2 => &u16::ENCODING,
                4 => &u32::ENCODING,
                // The alignment of `u64` may not be 8 on all architectures
                8 if mem::align_of::<u64>() == 8 => &u64::ENCODING,
                alignment => panic!("unsupported alignment {alignment} for `{}::Ivars`", T::NAME),
            },
        );
        unsafe { builder.add_ivar_inner::<T::Ivars>(&ivar_name, &ivar_encoding) };
    }

    if T::HAS_DROP_FLAG {
        // TODO: Maybe we can reuse the drop flag when subclassing an already
        // declared class?
        builder.add_ivar::<DropFlag>(&drop_flag_name);
    }

    let cls = builder.register();

    let ivars_offset = if T::HAS_IVARS {
        // Monomorphized error handling
        // Intentionally not #[track_caller], we expect this error to never occur
        fn get_ivar_failed() -> ! {
            unreachable!("failed retrieving instance variable on newly declared class")
        }

        cls.instance_variable(&ivar_name)
            .unwrap_or_else(|| get_ivar_failed())
            .offset()
    } else {
        // Fallback to an offset of zero.
        //
        // This is fine, since any reads here will only be via. zero-sized
        // ivars, where the actual pointer doesn't matter.
        0
    };

    let drop_flag_offset = if T::HAS_DROP_FLAG {
        // Monomorphized error handling
        // Intentionally not #[track_caller], we expect this error to never occur
        fn get_drop_flag_failed() -> ! {
            unreachable!("failed retrieving drop flag instance variable on newly declared class")
        }

        cls.instance_variable(&drop_flag_name)
            .unwrap_or_else(|| get_drop_flag_failed())
            .offset()
    } else {
        // Fall back to an offset of zero.
        //
        // This is fine, since the drop flag is never actually used in the
        // cases where it was not added.
        0
    };

    (cls, ivars_offset, drop_flag_offset)
}

// Copied from objc2 0.5.2 runtime/mod.rs; its original helper is private.
#[inline]
unsafe fn ivar_at_offset<T>(ptr: NonNull<AnyObject>, offset: isize) -> NonNull<T> {
    let ptr: NonNull<u8> = ptr.cast();
    let ptr: *mut u8 = ptr.as_ptr();
    // SAFETY: The offset is valid, as required by the caller.
    let ptr: *mut u8 = unsafe { ptr.offset(offset) };
    // SAFETY: The offset operation cannot produce a null pointer.
    let ptr: NonNull<u8> = unsafe { NonNull::new_unchecked(ptr) };
    ptr.cast()
}
