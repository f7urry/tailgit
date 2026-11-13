//! Commit graph layout for the log view.
//!
//! A port of IntelliJ's `platform/vcs-log/graph`:
//! * `PermanentLinearGraphBuilder`: rows with parent/child links (duplicates removed),
//! * `GraphLayoutBuilder`: a "layout index" per commit computed by a first-parent DFS
//!   from heads sorted by branch priority, so main lines get the lowest index (leftmost),
//! * `PrintElementGeneratorImpl`: per row, the node plus the edges passing through it are
//!   sorted by layout index; the position in that order is the lane. Edges spanning
//!   [`LONG_EDGE`] rows or more are hidden in the middle and drawn as arrows at both ends,
//! * `GraphColorManagerImpl` / `DefaultColorGenerator`: lane colors.

use std::collections::HashMap;

/// Edges spanning at least this many rows are collapsed to arrow stubs.
pub const LONG_EDGE: usize = 30;

/// Input: the parents of each row, as row indices. `None` marks a parent that is not
/// loaded (outside the loaded range or filtered out).
pub type ParentRows = Vec<Vec<Option<usize>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parent {
    Row(u32),
    Unloaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Element {
    Node(u32),
    Edge(u32, u32),
}

/// How a line segment ends at the row boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnd {
    /// Continues into the neighbouring row at `to_lane`.
    Normal,
    /// A long edge whose middle is hidden: draw an arrow.
    Arrow,
    /// Parent not loaded: a short dangling stub.
    Dangling,
}

/// Half of an edge inside one row, from the row center to the row's top or bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub from_lane: usize,
    /// Lane in the neighbouring row (same as `from_lane` for arrows/stubs).
    pub to_lane: usize,
    /// True for the upper half (towards the previous row).
    pub up: bool,
    pub color: i32,
    pub end: LineEnd,
}

/// Everything needed to paint one row of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowPrint {
    pub lanes: usize,
    pub node_lane: usize,
    pub node_color: i32,
    pub lines: Vec<Line>,
}

pub struct Graph {
    parents: Vec<Vec<Parent>>,
    children: Vec<Vec<u32>>,
    layout: Vec<u32>,
    /// Visible edges passing through each row (excluding edges ending at the row).
    passing: Vec<Vec<(u32, u32)>>,
    /// (first layout index, head row, color id of head), sorted by layout index.
    heads: Vec<(u32, u32, i32)>,
    max_lanes: usize,
}

/// Java's `String.hashCode`, used for branch colors like IntelliJ.
pub fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, u| h.wrapping_mul(31).wrapping_add(u as i32))
}

impl Graph {
    /// Builds the layout.
    ///
    /// `heads` are branch heads in layout priority order (most important first), with the
    /// name of their most important ref. Childless rows are added after them as heads.
    pub fn build(parent_rows: ParentRows, heads: &[(usize, String)]) -> Graph {
        let n = parent_rows.len();
        let mut parents: Vec<Vec<Parent>> = Vec::with_capacity(n);
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (row, ps) in parent_rows.into_iter().enumerate() {
            let mut list: Vec<Parent> = Vec::with_capacity(ps.len());
            for p in ps {
                let parent = match p {
                    Some(pr) if pr > row && pr < n => Parent::Row(pr as u32),
                    _ => Parent::Unloaded,
                };
                // DuplicateParentFixer
                if parent == Parent::Unloaded || !list.contains(&parent) {
                    list.push(parent);
                }
            }
            for p in &list {
                if let Parent::Row(pr) = p {
                    children[*pr as usize].push(row as u32);
                }
            }
            parents.push(list);
        }

        // Layout index: DFS along the first unvisited parent from each head.
        let mut layout = vec![0u32; n];
        let mut head_list: Vec<(u32, u32, i32)> = Vec::new();
        let mut current: u32 = 1;
        let mut stack: Vec<u32> = Vec::new();
        let head_rows = heads
            .iter()
            .map(|(r, name)| (*r, Some(name.as_str())))
            .chain((0..n).filter(|r| children[*r].is_empty()).map(|r| (r, None)))
            .chain((0..n).map(|r| (r, None)));
        for (head, name) in head_rows {
            if head >= n || layout[head] != 0 {
                continue;
            }
            let color = name.map(java_hash).unwrap_or(current as i32);
            head_list.push((current, head as u32, color));
            stack.push(head as u32);
            while let Some(&node) = stack.last() {
                let node = node as usize;
                let first_visit = layout[node] == 0;
                if first_visit {
                    layout[node] = current;
                }
                let next = parents[node].iter().find_map(|p| match p {
                    Parent::Row(pr) if layout[*pr as usize] == 0 => Some(*pr),
                    _ => None,
                });
                match next {
                    Some(pr) => stack.push(pr),
                    None => {
                        stack.pop();
                        if first_visit {
                            current += 1;
                        }
                    }
                }
            }
        }
        head_list.sort_by_key(|h| h.0);

        // Visible passing edges.
        let mut passing: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n];
        for (row, ps) in parents.iter().enumerate() {
            for p in ps {
                if let Parent::Row(pr) = *p {
                    let pr = pr as usize;
                    if pr <= row + 1 {
                        continue;
                    }
                    let e = (row as u32, pr as u32);
                    if pr - row < LONG_EDGE {
                        for r in passing.iter_mut().take(pr).skip(row + 1) {
                            r.push(e);
                        }
                    } else {
                        passing[row + 1].push(e);
                        passing[pr - 1].push(e);
                    }
                }
            }
        }
        let max_lanes = passing.iter().map(|p| p.len() + 1).max().unwrap_or(0);

        Graph {
            parents,
            children,
            layout,
            passing,
            heads: head_list,
            max_lanes,
        }
    }

    pub fn len(&self) -> usize {
        self.parents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parents.is_empty()
    }

    /// The widest row, in lanes.
    pub fn max_lanes(&self) -> usize {
        self.max_lanes
    }

    pub fn layout_index(&self, row: usize) -> u32 {
        self.layout[row]
    }

    fn key(&self, e: Element) -> (u32, u32, u32) {
        match e {
            Element::Node(r) => (self.layout[r as usize], r, r),
            Element::Edge(u, d) => (
                self.layout[u as usize].max(self.layout[d as usize]),
                u,
                d,
            ),
        }
    }

    fn elements(&self, row: usize) -> Vec<Element> {
        let mut v: Vec<Element> = self.passing[row]
            .iter()
            .map(|&(u, d)| Element::Edge(u, d))
            .collect();
        v.push(Element::Node(row as u32));
        v.sort_by_key(|e| self.key(*e));
        v
    }

    /// Color id of a node: the head's ref color on the head's own fragment,
    /// otherwise the fragment's layout index (`GraphColorManagerImpl`).
    pub fn node_color(&self, row: usize) -> i32 {
        let li = self.layout[row];
        let idx = self.heads.partition_point(|h| h.0 <= li);
        match idx.checked_sub(1).map(|i| self.heads[i]) {
            Some((head_li, _, color)) if head_li == li => color,
            _ => li as i32,
        }
    }

    fn edge_color(&self, u: u32, d: u32) -> i32 {
        if self.layout[d as usize] > self.layout[u as usize] {
            self.node_color(d as usize)
        } else {
            self.node_color(u as usize)
        }
    }

    /// Computes the lanes and line segments of one row.
    pub fn row(&self, row: usize) -> RowPrint {
        let here = self.elements(row);
        let above = (row > 0).then(|| self.elements(row - 1));
        let below = (row + 1 < self.len()).then(|| self.elements(row + 1));
        let lane_in = |list: &Option<Vec<Element>>, e: Element| {
            list.as_ref().and_then(|l| l.iter().position(|x| *x == e))
        };
        let mut lines = Vec::new();
        let mut node_lane = 0;
        let mut push = |from: usize, target: Option<usize>, up: bool, color: i32, missing: LineEnd| {
            let (to_lane, end) = match target {
                Some(t) => (t, LineEnd::Normal),
                None => (from, missing),
            };
            lines.push(Line {
                from_lane: from,
                to_lane,
                up,
                color,
                end,
            });
        };
        for (lane, e) in here.iter().enumerate() {
            match *e {
                Element::Node(r) => {
                    node_lane = lane;
                    for &c in &self.children[r as usize] {
                        let target = if c + 1 == r {
                            lane_in(&above, Element::Node(c))
                        } else {
                            lane_in(&above, Element::Edge(c, r))
                        };
                        push(lane, target, true, self.edge_color(c, r), LineEnd::Arrow);
                    }
                    for p in &self.parents[r as usize] {
                        match *p {
                            Parent::Row(pr) => {
                                let target = if pr == r + 1 {
                                    lane_in(&below, Element::Node(pr))
                                } else {
                                    lane_in(&below, Element::Edge(r, pr))
                                };
                                push(lane, target, false, self.edge_color(r, pr), LineEnd::Arrow);
                            }
                            Parent::Unloaded => push(
                                lane,
                                None,
                                false,
                                self.node_color(r as usize),
                                LineEnd::Dangling,
                            ),
                        }
                    }
                }
                Element::Edge(u, d) => {
                    let color = self.edge_color(u, d);
                    let up_target = if u as usize + 1 == row {
                        lane_in(&above, Element::Node(u))
                    } else {
                        lane_in(&above, *e)
                    };
                    push(lane, up_target, true, color, LineEnd::Arrow);
                    let down_target = if d as usize == row + 1 {
                        lane_in(&below, Element::Node(d))
                    } else {
                        lane_in(&below, *e)
                    };
                    push(lane, down_target, false, color, LineEnd::Arrow);
                }
            }
        }
        RowPrint {
            lanes: here.len(),
            node_lane,
            node_color: self.node_color(row),
            lines,
        }
    }
}

/// Builds graph input from commit hashes and parent hashes.
pub fn parent_rows<'a, I>(hashes: &[&'a str], parents: I) -> ParentRows
where
    I: IntoIterator<Item = Vec<&'a str>>,
{
    let index: HashMap<&str, usize> = hashes.iter().enumerate().map(|(i, h)| (*h, i)).collect();
    parents
        .into_iter()
        .map(|ps| ps.into_iter().map(|p| index.get(p).copied()).collect())
        .collect()
}

/// Converts a color id to RGB like IntelliJ's `DefaultColorGenerator`:
/// derive a hue from the id, then use saturation 0.4 and brightness 0.65.
pub fn color_rgb(id: i32) -> [u8; 3] {
    if id == 0 {
        return [128, 128, 128];
    }
    let fix = |n: i32| (n % 100).abs() + 70;
    let r = fix(id.wrapping_mul(200).wrapping_add(30)) as f32 / 255.0;
    let g = fix(id.wrapping_mul(130).wrapping_add(50)) as f32 / 255.0;
    let b = fix(id.wrapping_mul(90).wrapping_add(100)) as f32 / 255.0;
    let hue = rgb_hue(r, g, b);
    hsv_to_rgb(hue, 0.4, 0.65)
}

fn rgb_hue(r: f32, g: f32, b: f32) -> f32 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d == 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h / 6.0
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h6 = (h.fract() * 6.0).max(0.0);
    let i = h6.floor() as i32 % 6;
    let f = h6 - h6.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(v: &[&[usize]]) -> ParentRows {
        v.iter().map(|ps| ps.iter().map(|p| Some(*p)).collect()).collect()
    }

    #[test]
    fn linear_history_is_one_lane() {
        let g = Graph::build(rows(&[&[1], &[2], &[]]), &[(0, "main".into())]);
        for r in 0..3 {
            let p = g.row(r);
            assert_eq!(p.lanes, 1);
            assert_eq!(p.node_lane, 0);
        }
        let mid = g.row(1);
        assert_eq!(mid.lines.len(), 2);
        assert!(mid.lines.iter().all(|l| l.end == LineEnd::Normal && l.to_lane == 0));
        assert_eq!(g.node_color(0), g.node_color(2));
        assert_eq!(g.node_color(0), java_hash("main"));
    }

    #[test]
    fn branch_and_merge_keeps_first_parent_left() {
        // 0: merge(1, 2); 1: A -> 3; 2: B -> 3; 3: root
        let g = Graph::build(rows(&[&[1, 2], &[3], &[3], &[]]), &[(0, "main".into())]);
        assert_eq!(
            [0, 1, 2, 3].map(|r| g.layout_index(r)),
            [1, 1, 2, 1]
        );
        let r0 = g.row(0);
        assert_eq!(r0.lanes, 1);
        // merge node has two downward lines: to node 1 (lane 0) and to edge (0,2) (lane 1)
        let mut downs: Vec<usize> = r0.lines.iter().filter(|l| !l.up).map(|l| l.to_lane).collect();
        downs.sort();
        assert_eq!(downs, vec![0, 1]);
        let r1 = g.row(1);
        assert_eq!((r1.lanes, r1.node_lane), (2, 0));
        let r2 = g.row(2);
        assert_eq!((r2.lanes, r2.node_lane), (2, 1));
        let r3 = g.row(3);
        let mut ups: Vec<usize> = r3.lines.iter().filter(|l| l.up).map(|l| l.to_lane).collect();
        ups.sort();
        assert_eq!(ups, vec![0, 1]);
        assert_ne!(g.node_color(2), g.node_color(1));
    }

    #[test]
    fn two_heads_priority_decides_left_lane() {
        // 0: feature tip -> 2; 1: main tip -> 2; 2: root
        let input = rows(&[&[2], &[2], &[]]);
        let g = Graph::build(input.clone(), &[(1, "main".into()), (0, "feature".into())]);
        assert!(g.layout_index(1) < g.layout_index(0));
        let r1 = g.row(1);
        // row 1 holds main's node and the feature edge (0,2); main is left.
        assert_eq!(r1.node_lane, 0);
        let g2 = Graph::build(input, &[(0, "feature".into()), (1, "main".into())]);
        assert_eq!(g2.row(1).node_lane, 1);
    }

    #[test]
    fn unloaded_parents_dangle() {
        let g = Graph::build(vec![vec![Some(1)], vec![None]], &[]);
        let r1 = g.row(1);
        assert!(r1.lines.iter().any(|l| l.end == LineEnd::Dangling && !l.up));
    }

    #[test]
    fn octopus_merge() {
        let g = Graph::build(
            rows(&[&[1, 2, 3], &[4], &[4], &[4], &[]]),
            &[(0, "main".into())],
        );
        assert_eq!(g.row(0).lines.iter().filter(|l| !l.up).count(), 3);
        assert_eq!(g.row(1).lanes, 3);
        assert_eq!(g.row(4).lines.iter().filter(|l| l.up).count(), 3);
    }

    #[test]
    fn long_edges_collapse_to_arrows() {
        let n = LONG_EDGE + 5;
        // row 0 -> row n-1 directly, plus a linear chain 1..n-1
        let mut v: ParentRows = Vec::new();
        v.push(vec![Some(n - 1)]);
        for r in 1..n - 1 {
            v.push(vec![Some(r + 1)]);
        }
        v.push(vec![]);
        let g = Graph::build(v, &[(1, "main".into()), (0, "side".into())]);
        let r1 = g.row(1);
        assert!(r1.lines.iter().any(|l| l.end == LineEnd::Arrow && !l.up));
        assert_eq!(g.row(n / 2).lanes, 1, "middle of long edge hidden");
        let rl = g.row(n - 2);
        assert!(rl.lines.iter().any(|l| l.end == LineEnd::Arrow && l.up));
    }

    #[test]
    fn parent_rows_maps_hashes() {
        let p = parent_rows(&["a", "b"], vec![vec!["b", "zz"], vec![]]);
        assert_eq!(p, vec![vec![Some(1), None], vec![]]);
    }

    #[test]
    fn colors_are_muted() {
        let c = color_rgb(java_hash("main"));
        assert!(c.iter().all(|v| *v <= 166));
        assert_eq!(color_rgb(0), [128, 128, 128]);
    }
}
