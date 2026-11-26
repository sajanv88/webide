//! Opens workspaces, keeps the recent list, and persists settings, as JSON
//! files in a configuration folder chosen by the composition root.
//!
//! Files written here (both versioned, both replaced atomically):
//! - `recent-workspaces.json`: the last [`MAX_RECENT`] folders opened.
//! - `settings.json`: the user's settings. Unknown keys are ignored and missing
//!   ones take their defaults. A file that can't be read as valid settings is
//!   moved aside to `settings.invalid.json` and defaults are used, so a typo
//!   never stops the app from starting and is never silently overwritten.
//! - `trusted-folders.json`: the folders the person trusts. One that can't be
//!   read trusts nothing (Restricted Mode everywhere), the safe way to fail.

mod files;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use ide_core::workspace::MAX_RECENT;
use ide_core::{IdeError, RecentWorkspace, Settings, SettingsPatch, WorkspaceRoot, WorkspaceStore};

use crate::files::{RecentFile, SettingsFile, TrustFile};

const RECENT_FILE: &str = "recent-workspaces.json";
const SETTINGS_FILE: &str = "settings.json";
const INVALID_SETTINGS_FILE: &str = "settings.invalid.json";
const TRUST_FILE: &str = "trusted-folders.json";

/// Milliseconds since the Unix epoch.
pub type Clock = fn() -> u64;

/// [`WorkspaceStore`] backed by JSON files in `config_dir`.
#[derive(Debug)]
pub struct JsonWorkspaceStore {
    config_dir: PathBuf,
    clock: Clock,
    /// Serialises read-modify-write of the files.
    lock: Mutex<()>,
}

impl JsonWorkspaceStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self::with_clock(config_dir, system_clock)
    }

    pub fn with_clock(config_dir: PathBuf, clock: Clock) -> Self {
        Self {
            config_dir,
            clock,
            lock: Mutex::new(()),
        }
    }

    fn locked(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.config_dir.join(name)
    }

    fn load_recent(&self) -> Vec<RecentWorkspace> {
        match files::read_json::<RecentFile>(&self.file(RECENT_FILE)) {
            Ok(Some(file)) => file.into_domain(),
            // Missing, or unreadable: the recent list isn't worth failing over.
            Ok(None) | Err(_) => Vec::new(),
        }
    }

    fn load_trusted(&self) -> Vec<PathBuf> {
        match files::read_json::<TrustFile>(&self.file(TRUST_FILE)) {
            Ok(Some(file)) => file.into_domain(),
            Ok(None) | Err(_) => Vec::new(),
        }
    }

    fn load_settings(&self) -> Result<Settings, IdeError> {
        let path = self.file(SETTINGS_FILE);
        match files::read_json::<SettingsFile>(&path) {
            Ok(None) => Ok(Settings::default()),
            Ok(Some(file)) => match file.into_domain() {
                Ok(settings) => Ok(settings),
                Err(error) => self.set_invalid_settings_aside(&path, &error.to_string()),
            },
            Err(files::ReadError::Io(error)) => Err(error),
            Err(files::ReadError::Invalid(error)) => {
                self.set_invalid_settings_aside(&path, &error.to_string())
            }
        }
    }

    fn set_invalid_settings_aside(&self, path: &Path, reason: &str) -> Result<Settings, IdeError> {
        let aside = self.file(INVALID_SETTINGS_FILE);
        eprintln!(
            "settings: {} is invalid ({reason}); moved to {} and using defaults",
            path.display(),
            aside.display()
        );
        fs::rename(path, aside)?;
        Ok(Settings::default())
    }
}

impl WorkspaceStore for JsonWorkspaceStore {
    fn open(&self, path: &Path) -> Result<WorkspaceRoot, IdeError> {
        let canonical = fs::canonicalize(path)?;
        if !fs::metadata(&canonical)?.is_dir() {
            return Err(IdeError::NotADirectory);
        }
        let root = WorkspaceRoot::from_canonical(canonical)?;

        let _guard = self.locked();
        let mut recent = self.load_recent();
        recent.retain(|entry| entry.path != root.path());
        recent.insert(
            0,
            RecentWorkspace {
                path: root.path().to_path_buf(),
                name: root.name().to_owned(),
                opened_at_ms: (self.clock)(),
            },
        );
        recent.truncate(MAX_RECENT);
        files::write_json(
            &self.config_dir,
            RECENT_FILE,
            &RecentFile::from_domain(&recent),
        )?;
        Ok(root)
    }

    fn recent(&self) -> Result<Vec<RecentWorkspace>, IdeError> {
        let _guard = self.locked();
        Ok(self.load_recent())
    }

    fn settings(&self) -> Result<Settings, IdeError> {
        let _guard = self.locked();
        self.load_settings()
    }

    fn update_settings(&self, patch: &SettingsPatch) -> Result<Settings, IdeError> {
        let _guard = self.locked();
        let next = self.load_settings()?.apply(patch)?;
        files::write_json(
            &self.config_dir,
            SETTINGS_FILE,
            &SettingsFile::from_domain(&next),
        )?;
        Ok(next)
    }

    fn is_trusted(&self, root: &WorkspaceRoot) -> Result<bool, IdeError> {
        let _guard = self.locked();
        Ok(self
            .load_trusted()
            .iter()
            .any(|folder| root.path().starts_with(folder)))
    }

    fn set_trusted(&self, folder: &Path, trusted: bool) -> Result<(), IdeError> {
        let folder = fs::canonicalize(folder)?;
        let _guard = self.locked();
        let mut folders = self.load_trusted();
        if trusted {
            if folders.iter().any(|kept| folder.starts_with(kept)) {
                return Ok(());
            }
            // The folders in it are covered now.
            folders.retain(|kept| !kept.starts_with(&folder));
            folders.push(folder);
        } else {
            folders.retain(|kept| !kept.starts_with(&folder) && !folder.starts_with(kept));
        }
        files::write_json(
            &self.config_dir,
            TRUST_FILE,
            &TrustFile::from_domain(&folders),
        )
    }
}

fn system_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
