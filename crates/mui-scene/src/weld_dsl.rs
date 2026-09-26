//! Default-first material-weld macros: ordinary element constructors, not a
//! second style language. A shared vector outline is `.union(fill)` instead.

/// Default material weld, or `weld![Weld::shape(); a, b]` for an explicit policy.
/// Use `.weld(..)` on a row/col/grid when it should arrange the sources.
#[macro_export]
macro_rules! weld {
    ($options:expr; $($child:expr),* $(,)?) => {{
        $crate::Styled::weld($crate::stack([$($crate::IntoEl::into_el($child)),*]), $options)
    }};
    ($($child:expr),* $(,)?) => {
        $crate::weld![$crate::Weld::default(); $($child),*]
    };
}
