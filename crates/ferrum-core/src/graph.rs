//! Lays out the commit graph: which lane each commit sits in and which
//! lines connect the rows.
//!
//! Each row is split in two halves. `Line::Top` goes from the top edge of the
//! row to its center, `Line::Bottom` from the center to the bottom edge, so
//! consecutive rows join seamlessly.

use crate::{CommitInfo, Oid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub half: Half,
    pub from: usize,
    pub to: usize,
    /// Color index, stable for a lane until it ends.
    pub color: usize,
}

#[derive(Debug, Clone, Default)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub rows: Vec<GraphRow>,
    /// Widest row, in lanes.
    pub width: usize,
}

#[derive(Clone, Copy)]
struct Lane {
    /// Commit this lane is waiting for.
    expects: Oid,
    color: usize,
}

/// Builds the graph for commits in topological order (children before parents).
pub fn layout(commits: &[CommitInfo]) -> Graph {
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut next_color = 0;
    let mut graph = Graph::default();

    let mut new_color = || {
        let c = next_color;
        next_color += 1;
        c
    };

    for commit in commits {
        let mut lines = Vec::new();
        // Lanes that start in this row (they have no line above the center).
        let mut opened = vec![];

        // Lane that holds this commit: the first one waiting for it, or a free slot.
        let own = lanes
            .iter()
            .position(|l| l.is_some_and(|l| l.expects == commit.id));
        let (lane, color) = match own {
            Some(i) => (i, lanes[i].unwrap().color),
            None => {
                let i = free_slot(&mut lanes);
                (i, new_color())
            }
        };

        // Upper half: lanes waiting for this commit converge into it, others pass by.
        for (i, slot) in lanes.iter_mut().enumerate() {
            let Some(l) = *slot else { continue };
            if l.expects == commit.id {
                lines.push(Line {
                    half: Half::Top,
                    from: i,
                    to: lane,
                    color: l.color,
                });
                *slot = None;
            } else {
                lines.push(Line {
                    half: Half::Top,
                    from: i,
                    to: i,
                    color: l.color,
                });
            }
        }

        // Lower half: the first parent always continues this lane, so branches run
        // in parallel until their common ancestor, where they converge. Other
        // parents (merges) join a lane already waiting for them, or open one.
        for (n, &parent) in commit.parents.iter().enumerate() {
            let existing = lanes
                .iter()
                .position(|l| l.is_some_and(|l| l.expects == parent));
            let (target, line_color) = match existing {
                _ if n == 0 => {
                    lanes[lane] = Some(Lane {
                        expects: parent,
                        color,
                    });
                    (lane, color)
                }
                Some(i) => (i, lanes[i].unwrap().color),
                None => {
                    let i = free_slot(&mut lanes);
                    let c = new_color();
                    lanes[i] = Some(Lane {
                        expects: parent,
                        color: c,
                    });
                    opened.push(i);
                    (i, c)
                }
            };
            lines.push(Line {
                half: Half::Bottom,
                from: lane,
                to: target,
                color: line_color,
            });
        }

        // Lanes that passed by continue to the next row, even when a merge joined them.
        for (i, slot) in lanes.iter().enumerate() {
            let Some(l) = *slot else { continue };
            if i != lane && !opened.contains(&i) {
                lines.push(Line {
                    half: Half::Bottom,
                    from: i,
                    to: i,
                    color: l.color,
                });
            }
        }

        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }

        let used = lines
            .iter()
            .map(|l| l.from.max(l.to) + 1)
            .chain([lane + 1])
            .max()
            .unwrap_or(1);
        graph.width = graph.width.max(used);
        graph.rows.push(GraphRow { lane, color, lines });
    }
    graph
}

fn free_slot(lanes: &mut Vec<Option<Lane>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(n: u8) -> Oid {
        Oid::from_bytes(&[n; 20]).unwrap()
    }

    fn commit(id: u8, parents: &[u8]) -> CommitInfo {
        CommitInfo {
            id: oid(id),
            summary: String::new(),
            author: String::new(),
            email: String::new(),
            time: 0,
            offset_minutes: 0,
            parents: parents.iter().map(|&p| oid(p)).collect(),
        }
    }

    #[test]
    fn linear_history_uses_one_lane() {
        let g = layout(&[commit(3, &[2]), commit(2, &[1]), commit(1, &[])]);
        assert_eq!(g.width, 1);
        assert!(g.rows.iter().all(|r| r.lane == 0 && r.color == 0));
        // The root commit has nothing below it.
        assert!(g.rows[2].lines.iter().all(|l| l.half == Half::Top));
    }

    #[test]
    fn merge_opens_and_closes_a_lane() {
        // 4 merges 3 (feature) into 2 (main); both come from 1.
        let g = layout(&[
            commit(4, &[2, 3]),
            commit(3, &[1]),
            commit(2, &[1]),
            commit(1, &[]),
        ]);
        assert_eq!(g.width, 2);
        assert_eq!(g.rows[0].lane, 0);
        assert_eq!(g.rows[1].lane, 1, "feature commit gets its own lane");
        assert_eq!(g.rows[2].lane, 0);
        assert_eq!(g.rows[3].lane, 0);
        // Both lanes converge into the root commit.
        let tops: Vec<_> = g.rows[3]
            .lines
            .iter()
            .filter(|l| l.half == Half::Top)
            .collect();
        assert_eq!(tops.len(), 2);
        assert!(tops.iter().all(|l| l.to == 0));
        assert_ne!(g.rows[1].color, g.rows[0].color);
    }

    #[test]
    fn merge_into_passing_lane_keeps_it_going() {
        // 4 (lane 0) and 3 (lane 1) both wait for 1; 2 merges 1 while lane 1 passes by.
        let g = layout(&[
            commit(4, &[1]),
            commit(3, &[1]),
            commit(2, &[5, 1]),
            commit(5, &[]),
            commit(1, &[]),
        ]);
        let row = &g.rows[2];
        assert_eq!(row.lane, 2);
        // Merge line into lane 0, and lanes 0 and 1 still continue downward.
        assert!(
            row.lines
                .iter()
                .any(|l| l.half == Half::Bottom && l.from == 2 && l.to == 0)
        );
        for i in [0, 1] {
            assert!(
                row.lines
                    .iter()
                    .any(|l| l.half == Half::Bottom && l.from == i && l.to == i)
            );
        }
    }

    #[test]
    fn separate_tips_get_separate_lanes() {
        // Two branch tips (3 and 2) on top of 1.
        let g = layout(&[commit(3, &[1]), commit(2, &[1]), commit(1, &[])]);
        assert_eq!(g.rows[0].lane, 0);
        assert_eq!(g.rows[1].lane, 1);
        assert_eq!(g.rows[2].lane, 0);
        // Lane 0 passes by row 1.
        assert!(g.rows[1].lines.contains(&Line {
            half: Half::Top,
            from: 0,
            to: 0,
            color: 0
        }));
    }
}
