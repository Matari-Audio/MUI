// Copied from objc2 0.5.2 (MIT; see ../LICENSE-OBJC2).
// MUI modification: private namespaced class registration; objc2 lifecycle
// and macro expansion are otherwise retained. See ../MUI-PATCHES.md.
macro_rules! declare_class {
    {
        $(#[$m:meta])*
        $v:vis struct $name:ident;

        unsafe impl ClassType for $for_class:ty {
            $(#[inherits($($inheritance_rest:ty),+)])?
            type Super = $superclass:ty;

            type Mutability = $mutability:ty;

            const NAME: &'static str = $name_const:expr;
        }

        impl DeclaredClass for $for_declared:ty {
            $(type Ivars = $ivars:ty;)?
        }

        $($impls:tt)*
    } => {
        $(#[$m])*
        #[repr(C)]
        $v struct $name {
            // Superclasses are deallocated by calling `[super dealloc]`.
            __superclass: ::objc2::__macro_helpers::ManuallyDrop<$superclass>,
            // Include ivars for proper auto traits.
            __ivars: ::objc2::__macro_helpers::PhantomData<<Self as ::objc2::DeclaredClass>::Ivars>,
        }

        ::objc2::__extern_class_impl_traits! {
            // SAFETY: Upheld by caller
            unsafe impl () for $for_class {
                INHERITS = [$superclass, $($($inheritance_rest,)+)? ::objc2::runtime::AnyObject];

                fn as_super(&self) {
                    &*self.__superclass
                }

                fn as_super_mut(&mut self) {
                    &mut *self.__superclass
                }
            }
        }

        // Anonymous block to hide the shared statics
        const _: () = {
            static mut __OBJC2_CLASS: ::objc2::__macro_helpers::MaybeUninit<&'static ::objc2::runtime::AnyClass> = ::objc2::__macro_helpers::MaybeUninit::uninit();
            static mut __OBJC2_IVAR_OFFSET: ::objc2::__macro_helpers::MaybeUninit<::objc2::__macro_helpers::isize> = ::objc2::__macro_helpers::MaybeUninit::uninit();
            static mut __OBJC2_DROP_FLAG_OFFSET: ::objc2::__macro_helpers::MaybeUninit<::objc2::__macro_helpers::isize> = ::objc2::__macro_helpers::MaybeUninit::uninit();

            // Creation
            unsafe impl ClassType for $for_class {
                type Super = $superclass;
                type Mutability = $mutability;
                const NAME: &'static ::objc2::__macro_helpers::str = $name_const;

                fn class() -> &'static ::objc2::runtime::AnyClass {
                    ::objc2::__macro_helpers::assert_mutability_matches_superclass_mutability::<Self>();

                    // TODO: Use `std::sync::OnceLock`
                    static REGISTER_CLASS: ::objc2::__macro_helpers::Once = ::objc2::__macro_helpers::Once::new();

                    REGISTER_CLASS.call_once(|| {
                        let mut __objc2_builder = $crate::class::ClassBuilderHelper::<Self>::new();

                        // Implement protocols and methods
                        ::objc2::__declare_class_register_impls! {
                            (__objc2_builder)
                            $($impls)*
                        }

                        let (__objc2_cls, __objc2_ivar_offset, __objc2_drop_flag_offset) = __objc2_builder.register();

                        // SAFETY: Modification is ensured by `Once` to happen
                        // before any access to the variables.
                        unsafe {
                            (::core::ptr::addr_of_mut!(__OBJC2_CLASS)).write(::objc2::__macro_helpers::MaybeUninit::new(__objc2_cls));
                            if <Self as ::objc2::__macro_helpers::DeclaredIvarsHelper>::HAS_IVARS {
                                (::core::ptr::addr_of_mut!(__OBJC2_IVAR_OFFSET)).write(::objc2::__macro_helpers::MaybeUninit::new(__objc2_ivar_offset));
                            }
                            if <Self as ::objc2::__macro_helpers::DeclaredIvarsHelper>::HAS_DROP_FLAG {
                                (::core::ptr::addr_of_mut!(__OBJC2_DROP_FLAG_OFFSET)).write(::objc2::__macro_helpers::MaybeUninit::new(__objc2_drop_flag_offset));
                            }
                        }
                    });

                    // SAFETY: We just registered the class, so is now available
                    unsafe { __OBJC2_CLASS.assume_init() }
                }

                #[inline]
                fn as_super(&self) -> &Self::Super {
                    &*self.__superclass
                }

                #[inline]
                fn as_super_mut(&mut self) -> &mut Self::Super {
                    &mut *self.__superclass
                }
            }

            impl DeclaredClass for $for_declared {
                type Ivars = ::objc2::__select_ivars!($($ivars)?);

                #[inline]
                fn __ivars_offset() -> ::objc2::__macro_helpers::isize {
                    // Only access ivar offset if we have an ivar.
                    //
                    // This makes the offset not be included in the final
                    // executable if it's not needed.
                    if <Self as ::objc2::__macro_helpers::DeclaredIvarsHelper>::HAS_IVARS {
                        // SAFETY: Accessing the offset is guaranteed to only be
                        // done after the class has been initialized.
                        unsafe { __OBJC2_IVAR_OFFSET.assume_init() }
                    } else {
                        // Fall back to an offset of zero.
                        //
                        // This is fine, since any reads here will only be via. zero-sized
                        // ivars, where the actual pointer doesn't matter.
                        0
                    }
                }

                #[inline]
                fn __drop_flag_offset() -> ::objc2::__macro_helpers::isize {
                    if <Self as ::objc2::__macro_helpers::DeclaredIvarsHelper>::HAS_DROP_FLAG {
                        // SAFETY: Same as above.
                        unsafe { __OBJC2_DROP_FLAG_OFFSET.assume_init() }
                    } else {
                        // Fall back to an offset of zero.
                        //
                        // This is fine, since the drop flag is never actually used in the
                        // cases where it was not added.
                        0
                    }
                }

                // SAFETY: The offsets are implemented correctly
                const __UNSAFE_OFFSETS_CORRECT: () = ();
            }
        };

        // Methods
        ::objc2::__declare_class_output_impls! {
            $($impls)*
        }
    };
}

pub(crate) use declare_class;
