//! The three-way text merge model of IntelliJ's merge window (`MergeConflictModel`,
//! `TextMergeChange`, `MergeRangeUtil`). The result starts as the base text; each
//! change between base and the two sides is accepted, appended, ignored or
//! auto-resolved, and manual edits keep the change ranges in place.

use std::ops::Range;

use crate::merge3::{self, MergeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn idx(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// Which non-conflicting changes "Apply non-conflicting changes" applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyFrom {
    Left,
    All,
    Right,
}

/// `MergeConflictType.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Inserted,
    Deleted,
    Modified,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeChange {
    pub base: Range<usize>,
    pub left: Range<usize>,
    pub right: Range<usize>,
    pub kind: ChangeType,
    pub left_changed: bool,
    pub right_changed: bool,
    /// Resolved state per side (left, right); fully resolved when both are set.
    resolved: [bool; 2],
    /// A conflict where one side was accepted: accepting the other side appends.
    pub one_side_applied: bool,
    /// Lines of the result this change occupies.
    pub result: Range<usize>,
    /// The result lines were edited by hand (disables automatic resolution).
    pub edited: bool,
    /// Automatic (word-level) resolution of a conflict, when there is one.
    pub auto: Option<Vec<String>>,
}

impl MergeChange {
    pub fn is_conflict(&self) -> bool {
        self.kind == ChangeType::Conflict
    }
    pub fn is_resolved(&self) -> bool {
        self.resolved[0] && self.resolved[1]
    }
    pub fn is_resolved_side(&self, side: Side) -> bool {
        self.resolved[side.idx()]
    }
    pub fn is_changed(&self, side: Side) -> bool {
        match side {
            Side::Left => self.left_changed,
            Side::Right => self.right_changed,
        }
    }
    fn side_range(&self, side: Side) -> &Range<usize> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

fn eq_lines(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.trim_end_matches(['\n', '\r']) == y.trim_end_matches(['\n', '\r']))
}

/// `MergeRangeUtil.getMergeType`: (type, left changed, right changed).
pub fn classify(base: &[String], left: &[String], right: &[String]) -> (ChangeType, bool, bool) {
    use ChangeType::*;
    if base.is_empty() {
        return match (left.is_empty(), right.is_empty()) {
            (true, false) => (Inserted, false, true),
            (false, true) => (Inserted, true, false),
            (false, false) if eq_lines(left, right) => (Inserted, true, true),
            (false, false) => (Conflict, true, true),
            (true, true) => (Modified, false, false),
        };
    }
    if left.is_empty() && right.is_empty() {
        return (Deleted, true, true);
    }
    let bl = eq_lines(base, left);
    let br = eq_lines(base, right);
    match (bl, br) {
        (true, true) => (Modified, base != left, base != right),
        (true, false) => (if right.is_empty() { Deleted } else { Modified }, false, true),
        (false, true) => (if left.is_empty() { Deleted } else { Modified }, true, false),
        (false, false) if eq_lines(left, right) => (Modified, true, true),
        (false, false) => (Conflict, true, true),
    }
}

#[derive(Debug, Clone)]
pub struct MergeModel {
    pub base: Vec<String>,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub result: Vec<String>,
    pub changes: Vec<MergeChange>,
    /// Set by any edit or merge command (asks before discarding).
    pub modified: bool,
    /// A whole-file modify/delete conflict: nothing resolves automatically.
    pub file_deleted: bool,
}

impl MergeModel {
    pub fn new(base: &str, left: &str, right: &str, file_deleted: bool) -> MergeModel {
        let base = merge3::split_lines(base);
        let left = merge3::split_lines(left);
        let right = merge3::split_lines(right);
        let changes = merge3::merge_ranges(&base, &left, &right)
            .into_iter()
            .map(|MergeRange { base: b, left: l, right: r }| {
                let (kind, left_changed, right_changed) = classify(&base[b.clone()], &left[l.clone()], &right[r.clone()]);
                let auto = (kind == ChangeType::Conflict && !l.is_empty() && !r.is_empty() && !file_deleted)
                    .then(|| merge3::resolve_simple(&base[b.clone()], &left[l.clone()], &right[r.clone()]))
                    .flatten();
                MergeChange {
                    result: b.clone(),
                    base: b,
                    left: l,
                    right: r,
                    kind,
                    left_changed,
                    right_changed,
                    resolved: [false; 2],
                    one_side_applied: false,
                    edited: false,
                    auto,
                }
            })
            .collect();
        MergeModel {
            result: base.clone(),
            base,
            left,
            right,
            changes,
            modified: false,
            file_deleted,
        }
    }

    pub fn result_text(&self) -> String {
        self.result.concat()
    }

    fn side_lines(&self, side: Side, r: &Range<usize>) -> Vec<String> {
        match side {
            Side::Left => self.left[r.clone()].to_vec(),
            Side::Right => self.right[r.clone()].to_vec(),
        }
    }

    /// Replaces the result lines of change `i` and shifts the following changes.
    fn replace_result(&mut self, i: usize, lines: Vec<String>) {
        let r = self.changes[i].result.clone();
        let delta = lines.len() as isize - r.len() as isize;
        self.result.splice(r.clone(), lines);
        self.changes[i].result = r.start..(r.end as isize + delta) as usize;
        for c in &mut self.changes[i + 1..] {
            c.result = (c.result.start as isize + delta) as usize..(c.result.end as isize + delta) as usize;
        }
        self.modified = true;
    }

    fn mark_resolved(&mut self, i: usize) {
        self.changes[i].resolved = [true, true];
        self.changes[i].one_side_applied = false;
    }

    /// `>>` / `<<`: accept a side (with `resolve`, Ctrl+click, a conflict is resolved at once).
    pub fn accept(&mut self, i: usize, side: Side, resolve: bool) {
        let c = &self.changes[i];
        if c.is_resolved_side(side) {
            return;
        }
        if !c.is_changed(side) {
            self.mark_resolved(i);
            return;
        }
        let lines = self.side_lines(side, c.side_range(side));
        if !c.is_conflict() {
            self.replace_result(i, lines);
            self.mark_resolved(i);
            return;
        }
        let lines = if c.one_side_applied {
            let mut v = self.result[c.result.clone()].to_vec();
            if v.last().is_some_and(|l| !l.ends_with('\n')) {
                if let Some(l) = v.last_mut() {
                    l.push('\n');
                }
            }
            v.extend(lines);
            v
        } else {
            lines
        };
        let other_empty = self.changes[i].side_range(side.other()).is_empty();
        self.replace_result(i, lines);
        let c = &mut self.changes[i];
        if resolve || other_empty {
            self.mark_resolved(i);
        } else {
            c.one_side_applied = true;
            c.resolved[side.idx()] = true;
        }
    }

    /// `✖`: ignore a side, keeping the result text.
    pub fn ignore(&mut self, i: usize, side: Side, resolve: bool) {
        if !self.changes[i].is_conflict() || resolve {
            self.mark_resolved(i);
        } else {
            self.changes[i].resolved[side.idx()] = true;
        }
        self.modified = true;
    }

    /// Ignore from the result pane: mark the change resolved as it is.
    pub fn ignore_all(&mut self, i: usize) {
        self.mark_resolved(i);
        self.modified = true;
    }

    fn result_unchanged(&self, c: &MergeChange) -> bool {
        !c.edited && self.result[c.result.clone()] == self.base[c.base.clone()]
    }

    /// `canResolveChangeAutomatically`: `side == None` means the base / "All".
    pub fn can_resolve(&self, i: usize, side: Option<Side>) -> bool {
        let c = &self.changes[i];
        if self.file_deleted {
            return false;
        }
        if c.is_conflict() {
            side.is_none() && c.auto.is_some() && !c.resolved[0] && !c.resolved[1] && self.result_unchanged(c)
        } else {
            let changed = match side {
                Some(s) => c.is_changed(s),
                None => c.left_changed || c.right_changed,
            };
            !c.is_resolved() && changed && self.result_unchanged(c)
        }
    }

    /// The magic wand / `resolveChangeAutomatically`.
    pub fn resolve(&mut self, i: usize, side: Option<Side>) {
        if !self.can_resolve(i, side) {
            return;
        }
        let c = &self.changes[i];
        let lines = if c.is_conflict() {
            c.auto.clone().unwrap_or_default()
        } else {
            let s = side.unwrap_or(if c.left_changed { Side::Left } else { Side::Right });
            self.side_lines(s, c.side_range(s))
        };
        self.replace_result(i, lines);
        self.mark_resolved(i);
    }

    pub fn can_apply_non_conflicting(&self, from: ApplyFrom) -> bool {
        (0..self.changes.len()).any(|i| !self.changes[i].is_conflict() && self.can_resolve(i, side_of(from)))
    }

    /// "Apply non-conflicting changes: Left / All / Right".
    pub fn apply_non_conflicting(&mut self, from: ApplyFrom) {
        for i in 0..self.changes.len() {
            if !self.changes[i].is_conflict() {
                self.resolve(i, side_of(from));
            }
        }
    }

    pub fn can_resolve_simple(&self) -> bool {
        (0..self.changes.len()).any(|i| self.can_resolve(i, None))
    }

    /// "Resolve Simple Conflicts": every change resolvable from the base.
    pub fn resolve_simple(&mut self) {
        for i in 0..self.changes.len() {
            self.resolve(i, None);
        }
    }

    /// "Revert Conflict Resolution": back to the base text and unresolved changes.
    pub fn revert(&mut self) {
        self.result = self.base.clone();
        for c in &mut self.changes {
            c.result = c.base.clone();
            c.resolved = [false; 2];
            c.one_side_applied = false;
            c.edited = false;
        }
        self.modified = false;
    }

    /// Manual edit of the result lines of change `i`.
    pub fn edit_change(&mut self, i: usize, lines: Vec<String>) {
        self.replace_result(i, lines);
        let c = &mut self.changes[i];
        c.edited = true;
        if c.kind == ChangeType::Deleted && !c.is_resolved() && c.result.is_empty() {
            self.mark_resolved(i);
        }
    }

    /// Manual edit of result lines `range` between changes (the range must not overlap a change).
    pub fn edit_gap(&mut self, range: Range<usize>, lines: Vec<String>) {
        let delta = lines.len() as isize - range.len() as isize;
        let at = range.end;
        self.result.splice(range, lines);
        for c in &mut self.changes {
            if c.result.start >= at {
                c.result = (c.result.start as isize + delta) as usize..(c.result.end as isize + delta) as usize;
            }
        }
        self.modified = true;
    }

    /// Unresolved (changes, conflicts), as counted by the status line.
    pub fn unresolved(&self) -> (usize, usize) {
        let conflicts = self.changes.iter().filter(|c| c.is_conflict() && !c.is_resolved()).count();
        let changes = self.changes.iter().filter(|c| !c.is_conflict() && !c.is_resolved()).count();
        (changes, conflicts)
    }
}

fn side_of(from: ApplyFrom) -> Option<Side> {
    match from {
        ApplyFrom::Left => Some(Side::Left),
        ApplyFrom::Right => Some(Side::Right),
        ApplyFrom::All => None,
    }
}

/// "N changes, M conflicts" / "All conflicts resolved".
pub fn status_text(changes: usize, conflicts: usize) -> String {
    if changes == 0 && conflicts == 0 {
        return "All conflicts resolved".into();
    }
    let ch = match changes {
        0 => "No changes".to_string(),
        1 => "1 change".to_string(),
        n => format!("{n} changes"),
    };
    let co = match conflicts {
        0 => "No conflicts".to_string(),
        1 => "1 conflict".to_string(),
        n => format!("{n} conflicts"),
    };
    let co = if changes > 0 || conflicts > 0 { co.replacen("No ", "no ", 1) } else { co };
    format!("{ch}, {co}")
}

/// The "Apply Changes" confirmation text for unprocessed changes and conflicts.
pub fn unprocessed_message(changes: usize, conflicts: usize) -> String {
    let mut parts = Vec::new();
    match changes {
        0 => {}
        1 => parts.push("one change".to_string()),
        n => parts.push(format!("{n} changes")),
    }
    match conflicts {
        0 => {}
        1 => parts.push("one conflict".to_string()),
        n => parts.push(format!("{n} conflicts")),
    }
    let verb = if changes == 1 || (changes == 0 && conflicts == 1) { "is" } else { "are" };
    format!(
        "There {verb} {} left unprocessed.\nSave changes and mark the conflict resolved anyway?",
        parts.join(" and ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> MergeModel {
        // line 1 changed only on the left, line 3 conflicts, line 5 changed only on the right.
        MergeModel::new("a\nb\nc\nd\ne\n", "A\nb\nL\nd\ne\n", "a\nb\nR\nd\nE\n", false)
    }

    #[test]
    fn classifies_changes() {
        let m = model();
        let kinds: Vec<_> = m.changes.iter().map(|c| (c.kind, c.left_changed, c.right_changed)).collect();
        assert_eq!(
            kinds,
            vec![
                (ChangeType::Modified, true, false),
                (ChangeType::Conflict, true, true),
                (ChangeType::Modified, false, true),
            ]
        );
        assert_eq!(m.result_text(), "a\nb\nc\nd\ne\n", "result starts as base");
        assert_eq!(m.unresolved(), (2, 1));
    }

    #[test]
    fn applies_non_conflicting_per_side() {
        let mut m = model();
        m.apply_non_conflicting(ApplyFrom::Left);
        assert_eq!(m.result_text(), "A\nb\nc\nd\ne\n");
        assert_eq!(m.unresolved(), (1, 1));
        m.apply_non_conflicting(ApplyFrom::All);
        assert_eq!(m.result_text(), "A\nb\nc\nd\nE\n");
        assert_eq!(m.unresolved(), (0, 1));
    }

    #[test]
    fn accept_left_then_right_appends() {
        let mut m = model();
        m.accept(1, Side::Left, false);
        assert!(m.changes[1].one_side_applied && !m.changes[1].is_resolved());
        m.accept(1, Side::Right, false);
        assert!(m.changes[1].is_resolved());
        assert_eq!(m.result_text(), "a\nb\nL\nR\nd\ne\n");
        // The change after it moved down by one line.
        assert_eq!(m.changes[2].result, 5..6);
    }

    #[test]
    fn ctrl_accept_resolves_and_ignore() {
        let mut m = model();
        m.accept(1, Side::Right, true);
        assert!(m.changes[1].is_resolved());
        assert_eq!(m.result_text(), "a\nb\nR\nd\ne\n");
        m.ignore(0, Side::Left, false);
        assert!(m.changes[0].is_resolved(), "ignoring a non-conflict resolves it");
        m.revert();
        assert_eq!(m.unresolved(), (2, 1));
        assert_eq!(m.result_text(), "a\nb\nc\nd\ne\n");
    }

    #[test]
    fn resolves_simple_conflicts_and_edits() {
        let mut m = MergeModel::new("x = foo(1, 2)\n", "x = bar(1, 2)\n", "x = foo(1, 3)\n", false);
        assert!(m.can_resolve_simple());
        m.resolve_simple();
        assert_eq!(m.result_text(), "x = bar(1, 3)\n");
        let mut m = model();
        m.edit_gap(1..2, vec!["B\n".into(), "B2\n".into()]);
        assert_eq!(m.changes[1].result, 3..4);
        m.edit_change(1, vec!["mine\n".into()]);
        assert!(!m.can_resolve(1, None));
        assert_eq!(m.result_text(), "a\nB\nB2\nmine\nd\ne\n");
    }

    #[test]
    fn messages() {
        assert_eq!(status_text(0, 0), "All conflicts resolved");
        assert_eq!(status_text(2, 1), "2 changes, 1 conflict");
        assert_eq!(status_text(0, 3), "No changes, 3 conflicts");
        assert_eq!(
            unprocessed_message(1, 1),
            "There is one change and one conflict left unprocessed.\nSave changes and mark the conflict resolved anyway?"
        );
        assert!(unprocessed_message(2, 0).starts_with("There are 2 changes left"));
        assert!(unprocessed_message(0, 1).starts_with("There is one conflict left"));
    }
}
