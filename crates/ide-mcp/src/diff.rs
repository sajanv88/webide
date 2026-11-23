//! A unified diff of two texts, by line (Myers' algorithm), for `git_diff`.

/// Lines of context around each change.
const CONTEXT: usize = 3;
/// Beyond this many differences the edit isn't searched for: everything is
/// shown removed, then added.
const MAX_EDITS: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// The same line: its index in each text.
    Same(usize, usize),
    Removed(usize),
    Added(usize),
}

/// The changes from `original` to `modified`, as `diff -u` writes them, with
/// `path` in the header. Empty when they're the same.
pub fn unified_diff(path: &str, original: &str, modified: &str) -> String {
    let old: Vec<&str> = original.split_inclusive('\n').collect();
    let new: Vec<&str> = modified.split_inclusive('\n').collect();
    let ops = edits(&old, &new);
    if ops.iter().all(|op| matches!(op, Op::Same(..))) {
        return String::new();
    }
    // How many lines of each text come before each op.
    let mut before = Vec::with_capacity(ops.len());
    let (mut old_at, mut new_at) = (0, 0);
    for op in &ops {
        before.push((old_at, new_at));
        match op {
            Op::Same(..) => {
                old_at += 1;
                new_at += 1;
            }
            Op::Removed(_) => old_at += 1,
            Op::Added(_) => new_at += 1,
        }
    }
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    for (start, end) in hunks(&ops) {
        let (old_before, new_before) = before[start];
        write_hunk(
            &mut out,
            &ops[start..end],
            (old_before, new_before),
            &old,
            &new,
        );
    }
    out
}

/// Each hunk as a range of `ops`: its changes and their context.
fn hunks(ops: &[Op]) -> Vec<(usize, usize)> {
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (at, op) in ops.iter().enumerate() {
        if matches!(op, Op::Same(..)) {
            continue;
        }
        let start = at.saturating_sub(CONTEXT);
        let end = (at + 1 + CONTEXT).min(ops.len());
        match hunks.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => hunks.push((start, end)),
        }
    }
    hunks
}

/// One hunk; `before` is how many lines of each text come before it.
fn write_hunk(out: &mut String, ops: &[Op], before: (usize, usize), old: &[&str], new: &[&str]) {
    let old_count = ops.iter().filter(|op| !matches!(op, Op::Added(_))).count();
    let new_count = ops
        .iter()
        .filter(|op| !matches!(op, Op::Removed(_)))
        .count();
    // An empty range names the line before it, as `diff -u` does.
    let range = |before: usize, count: usize| {
        if count == 0 {
            format!("{before},0")
        } else {
            format!("{},{count}", before + 1)
        }
    };
    out.push_str(&format!(
        "@@ -{} +{} @@\n",
        range(before.0, old_count),
        range(before.1, new_count)
    ));
    for op in ops {
        let (mark, line) = match *op {
            Op::Same(i, _) => (' ', old[i]),
            Op::Removed(i) => ('-', old[i]),
            Op::Added(j) => ('+', new[j]),
        };
        out.push(mark);
        match line.strip_suffix('\n') {
            Some(line) => {
                out.push_str(line);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
}

/// The shortest edit from `old` to `new`.
fn edits(old: &[&str], new: &[&str]) -> Vec<Op> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];

    let mut ops: Vec<Op> = (0..prefix).map(|i| Op::Same(i, i)).collect();
    match myers(a, b) {
        Some(core) => ops.extend(core.into_iter().map(|op| match op {
            Op::Same(i, j) => Op::Same(i + prefix, j + prefix),
            Op::Removed(i) => Op::Removed(i + prefix),
            Op::Added(j) => Op::Added(j + prefix),
        })),
        None => {
            ops.extend((0..a.len()).map(|i| Op::Removed(i + prefix)));
            ops.extend((0..b.len()).map(|j| Op::Added(j + prefix)));
        }
    }
    ops.extend((0..suffix).map(|k| Op::Same(old.len() - suffix + k, new.len() - suffix + k)));
    ops
}

/// Myers' O(ND) diff, or `None` past `MAX_EDITS` differences.
fn myers(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    if max == 0 {
        return Some(Vec::new());
    }
    let offset = max as isize;
    let mut v = vec![0_isize; 2 * max + 2];
    // `trace[d]`: the furthest x on each diagonal k in -d..=d before step d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let index = |k: isize| (k + offset) as usize;
    for d in 0..=(max.min(MAX_EDITS) as isize) {
        trace.push(v[index(-d)..=index(d)].to_vec());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[index(k - 1)] < v[index(k + 1)]) {
                v[index(k + 1)]
            } else {
                v[index(k - 1)] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index(k)] = x;
            if x >= n && y >= m {
                return Some(backtrack(&trace, n, m));
            }
            k += 2;
        }
    }
    None
}

fn backtrack(trace: &[Vec<isize>], n: isize, m: isize) -> Vec<Op> {
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() as isize).rev() {
        let row = &trace[d as usize];
        let at = |k: isize| row[(k + d) as usize];
        let k = x - y;
        let previous = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let previous_x = at(previous);
        let previous_y = previous_x - previous;
        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Same(x as usize, y as usize));
        }
        if x == previous_x {
            ops.push(Op::Added(previous_y as usize));
        } else {
            ops.push(Op::Removed(previous_x as usize));
        }
        x = previous_x;
        y = previous_y;
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        ops.push(Op::Same(x as usize, y as usize));
    }
    ops.reverse();
    ops
}
