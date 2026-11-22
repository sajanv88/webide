//! What the watcher reports: changes made to a workspace's files, by anyone.

use std::sync::Arc;

use crate::path::RelPath;

/// A batch with more changes than this is reported as `rescan` instead, with no
/// list: after a large change (a checkout, an install) it's cheaper to reload
/// what's shown than to go through each path.
pub const MAX_CHANGES_PER_BATCH: usize = 1000;

/// One change to a file or folder of the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    Created(RelPath),
    /// Its contents or its metadata changed.
    Modified(RelPath),
    Removed(RelPath),
    /// Moved within the workspace, when the system reports both names together.
    /// Otherwise a move is a `Removed` and a `Created`.
    Renamed {
        from: RelPath,
        to: RelPath,
    },
}

impl FsChange {
    /// The path as it is now (the new name, for a rename).
    pub fn path(&self) -> &RelPath {
        match self {
            Self::Created(path) | Self::Modified(path) | Self::Removed(path) => path,
            Self::Renamed { to, .. } => to,
        }
    }
}

/// What a watch reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// Files changed. With `rescan`, the list is incomplete (or empty): changes
    /// were missed or too many to list, so reload everything that's shown.
    Changed {
        changes: Vec<FsChange>,
        rescan: bool,
    },
    /// Watching failed or stopped working; changes may go unnoticed.
    Failed { message: String },
}

/// Where a watch delivers its events, on a thread of the watcher's own.
pub type WatchSink = Arc<dyn Fn(WatchEvent) + Send + Sync>;

/// Keeps a watch running. Dropping it stops the watch.
pub struct WatchGuard {
    _inner: Box<dyn Send + Sync>,
}

impl WatchGuard {
    pub fn new(inner: impl Send + Sync + 'static) -> Self {
        Self {
            _inner: Box::new(inner),
        }
    }
}

impl std::fmt::Debug for WatchGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchGuard").finish_non_exhaustive()
    }
}

/// Collapses changes so each path appears once, in the order first seen, with
/// what happened last. Over [`MAX_CHANGES_PER_BATCH`], returns `None`: rescan.
pub fn coalesce(changes: Vec<FsChange>) -> Option<Vec<FsChange>> {
    let mut out: Vec<FsChange> = Vec::new();
    let mut index = std::collections::HashMap::<RelPath, usize>::new();
    for change in changes {
        match index.get(change.path()) {
            Some(&at) => {
                let merged = merge(&out[at], change);
                out[at] = merged;
            }
            None => {
                index.insert(change.path().clone(), out.len());
                out.push(change);
            }
        }
        if out.len() > MAX_CHANGES_PER_BATCH {
            return None;
        }
    }
    Some(out)
}

/// Two changes to the same path, `earlier` then `later`, as one.
fn merge(earlier: &FsChange, later: FsChange) -> FsChange {
    use FsChange::{Created, Modified, Removed, Renamed};
    match (earlier, later) {
        // Created then changed is still new.
        (Created(_), Modified(path)) => Created(path),
        // Removed then created again (an editor's save-by-replace): it changed.
        (Removed(_), Created(path)) => Modified(path),
        (Renamed { from, .. }, Modified(to)) => Renamed {
            from: from.clone(),
            to,
        },
        (_, later) => later,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> RelPath {
        RelPath::parse_with(path, crate::NameRules::Unix).unwrap()
    }

    #[test]
    fn coalesces_by_path_keeping_the_first_order() {
        let changes = vec![
            FsChange::Created(p("a.ts")),
            FsChange::Modified(p("b.ts")),
            FsChange::Modified(p("a.ts")),
            FsChange::Removed(p("c.ts")),
            FsChange::Created(p("c.ts")),
            FsChange::Modified(p("b.ts")),
        ];
        assert_eq!(
            coalesce(changes),
            Some(vec![
                FsChange::Created(p("a.ts")),
                FsChange::Modified(p("b.ts")),
                FsChange::Modified(p("c.ts")),
            ])
        );
    }

    #[test]
    fn a_rename_keeps_its_old_name_through_later_changes() {
        let changes = vec![
            FsChange::Renamed {
                from: p("old.ts"),
                to: p("new.ts"),
            },
            FsChange::Modified(p("new.ts")),
        ];
        assert_eq!(
            coalesce(changes),
            Some(vec![FsChange::Renamed {
                from: p("old.ts"),
                to: p("new.ts"),
            }])
        );
    }

    #[test]
    fn too_many_changes_become_a_rescan() {
        let changes = (0..=MAX_CHANGES_PER_BATCH)
            .map(|n| FsChange::Modified(p(&format!("f{n}"))))
            .collect();
        assert_eq!(coalesce(changes), None);
        let fits = (0..MAX_CHANGES_PER_BATCH)
            .map(|n| FsChange::Modified(p(&format!("f{n}"))))
            .collect();
        assert!(coalesce(fits).is_some());
    }
}
