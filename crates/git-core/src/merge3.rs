//! Line-based three-way merge (diff3), used by the conflict resolver.
//! Follows the classic merge3 sync-region algorithm.

use similar::{capture_diff_slices, Algorithm, DiffOp};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    /// Lines identical in base, ours and theirs.
    Unchanged(Vec<String>),
    /// Changed only on our side (or identically on both sides).
    Ours(Vec<String>),
    /// Changed only on their side.
    Theirs(Vec<String>),
    /// Both sides changed the same region differently.
    Conflict {
        base: Vec<String>,
        ours: Vec<String>,
        theirs: Vec<String>,
    },
}

/// Splits text into lines keeping line terminators.
pub fn split_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_string).collect()
}

/// (base_start, other_start, len) of matching runs.
fn matching_blocks(base: &[String], other: &[String]) -> Vec<(usize, usize, usize)> {
    capture_diff_slices(Algorithm::Myers, base, other)
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => Some((old_index, new_index, len)),
            _ => None,
        })
        .collect()
}

type SyncRegion = (usize, usize, usize, usize, usize, usize);

fn sync_regions(base: &[String], a: &[String], b: &[String]) -> Vec<SyncRegion> {
    let ma = matching_blocks(base, a);
    let mb = matching_blocks(base, b);
    let (mut ia, mut ib) = (0, 0);
    let mut out = Vec::new();
    while ia < ma.len() && ib < mb.len() {
        let (abase, amatch, alen) = ma[ia];
        let (bbase, bmatch, blen) = mb[ib];
        let start = abase.max(bbase);
        let end = (abase + alen).min(bbase + blen);
        if start < end {
            let asub = amatch + (start - abase);
            let bsub = bmatch + (start - bbase);
            let len = end - start;
            out.push((start, end, asub, asub + len, bsub, bsub + len));
        }
        if abase + alen < bbase + blen {
            ia += 1;
        } else {
            ib += 1;
        }
    }
    out.push((base.len(), base.len(), a.len(), a.len(), b.len(), b.len()));
    out
}

/// Line ranges of one change between base and the two sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRange {
    pub base: std::ops::Range<usize>,
    pub left: std::ops::Range<usize>,
    pub right: std::ops::Range<usize>,
}

/// The gaps between blocks that are unchanged on all three sides, like IntelliJ's
/// `ComparisonMergeUtil.FairMergeBuilder`: changes from both sides that overlap or
/// touch in base (with no base line unchanged on both sides between them) form one range.
pub fn merge_ranges(base: &[String], left: &[String], right: &[String]) -> Vec<MergeRange> {
    let mut out = Vec::new();
    let (mut iz, mut ia, mut ib) = (0, 0, 0);
    for (zmatch, zend, amatch, aend, bmatch, bend) in sync_regions(base, left, right) {
        if iz < zmatch || ia < amatch || ib < bmatch {
            out.push(MergeRange {
                base: iz..zmatch,
                left: ia..amatch,
                right: ib..bmatch,
            });
        }
        iz = zend;
        ia = aend;
        ib = bend;
    }
    out
}

/// Three-way merges `ours` and `theirs` against `base`.
pub fn merge(base: &[String], ours: &[String], theirs: &[String]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let (mut iz, mut ia, mut ib) = (0, 0, 0);
    for (zmatch, zend, amatch, aend, bmatch, bend) in sync_regions(base, ours, theirs) {
        let a = &ours[ia..amatch];
        let b = &theirs[ib..bmatch];
        let z = &base[iz..zmatch];
        if !a.is_empty() || !b.is_empty() {
            if a == b {
                chunks.push(Chunk::Ours(a.to_vec()));
            } else if a == z {
                chunks.push(Chunk::Theirs(b.to_vec()));
            } else if b == z {
                chunks.push(Chunk::Ours(a.to_vec()));
            } else {
                chunks.push(Chunk::Conflict {
                    base: z.to_vec(),
                    ours: a.to_vec(),
                    theirs: b.to_vec(),
                });
            }
        }
        if zend > zmatch {
            chunks.push(Chunk::Unchanged(base[zmatch..zend].to_vec()));
        }
        iz = zend;
        ia = aend;
        ib = bend;
    }
    // Merge adjacent unchanged runs for compact display.
    let mut merged: Vec<Chunk> = Vec::new();
    for c in chunks {
        match (merged.last_mut(), c) {
            (Some(Chunk::Unchanged(prev)), Chunk::Unchanged(next)) => prev.extend(next),
            (_, c) => merged.push(c),
        }
    }
    merged
}

pub fn conflict_count(chunks: &[Chunk]) -> usize {
    chunks
        .iter()
        .filter(|c| matches!(c, Chunk::Conflict { .. }))
        .count()
}

/// Tries to resolve a conflict automatically, like IntelliJ's "Resolve simple
/// conflicts": sides that differ only in whitespace, or changes that do not
/// overlap at word level. Returns the merged lines, or `None` if it is a real conflict.
pub fn resolve_simple(base: &[String], ours: &[String], theirs: &[String]) -> Option<Vec<String>> {
    // IntelliJ's MergeResolveUtil: a word-level merge under the default policy first,
    // then one where whitespace-only differences do not count.
    let words = |v: &[String]| tokenize(&v.concat());
    let chunks = merge(&words(base), &words(ours), &words(theirs));
    if conflict_count(&chunks) > 0 {
        let squash = |v: &[String]| v.concat().split_whitespace().collect::<Vec<_>>().join(" ");
        return (squash(ours) == squash(theirs)).then(|| ours.to_vec());
    }
    let text: String = chunks
        .into_iter()
        .flat_map(|c| match c {
            Chunk::Unchanged(v) | Chunk::Ours(v) | Chunk::Theirs(v) => v,
            Chunk::Conflict { .. } => Vec::new(),
        })
        .collect();
    Some(split_lines(&text))
}

/// Splits text into word, whitespace and punctuation tokens (concatenating them gives the text back).
fn tokenize(text: &str) -> Vec<String> {
    #[derive(PartialEq)]
    enum K {
        Word,
        Space,
        Newline,
        Other,
    }
    let kind = |c: char| {
        if c == '\n' {
            K::Newline
        } else if c.is_whitespace() {
            K::Space
        } else if c.is_alphanumeric() || c == '_' {
            K::Word
        } else {
            K::Other
        }
    };
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<K> = None;
    for c in text.chars() {
        let k = kind(c);
        let join = matches!((&prev, &k), (Some(K::Word), K::Word) | (Some(K::Space), K::Space));
        match out.last_mut() {
            Some(last) if join => last.push(c),
            _ => out.push(c.to_string()),
        }
        prev = Some(k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: &str) -> Vec<String> {
        split_lines(s)
    }

    #[test]
    fn non_overlapping_changes_merge_cleanly() {
        let base = l("a\nb\nc\nd\n");
        let ours = l("A\nb\nc\nd\n");
        let theirs = l("a\nb\nc\nD\n");
        let m = merge(&base, &ours, &theirs);
        assert_eq!(conflict_count(&m), 0);
        let text: String = m
            .iter()
            .flat_map(|c| match c {
                Chunk::Unchanged(v) | Chunk::Ours(v) | Chunk::Theirs(v) => v.clone(),
                Chunk::Conflict { .. } => vec![],
            })
            .collect();
        assert_eq!(text, "A\nb\nc\nD\n");
    }

    #[test]
    fn overlapping_changes_conflict() {
        let base = l("a\nb\nc\n");
        let ours = l("a\nX\nc\n");
        let theirs = l("a\nY\nc\n");
        let m = merge(&base, &ours, &theirs);
        assert_eq!(conflict_count(&m), 1);
        assert!(m.contains(&Chunk::Conflict {
            base: l("b\n"),
            ours: l("X\n"),
            theirs: l("Y\n"),
        }));
    }

    #[test]
    fn same_change_both_sides() {
        let base = l("a\nb\n");
        let m = merge(&base, &l("a\nZ\n"), &l("a\nZ\n"));
        assert_eq!(conflict_count(&m), 0);
    }

    #[test]
    fn resolves_word_level_and_whitespace_conflicts() {
        let base = l("let a = foo(1, 2);\n");
        let ours = l("let a = bar(1, 2);\n");
        let theirs = l("let a = foo(1, 3);\n");
        assert_eq!(resolve_simple(&base, &ours, &theirs), Some(l("let a = bar(1, 3);\n")));
        let ws = resolve_simple(&l("x\n"), &l("y  z\n"), &l("y z\n"));
        assert_eq!(ws, Some(l("y  z\n")));
        assert_eq!(resolve_simple(&l("a\n"), &l("b\n"), &l("c\n")), None);
    }
}
