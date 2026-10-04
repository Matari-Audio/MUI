// Copied/adapted from objc2 0.5.2 (MIT; see ../LICENSE-OBJC2).
// MUI modification: private runtime names and public-API equivalents of
// crate-private builder accessors. See ../MUI-PATCHES.md for the exact sources.

use crate::class_ivars::{register_with_ivars, setup_dealloc};
use objc2::{
    ClassType, DeclaredClass, Message,
    encode::{Encode, EncodeArguments, EncodeReturn, Encoding},
    ffi,
    runtime::{AnyClass, AnyObject, Bool, MethodImplementation, Sel},
};
use std::{ffi::CString, marker::PhantomData, mem, mem::ManuallyDrop, ptr::NonNull};

pub(crate) struct ClassBuilder {
    cls: NonNull<ffi::objc_class>,
}

impl ClassBuilder {
    fn new(name: &str, superclass: &AnyClass) -> Option<Self> {
        let name = CString::new(name).unwrap();
        let super_ptr = (superclass as *const AnyClass).cast();
        // SAFETY: The superclass is registered and the name is a C string.
        let cls = unsafe { ffi::objc_allocateClassPair(super_ptr, name.as_ptr(), 0) };
        NonNull::new(cls).map(|cls| Self { cls })
    }

    pub(crate) unsafe fn add_method<T, F>(&mut self, sel: Sel, func: F)
    where
        T: Message + ?Sized,
        F: MethodImplementation<Callee = T>,
    {
        // This is the public-API equivalent of objc2's debug signature check.
        #[cfg(debug_assertions)]
        {
            // SAFETY: class_getSuperclass is valid before registration too.
            let superclass = unsafe { ffi::class_getSuperclass(self.cls.as_ptr()) };
            // SAFETY: A non-null superclass points to a registered class.
            if let Some(superclass) = unsafe { superclass.cast::<AnyClass>().as_ref() }
                && superclass.instance_method(sel).is_some()
            {
                superclass
                    .verify_sel::<F::Arguments, F::Return>(sel)
                    .unwrap();
            }
        }
        let encs = F::Arguments::ENCODINGS;
        let sel_args = sel.name().chars().filter(|&c| c == ':').count();
        assert_eq!(
            sel_args,
            encs.len(),
            "selector {sel} accepts {sel_args} arguments, but function accepts {}",
            encs.len()
        );
        let mut types = format!(
            "{}{}{}",
            F::Return::ENCODING_RETURN,
            <*mut AnyObject>::ENCODING,
            Sel::ENCODING
        );
        for enc in encs {
            use std::fmt::Write;
            write!(&mut types, "{enc}").unwrap();
        }
        let types = CString::new(types).unwrap();
        // SAFETY: The caller guarantees the selector/function signatures match.
        let success = Bool::from_raw(unsafe {
            ffi::class_addMethod(
                self.cls.as_ptr(),
                sel.as_ptr(),
                Some(func.__imp()),
                types.as_ptr(),
            )
        });
        assert!(success.as_bool(), "failed to add method {sel}");
    }

    pub(crate) fn add_ivar<T: Encode>(&mut self, name: &str) {
        // SAFETY: T supplies its correct encoding.
        unsafe { self.add_ivar_inner::<T>(name, &T::ENCODING) }
    }

    pub(crate) unsafe fn add_ivar_inner<T>(&mut self, name: &str, encoding: &Encoding) {
        let c_name = CString::new(name).unwrap();
        let encoding = CString::new(encoding.to_string()).unwrap();
        let align = mem::align_of::<T>();
        assert!(
            align.count_ones() == 1,
            "alignment required to be a power of 2"
        );
        // SAFETY: The class is unregistered and the supplied type/encoding is
        // the opaque ivar storage used by the original objc2 registration.
        let success = Bool::from_raw(unsafe {
            ffi::class_addIvar(
                self.cls.as_ptr(),
                c_name.as_ptr(),
                mem::size_of::<T>(),
                align.trailing_zeros() as u8,
                encoding.as_ptr(),
            )
        });
        assert!(success.as_bool(), "failed to add ivar {name}");
    }

    pub(crate) fn register(self) -> &'static AnyClass {
        let this = ManuallyDrop::new(self);
        // SAFETY: This builder owns the unregistered class and consumes itself
        // exactly once. The runtime retains registered classes permanently.
        unsafe { ffi::objc_registerClassPair(this.cls.as_ptr()) };
        // SAFETY: The class is now registered and lives for the process lifetime.
        unsafe { this.cls.cast::<AnyClass>().as_ref() }
    }
}

impl Drop for ClassBuilder {
    fn drop(&mut self) {
        // SAFETY: register consumes this builder without dropping it; any
        // remaining builder still exclusively owns an unregistered class.
        unsafe { ffi::objc_disposeClassPair(self.cls.as_ptr()) }
    }
}

pub(crate) struct ClassBuilderHelper<T: ?Sized> {
    builder: ClassBuilder,
    p: PhantomData<T>,
}

impl<T: DeclaredClass> ClassBuilderHelper<T> {
    pub(crate) fn new() -> Self
    where
        T::Super: ClassType,
    {
        let name = crate::image::class_name(T::NAME);
        let mut builder = ClassBuilder::new(&name, <T::Super as ClassType>::class())
            .unwrap_or_else(|| panic!("could not create new class {name}"));
        setup_dealloc::<T>(&mut builder);
        Self {
            builder,
            p: PhantomData,
        }
    }

    pub(crate) unsafe fn add_method<F>(&mut self, sel: Sel, func: F)
    where
        F: MethodImplementation<Callee = T>,
    {
        // SAFETY: Checked by caller, as in objc2's original helper.
        unsafe { self.builder.add_method(sel, func) }
    }

    pub(crate) fn register(self) -> (&'static AnyClass, isize, isize) {
        register_with_ivars::<T>(self.builder)
    }
}
