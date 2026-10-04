//! Explicit tracks and sparse grid placement.
use crate::{Error, Limits, measure::Measured};

/// An explicit grid track. Percentages use the grid's corresponding inner axis; without
/// a definite extent they behave as auto tracks. `Fr` has a min-content floor,
/// while `MinFr` uses the supplied pixel floor. `Auto` hugs max-content and
/// stretches into space remaining after fractional tracks.
///
/// Children flow in declaration/order order unless positioned with
/// [`crate::Node::grid_at`]. Column and row spans reserve adjacent cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrack {
    Px(f64),
    Pct(f64),
    Fr(f64),
    Auto,
    MinContent,
    /// Max-content sized, without auto-track stretching or shrinking.
    MaxContent,
    /// A content-sized track bounded by pixel minimum and maximum.
    MinMax {
        min: f64,
        max: f64,
    },
    /// A fractional track with an explicit pixel minimum, like minmax(0,1fr).
    MinFr {
        min: f64,
        fr: f64,
    },
}
impl GridTrack {
    pub(crate) fn valid(self, limits: Limits) -> bool {
        let valid = |v: f64| v.is_finite() && (0.0..=limits.extent).contains(&v);
        match self {
            Self::Px(v) | Self::Pct(v) | Self::Fr(v) => valid(v),
            Self::MinMax { min, max } => valid(min) && valid(max) && min <= max,
            Self::MinFr { min, fr } => valid(min) && valid(fr),
            Self::Auto | Self::MinContent | Self::MaxContent => true,
        }
    }
    fn fraction(self) -> f64 {
        match self {
            Self::Fr(fr) | Self::MinFr { fr, .. } => fr,
            _ => 0.0,
        }
    }
    fn cap(self) -> f64 {
        match self {
            Self::MinMax { max, .. } => max,
            _ => f64::INFINITY,
        }
    }
    fn content_floor(self, definite: bool) -> bool {
        matches!(
            self,
            Self::Auto | Self::Fr(_) | Self::MinContent | Self::MaxContent
        ) || matches!(self, Self::Pct(_)) && !definite
    }
    fn content_hug(self, definite: bool) -> bool {
        self.content_floor(definite) || matches!(self, Self::MinMax { .. } | Self::MinFr { .. })
    }
}

/// The minimum and max-content track contributions, including spanning items.
/// Fixed tracks keep their authored sizes; spanning excess goes into content
/// tracks rather than enlarging fixed neighbors.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub(crate) index: usize,
    pub(crate) column: usize,
    pub(crate) row: usize,
    pub(crate) column_span: usize,
    pub(crate) row_span: usize,
}

/// Reserve explicitly positioned items first, then auto-place remaining items
/// in sparse row-major order, skipping cells occupied by either kind of span.
pub(crate) fn placements<P>(
    flow: &[&Measured<'_, P>],
    cols: usize,
    limit: usize,
) -> Result<Vec<Placement>, Error> {
    let limit = limit.min(isize::MAX as usize / size_of::<GridTrack>());
    let mut occupied = rustc_hash::FxHashSet::default();
    let mut result = vec![None; flow.len()];
    let mut cursor = (0, 0);
    for explicit in [true, false] {
        for (i, c) in flow.iter().enumerate() {
            let position = c.node.rare().grid_position;
            if position.is_some() != explicit {
                continue;
            }
            let column_span = c.node.span.clamp(1, cols);
            let row_span = c.node.rare().grid_row_span;
            let (mut column, mut row) = position.map_or(cursor, |[col, row]| (col, row));
            if column >= cols
                || column_span > cols - column
                || row > limit
                || row_span > limit - row
            {
                if explicit {
                    return Err(Error::InvalidValue);
                }
                column = 0;
                row += 1;
            }
            if !explicit {
                loop {
                    if row >= limit || row_span > limit - row {
                        return Err(Error::BudgetExceeded);
                    }
                    if column + column_span <= cols
                        && (row..row + row_span).all(|r| {
                            (column..column + column_span).all(|col| !occupied.contains(&(col, r)))
                        })
                    {
                        break;
                    }
                    column += 1;
                    if column >= cols {
                        column = 0;
                        row += 1;
                    }
                }
            }
            for r in row..row + row_span {
                for col in column..column + column_span {
                    if occupied.insert((col, r)) && occupied.len() > limit {
                        return Err(Error::BudgetExceeded);
                    }
                }
            }
            result[i] = Some(Placement {
                index: c.index,
                column,
                row,
                column_span,
                row_span,
            });
            if !explicit {
                cursor = (column + column_span, row);
                if cursor.0 >= cols {
                    cursor = (0, row + 1);
                }
            }
        }
    }
    Ok(result.into_iter().map(Option::unwrap).collect())
}

pub(crate) fn tracks<P>(
    tracks: &[GridTrack],
    flow: &[&Measured<'_, P>],
    placements: &[Placement],
    gap: f64,
    available: Option<f64>,
    minimum_only: bool,
    vertical: bool,
) -> Vec<f64> {
    let definite = available.is_some();
    let mut minimum: Vec<_> = tracks
        .iter()
        .map(|t| match *t {
            GridTrack::Px(v) => v,
            GridTrack::Pct(p) => available.map_or(0.0, |w| w * p / 100.0),
            GridTrack::MinMax { min, .. } | GridTrack::MinFr { min, .. } => min,
            _ => 0.0,
        })
        .collect();
    let mut desired = minimum.clone();
    // Single-span contributions settle first; spans then pay only their excess.
    for spanning in [false, true] {
        for (c, p) in flow.iter().zip(placements) {
            let (start, span) = if vertical {
                (p.row, p.row_span)
            } else {
                (p.column, p.column_span)
            };
            let end = start + span;
            if (span > 1) == spanning {
                contribute(
                    &mut minimum[start..end],
                    &tracks[start..end],
                    (c.floor.main(vertical) - gap * (span - 1) as f64).max(0.0),
                    |t| t.content_floor(definite),
                );
                for i in start..end {
                    desired[i] = desired[i].max(minimum[i]);
                }
                contribute(
                    &mut desired[start..end],
                    &tracks[start..end],
                    (c.size.main(vertical) - gap * (span - 1) as f64).max(0.0),
                    |t| t.content_hug(definite) && !matches!(t, GridTrack::MinContent),
                );
            }
        }
    }
    for (i, track) in tracks.iter().enumerate() {
        if matches!(track, GridTrack::MaxContent) {
            minimum[i] = desired[i];
        }
    }
    if minimum_only {
        return minimum;
    }
    let Some(available) = available else {
        let fraction = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.fraction() > 0.0)
            .map(|(i, t)| desired[i] / t.fraction())
            .fold(0.0, f64::max);
        for (i, t) in tracks.iter().enumerate() {
            if matches!(t, GridTrack::Fr(_) | GridTrack::MinFr { .. }) {
                desired[i] = (fraction * t.fraction()).max(minimum[i]);
            }
        }
        return desired;
    };
    let mut widths: Vec<_> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if matches!(t, GridTrack::Fr(_) | GridTrack::MinFr { .. }) {
                minimum[i]
            } else {
                desired[i]
            }
        })
        .collect();
    let available = (available - gap * tracks.len().saturating_sub(1) as f64).max(0.0);
    // Constrained auto/content tracks may shrink from max-content to their
    // min-content floor before fractions receive the remaining space.
    let mut deficit = widths.iter().sum::<f64>() - available;
    let mut shrinking: Vec<_> = (0..tracks.len())
        .filter(|&i| {
            matches!(tracks[i], GridTrack::Auto | GridTrack::MinMax { .. })
                && widths[i] > minimum[i]
        })
        .collect();
    while deficit > 1e-9 && !shrinking.is_empty() {
        let share = deficit / shrinking.len() as f64;
        for &i in &shrinking {
            let delta = share.min(widths[i] - minimum[i]);
            widths[i] -= delta;
            deficit -= delta;
        }
        shrinking.retain(|&i| widths[i] > minimum[i] + 1e-9);
    }

    let mut active: Vec<_> = (0..tracks.len())
        .filter(|&i| tracks[i].fraction() > 0.0)
        .collect();
    // Fraction sizing freezes tracks whose minimum exceeds their share.
    // ponytail: linear active-set scans give O(n²) worst-case sizing; use a
    // sorted active set if UI grids develop thousands of constrained tracks.
    while !active.is_empty() {
        let fixed = widths.iter().sum::<f64>() - active.iter().map(|&i| widths[i]).sum::<f64>();
        let total: f64 = active.iter().map(|&i| tracks[i].fraction()).sum();
        let fraction = (available - fixed).max(0.0) / total.max(1.0);
        let too_small: Vec<_> = active
            .iter()
            .copied()
            .filter(|&i| fraction * tracks[i].fraction() < minimum[i])
            .collect();
        if too_small.is_empty() {
            for &i in &active {
                widths[i] = fraction * tracks[i].fraction();
            }
            break;
        }
        active.retain(|i| !too_small.contains(i));
    }
    // Auto/minmax tracks stretch only into space left by fractional tracks.
    let stretch = |t| {
        matches!(t, GridTrack::Auto | GridTrack::MinMax { .. })
            || matches!(t, GridTrack::Pct(_)) && !definite
    };
    contribute(&mut widths, tracks, available, stretch);
    widths
}

fn contribute(
    widths: &mut [f64],
    tracks: &[GridTrack],
    total: f64,
    eligible: impl Fn(GridTrack) -> bool,
) {
    let mut excess = total - widths.iter().sum::<f64>();
    let mut active: Vec<_> = (0..widths.len())
        .filter(|&i| eligible(tracks[i]) && widths[i] < tracks[i].cap())
        .collect();
    while excess > 1e-9 && !active.is_empty() {
        let share = excess / active.len() as f64;
        for &i in &active {
            let delta = share.min(tracks[i].cap() - widths[i]);
            widths[i] += delta;
            excess -= delta;
        }
        active.retain(|&i| widths[i] + 1e-9 < tracks[i].cap());
    }
}
