//! What a declaration says that the solver would have worked out anyway, or
//! that fights it: sizes written by hand where intrinsic sizing holds.
//!
//! Static: it reads the tree, not a layout, so it runs in any test.
//!
//! ```
//! use mui_layout::{block, col, lint, Len};
//! let tree = col([block(Len::Pct(100.), 20).id("wide")]);
//! assert_eq!(lint::lint(&tree, |_| false).len(), 1);
//! ```

use crate::{Align, Kind, Len, Node};

/// One declaration the solver did not need, or one that fights it.
#[derive(Clone, Debug, PartialEq)]
pub enum Lint {
    /// `100%` across an axis the parent already stretches the child on:
    /// delete it.
    Redundant { at: String, vertical: bool },
    /// `100%` along a row or column that has siblings: it claims the whole
    /// line and squeezes them. `.grow(1)` takes what is left.
    WholeLine { at: String, vertical: bool },
    /// A fixed pixel extent on text, which measures itself. Use
    /// `.value_reserve(..)` or `.measure_as(..)` for a stable width.
    FixedText { at: String, vertical: bool },
}

/// Every lint in `root`. `is_text` says which payloads are text.
pub fn lint<P>(root: &Node<P>, is_text: impl Fn(&P) -> bool) -> Vec<Lint> {
    let mut out = Vec::new();
    walk(root, root.key().unwrap_or("root"), &is_text, &mut out);
    out
}

fn walk<P>(n: &Node<P>, at: &str, is_text: &impl Fn(&P) -> bool, out: &mut Vec<Lint>) {
    if matches!(n.kind, Kind::Content(_)) && is_text(&n.payload) {
        for (vertical, len) in [(false, n.width), (true, n.height)] {
            if matches!(len, Len::Px(_)) {
                out.push(Lint::FixedText {
                    at: at.to_string(),
                    vertical,
                });
            }
        }
    }
    let flow = n.children().iter().filter(|c| !c.float).count();
    for (i, c) in n.children().iter().enumerate() {
        let here = c.key().map_or_else(|| format!("{at}/{i}"), str::to_string);
        if let (Some(v), false) = (n.vertical(), c.float) {
            let whole = |len: Len| len == Len::Pct(100.);
            // Across: the parent's stretch already gives the full extent.
            if whole(c.len(!v)) && c.cross_align(n, v) == Align::Stretch {
                out.push(Lint::Redundant {
                    at: here.clone(),
                    vertical: !v,
                });
            }
            // Along: a scroller lays children past its frame on purpose.
            if whole(c.len(v)) && flow > 1 && !n.scroll {
                out.push(Lint::WholeLine {
                    at: here.clone(),
                    vertical: v,
                });
            }
        }
        walk(c, &here, is_text, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{block, col, row};

    #[test]
    fn hand_sizes_the_solver_would_give_are_reported() {
        let full = || block(Len::Pct(100.), 10.);
        let found = lint(
            &row([
                full().id("a"),
                block(10., 10.),
                col([full().id("b")]).id("c"),
            ]),
            |_| false,
        );
        assert_eq!(
            found,
            [
                Lint::WholeLine {
                    at: "a".into(),
                    vertical: false
                },
                Lint::Redundant {
                    at: "b".into(),
                    vertical: false
                },
            ]
        );
        // Placed elsewhere, the 100% is the declaration doing work.
        assert!(lint(&col([full().right()]), |_| false).is_empty());
        assert!(lint(&row([full()]), |_| false).is_empty());
    }
}
