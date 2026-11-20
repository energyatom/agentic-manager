//! A small unified line diff (longest common subsequence) for History
//! entries. Project code by the plan: no diff crate.
//!
//! The output is the usual unified format: `--- a/<file>` and `+++ b/<file>`
//! headers, then `@@ -l,n +l,n @@` hunks with three lines of context (`,n`
//! is left out when it is 1, as GNU diff does); a last line without a
//! newline is followed by `\ No newline at end of file`. Text is read as
//! UTF-8 with invalid bytes replaced (config files are UTF-8; the loader
//! refuses others).

const CONTEXT: usize = 3;
/// Above this many cells the LCS table is not built and the files are shown
/// as wholly replaced (config files are far smaller).
const MAX_CELLS: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    Del,
    Ins,
}

/// Unified diff of `before` to `after` for `file` (empty when equal).
pub(crate) fn unified(file: &str, before: &[u8], after: &[u8]) -> String {
    let before = String::from_utf8_lossy(before);
    let after = String::from_utf8_lossy(after);
    let a: Vec<&str> = before.split_inclusive('\n').collect();
    let b: Vec<&str> = after.split_inclusive('\n').collect();
    let ops = script(&a, &b);
    if ops.iter().all(|o| *o == Op::Same) {
        return String::new();
    }
    // Position in a and b before each op.
    let mut pos = Vec::with_capacity(ops.len() + 1);
    let (mut i, mut j) = (0usize, 0usize);
    for op in &ops {
        pos.push((i, j));
        match op {
            Op::Same => {
                i += 1;
                j += 1;
            }
            Op::Del => i += 1,
            Op::Ins => j += 1,
        }
    }
    pos.push((i, j));

    let mut out = format!("--- a/{file}\n+++ b/{file}\n");
    let changes: Vec<usize> = (0..ops.len()).filter(|&k| ops[k] != Op::Same).collect();
    let mut g = 0;
    while g < changes.len() {
        // Group changes whose gap of unchanged lines is at most 2 × context.
        let first = changes[g];
        let mut last = first;
        g += 1;
        while g < changes.len() && changes[g] - last <= 2 * CONTEXT + 1 {
            last = changes[g];
            g += 1;
        }
        let start = first.saturating_sub(CONTEXT);
        let end = (last + CONTEXT + 1).min(ops.len());
        let old_n = ops[start..end].iter().filter(|o| **o != Op::Ins).count();
        let new_n = ops[start..end].iter().filter(|o| **o != Op::Del).count();
        let (a0, b0) = pos[start];
        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(a0, old_n),
            range(b0, new_n)
        ));
        for (k, op) in ops.iter().enumerate().take(end).skip(start) {
            let (ai, bj) = pos[k];
            let (mark, line) = match op {
                Op::Same => (' ', a[ai]),
                Op::Del => ('-', a[ai]),
                Op::Ins => ('+', b[bj]),
            };
            out.push(mark);
            out.push_str(line);
            if !line.ends_with('\n') {
                out.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    out
}

/// `l,n` (1-based start), `l` when n is 1, and the line before when n is 0.
fn range(start0: usize, n: usize) -> String {
    match n {
        0 => format!("{start0},0"),
        1 => format!("{}", start0 + 1),
        _ => format!("{},{n}", start0 + 1),
    }
}

/// The edit script from `a` to `b`: common prefix and suffix first, then an
/// LCS over the middle.
fn script(a: &[&str], b: &[&str]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let am = &a[prefix..a.len() - suffix];
    let bm = &b[prefix..b.len() - suffix];
    let mut ops = vec![Op::Same; prefix];
    let (n, m) = (am.len(), bm.len());
    if (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        ops.extend(std::iter::repeat_n(Op::Del, n));
        ops.extend(std::iter::repeat_n(Op::Ins, m));
    } else {
        // lcs[i * (m + 1) + j]: LCS length of am[i..] and bm[j..].
        let w = m + 1;
        let mut lcs = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * w + j] = if am[i] == bm[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if am[i] == bm[j] {
                ops.push(Op::Same);
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
                ops.push(Op::Del);
                i += 1;
            } else {
                ops.push(Op::Ins);
                j += 1;
            }
        }
        ops.extend(std::iter::repeat_n(Op::Del, n - i));
        ops.extend(std::iter::repeat_n(Op::Ins, m - j));
    }
    ops.extend(std::iter::repeat_n(Op::Same, suffix));
    ops
}

#[cfg(test)]
mod tests {
    use super::unified;

    #[test]
    fn equal_texts_give_no_diff() {
        assert_eq!(unified("f", b"a\nb\n", b"a\nb\n"), "");
    }

    #[test]
    fn one_changed_line_with_context() {
        let a = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let b = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
        assert_eq!(
            unified("x.yaml", a.as_bytes(), b.as_bytes()),
            "--- a/x.yaml\n+++ b/x.yaml\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"
        );
    }

    #[test]
    fn far_apart_changes_make_two_hunks() {
        let a: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let b: String = (1..=20)
            .map(|i| match i {
                2 => "two\n".to_string(),
                19 => "nineteen\n".to_string(),
                _ => format!("{i}\n"),
            })
            .collect();
        let d = unified("f", a.as_bytes(), b.as_bytes());
        assert_eq!(d.matches("@@ -").count(), 2, "{d}");
        assert!(d.contains("@@ -1,5 +1,5 @@\n 1\n-2\n+two\n"), "{d}");
        assert!(d.contains("@@ -16,5 +16,5 @@\n"), "{d}");
    }

    #[test]
    fn insertions_deletions_and_a_missing_final_newline() {
        assert_eq!(
            unified("f", b"", b"a\n"),
            "--- a/f\n+++ b/f\n@@ -0,0 +1 @@\n+a\n"
        );
        assert_eq!(
            unified("f", b"a\nb", b"a\nb\n"),
            "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+b\n"
        );
    }
}
