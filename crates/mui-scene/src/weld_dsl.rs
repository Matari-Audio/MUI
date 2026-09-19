//! Default-first welding macros. The existing `.weld(fill)` remains the exact
//! legacy API; Rust methods cannot overload `.weld()` and `.weld(fill)` by arity.
//! These macros are ordinary element constructors, not a second style language.

/// Default material weld, or `weld![Weld::shape(); a, b]` for an explicit policy.
/// Use `.weld_with(..)` on a row/column/grid when it should arrange the sources.
#[macro_export]
macro_rules! weld {
    ($options:expr; $($child:expr),* $(,)?) => {{
        $crate::Styled::weld_with($crate::overlay([$($crate::IntoEl::into_el($child)),*]), $options)
    }};
    ($($child:expr),* $(,)?) => {
        $crate::weld![$crate::Weld::default(); $($child),*]
    };
}
/// Animated real fusion: `weld_morph![progress; a, b]`. Progress is explicit,
/// so a spring, a timeline, or pointer distance can drive the same operation.
#[macro_export]
macro_rules! weld_morph {
    ($progress:expr; $($child:expr),* $(,)?) => {
        $crate::weld![$crate::Weld::default().morph($progress); $($child),*]
    };
    ($options:expr, $progress:expr; $($child:expr),* $(,)?) => {
        $crate::weld![$options.morph($progress); $($child),*]
    };
}
