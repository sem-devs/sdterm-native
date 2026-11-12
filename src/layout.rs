//! How a workspace arranges its terminals.
//!
//! The Electron app used CSS grid cells; here a layout is a small tree of splits that the window
//! draws with nested flex boxes. The rules are the same: what "auto" means for a given count, a
//! short last row that stretches, quarters with open spots, one big terminal and a few small ones.

use crate::model::LayoutId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Children side by side (columns).
    Horizontal,
    /// Children stacked (rows).
    Vertical,
}

/// The draggable line between the first child of a split and the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Divider {
    /// Where the line is, as a fraction from the left or top edge.
    pub at: f32,
    /// Dragging towards the start grows the main area (main on the right).
    pub from_end: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// The terminal at this position in the workspace's list.
    Pane(usize),
    /// An open spot (quarters): one click starts a terminal there.
    Open,
    Split {
        axis: Axis,
        divider: Option<Divider>,
        /// Each child with its share of the space.
        children: Vec<(f32, Node)>,
    },
}

/// What "auto" means for a given number of terminals; quarters give way to a grid past four.
pub fn resolve(layout: LayoutId, count: usize) -> LayoutId {
    match layout {
        LayoutId::Quarters if count > 4 => LayoutId::Grid,
        LayoutId::Auto => match count {
            0..=2 => LayoutId::Columns,
            3 => LayoutId::MainLeft,
            _ => LayoutId::Grid,
        },
        other => other,
    }
}

const MIN_SPLIT: f32 = 0.2;
const MAX_SPLIT: f32 = 0.8;

fn clamp_split(split: Option<f64>, default: f32) -> f32 {
    split.map_or(default, |s| s as f32).clamp(MIN_SPLIT, MAX_SPLIT)
}

fn equal(axis: Axis, nodes: Vec<Node>) -> Node {
    if nodes.len() == 1 {
        return nodes.into_iter().next().unwrap();
    }
    Node::Split {
        axis,
        divider: None,
        children: nodes.into_iter().map(|n| (1.0, n)).collect(),
    }
}

pub fn compute(layout: LayoutId, count: usize, split: Option<f64>) -> Node {
    let n = count.max(1);
    let layout = resolve(layout, n);
    if n == 1 && layout != LayoutId::Quarters {
        return Node::Pane(0);
    }
    let panes = |from: usize| (from..n).map(Node::Pane).collect::<Vec<_>>();

    match layout {
        LayoutId::Columns | LayoutId::Rows => {
            let axis = if layout == LayoutId::Columns { Axis::Horizontal } else { Axis::Vertical };
            if n == 2 {
                let s = clamp_split(split, 0.5);
                Node::Split {
                    axis,
                    divider: Some(Divider { at: s, from_end: false }),
                    children: vec![(s, Node::Pane(0)), (1.0 - s, Node::Pane(1))],
                }
            } else {
                equal(axis, panes(0))
            }
        }
        LayoutId::Grid => {
            let cols = match n {
                1..=2 => n,
                3..=4 => 2,
                _ => 3,
            };
            let mut rows = Vec::new();
            for start in (0..n).step_by(cols) {
                let end = (start + cols).min(n);
                let k = end - start;
                let mut children: Vec<(f32, Node)> =
                    (start..end).map(|i| (1.0, Node::Pane(i))).collect();
                // A short last row: its last terminal stretches over the missing columns.
                if k < cols {
                    if let Some(last) = children.last_mut() {
                        last.0 = (cols - k + 1) as f32;
                    }
                }
                rows.push(if k == 1 {
                    children.remove(0).1
                } else {
                    Node::Split { axis: Axis::Horizontal, divider: None, children }
                });
            }
            equal(Axis::Vertical, rows)
        }
        LayoutId::Quarters => {
            // Always four equal spots in reading order. Spots without a terminal stay open.
            let spot = |i: usize| if i < n { Node::Pane(i) } else { Node::Open };
            equal(
                Axis::Vertical,
                vec![
                    equal(Axis::Horizontal, vec![spot(0), spot(1)]),
                    equal(Axis::Horizontal, vec![spot(2), spot(3)]),
                ],
            )
        }
        LayoutId::MainLeft | LayoutId::MainRight => {
            let s = clamp_split(split, 0.6);
            let rest = equal(Axis::Vertical, panes(1));
            if layout == LayoutId::MainLeft {
                Node::Split {
                    axis: Axis::Horizontal,
                    divider: Some(Divider { at: s, from_end: false }),
                    children: vec![(s, Node::Pane(0)), (1.0 - s, rest)],
                }
            } else {
                Node::Split {
                    axis: Axis::Horizontal,
                    divider: Some(Divider { at: 1.0 - s, from_end: true }),
                    children: vec![(1.0 - s, rest), (s, Node::Pane(0))],
                }
            }
        }
        LayoutId::MainTop => {
            let s = clamp_split(split, 0.6);
            Node::Split {
                axis: Axis::Vertical,
                divider: Some(Divider { at: s, from_end: false }),
                children: vec![
                    (s, Node::Pane(0)),
                    (1.0 - s, equal(Axis::Horizontal, panes(1))),
                ],
            }
        }
        LayoutId::Auto => unreachable!("resolved above"),
    }
}

/// The split to store after the divider was dragged to `pointer` (a fraction of the container).
pub fn split_from_pointer(divider: Divider, pointer: f32) -> f64 {
    let s = if divider.from_end { 1.0 - pointer } else { pointer };
    s.clamp(MIN_SPLIT, MAX_SPLIT) as f64
}

/// The terminals a tree draws, in the order they appear, and how many open spots it has.
pub fn leaves(node: &Node) -> (Vec<usize>, usize) {
    fn walk(node: &Node, panes: &mut Vec<usize>, open: &mut usize) {
        match node {
            Node::Pane(i) => panes.push(*i),
            Node::Open => *open += 1,
            Node::Split { children, .. } => children.iter().for_each(|(_, c)| walk(c, panes, open)),
        }
    }
    let (mut panes, mut open) = (Vec::new(), 0);
    walk(node, &mut panes, &mut open);
    (panes, open)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LAYOUTS;

    fn shown(node: &Node) -> Vec<usize> {
        let mut v = leaves(node).0;
        v.sort_unstable();
        v
    }

    #[test]
    fn every_layout_shows_every_terminal_exactly_once() {
        for layout in LAYOUTS {
            for n in 1..=9 {
                for split in [None, Some(0.3)] {
                    let tree = compute(layout, n, split);
                    assert_eq!(shown(&tree), (0..n).collect::<Vec<_>>(), "{layout:?} with {n}");
                }
            }
        }
    }

    #[test]
    fn auto_follows_the_count() {
        assert_eq!(resolve(LayoutId::Auto, 1), LayoutId::Columns);
        assert_eq!(resolve(LayoutId::Auto, 2), LayoutId::Columns);
        assert_eq!(resolve(LayoutId::Auto, 3), LayoutId::MainLeft);
        assert_eq!(resolve(LayoutId::Auto, 4), LayoutId::Grid);
        assert_eq!(resolve(LayoutId::Auto, 9), LayoutId::Grid);
    }

    #[test]
    fn quarters_keep_four_spots_and_open_the_free_ones() {
        for n in 1..=4 {
            let (panes, open) = leaves(&compute(LayoutId::Quarters, n, None));
            assert_eq!(panes.len() + open, 4, "{n} terminals");
            assert_eq!(open, 4 - n);
        }
        // Past four it becomes a grid with no open spots.
        assert_eq!(leaves(&compute(LayoutId::Quarters, 5, None)).1, 0);
    }

    #[test]
    fn a_short_last_row_stretches() {
        // Five terminals in a grid of three columns: the last row has two, the last one wider.
        let Node::Split { children, .. } = compute(LayoutId::Grid, 5, None) else { panic!() };
        let Node::Split { children: last_row, .. } = &children[1].1 else { panic!() };
        assert_eq!(last_row.len(), 2);
        assert_eq!(last_row[0].0, 1.0);
        assert_eq!(last_row[1].0, 2.0);
    }

    #[test]
    fn only_two_region_layouts_have_a_divider() {
        let has = |layout, n| matches!(compute(layout, n, None), Node::Split { divider: Some(_), .. });
        assert!(has(LayoutId::Columns, 2));
        assert!(has(LayoutId::Rows, 2));
        assert!(!has(LayoutId::Columns, 3));
        assert!(has(LayoutId::MainLeft, 4));
        assert!(has(LayoutId::MainRight, 4));
        assert!(has(LayoutId::MainTop, 4));
        assert!(!has(LayoutId::Grid, 4));
        assert!(!has(LayoutId::Quarters, 2));
    }

    #[test]
    fn dragging_the_divider_maps_back_to_a_split() {
        let left = Divider { at: 0.6, from_end: false };
        assert!((split_from_pointer(left, 0.45) - 0.45).abs() < 1e-6);
        // Main on the right: dragging towards the start grows the main area.
        let right = Divider { at: 0.4, from_end: true };
        assert!((split_from_pointer(right, 0.3) - 0.7).abs() < 1e-6);
        // Never below or above the limits.
        assert!((split_from_pointer(left, 0.01) - 0.2).abs() < 1e-6);
        assert!((split_from_pointer(left, 0.99) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn the_stored_split_is_kept_within_limits() {
        let Node::Split { children, .. } = compute(LayoutId::Columns, 2, Some(0.95)) else { panic!() };
        assert!((children[0].0 - 0.8).abs() < 1e-6);
    }
}
