//! The on-disk formats, kept separate from the domain types so either can change.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ide_core::{
    EditorSettings, IdeError, LineHighlight, RecentWorkspace, Settings, SettingsPatch, Theme,
    Whitespace,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentFile {
    version: u32,
    #[serde(default)]
    workspaces: Vec<RecentEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentEntry {
    path: String,
    name: String,
    opened_at_ms: u64,
}

impl RecentFile {
    pub(crate) fn from_domain(recent: &[RecentWorkspace]) -> Self {
        let workspaces = recent
            .iter()
            // A path that isn't valid UTF-8 can't be stored in JSON; it's left out.
            .filter_map(|entry| {
                Some(RecentEntry {
                    path: entry.path.to_str()?.to_owned(),
                    name: entry.name.clone(),
                    opened_at_ms: entry.opened_at_ms,
                })
            })
            .collect();
        Self {
            version: VERSION,
            workspaces,
        }
    }

    pub(crate) fn into_domain(self) -> Vec<RecentWorkspace> {
        self.workspaces
            .into_iter()
            .map(|entry| RecentWorkspace {
                path: PathBuf::from(entry.path),
                name: entry.name,
                opened_at_ms: entry.opened_at_ms,
            })
            .collect()
    }
}

/// The folders the person trusts, canonical.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrustFile {
    version: u32,
    #[serde(default)]
    folders: Vec<String>,
}

impl TrustFile {
    pub(crate) fn from_domain(folders: &[PathBuf]) -> Self {
        Self {
            version: VERSION,
            // A path that isn't valid UTF-8 can't be stored in JSON: not trusted.
            folders: folders
                .iter()
                .filter_map(|folder| Some(folder.to_str()?.to_owned()))
                .collect(),
        }
    }

    pub(crate) fn into_domain(self) -> Vec<PathBuf> {
        self.folders.into_iter().map(PathBuf::from).collect()
    }
}

/// Every field optional, so a missing one takes its default.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SettingsFile {
    version: Option<u32>,
    theme: Option<ThemeName>,
    files: FilesSection,
    editor: EditorSection,
    languages: LanguagesSection,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LanguagesSection {
    eslint: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct EditorSection {
    indent_guides: Option<bool>,
    /// `null` or absent: no ruler.
    ruler_column: Option<u16>,
    whitespace: Option<WhitespaceName>,
    line_highlight: Option<LineHighlightName>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum WhitespaceName {
    None,
    Selection,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LineHighlightName {
    None,
    Tint,
    Border,
    Both,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FilesSection {
    read_only_above_bytes: Option<u64>,
    refuse_above_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ThemeName {
    System,
    Light,
    Dark,
}

impl SettingsFile {
    pub(crate) fn from_domain(settings: &Settings) -> Self {
        Self {
            version: Some(VERSION),
            theme: Some(match settings.theme {
                Theme::System => ThemeName::System,
                Theme::Light => ThemeName::Light,
                Theme::Dark => ThemeName::Dark,
            }),
            files: FilesSection {
                read_only_above_bytes: Some(settings.read_only_above_bytes),
                refuse_above_bytes: Some(settings.refuse_above_bytes),
            },
            editor: EditorSection {
                indent_guides: Some(settings.editor.indent_guides),
                ruler_column: settings.editor.ruler_column,
                whitespace: Some(match settings.editor.whitespace {
                    Whitespace::None => WhitespaceName::None,
                    Whitespace::Selection => WhitespaceName::Selection,
                    Whitespace::All => WhitespaceName::All,
                }),
                line_highlight: Some(match settings.editor.line_highlight {
                    LineHighlight::None => LineHighlightName::None,
                    LineHighlight::Tint => LineHighlightName::Tint,
                    LineHighlight::Border => LineHighlightName::Border,
                    LineHighlight::Both => LineHighlightName::Both,
                }),
            },
            languages: LanguagesSection {
                eslint: Some(settings.eslint),
            },
        }
    }

    pub(crate) fn into_domain(self) -> Result<Settings, IdeError> {
        let defaults = EditorSettings::default();
        let editor = EditorSettings {
            indent_guides: self.editor.indent_guides.unwrap_or(defaults.indent_guides),
            ruler_column: self.editor.ruler_column,
            whitespace: self
                .editor
                .whitespace
                .map_or(defaults.whitespace, |name| match name {
                    WhitespaceName::None => Whitespace::None,
                    WhitespaceName::Selection => Whitespace::Selection,
                    WhitespaceName::All => Whitespace::All,
                }),
            line_highlight: self
                .editor
                .line_highlight
                .map_or(defaults.line_highlight, |name| match name {
                    LineHighlightName::None => LineHighlight::None,
                    LineHighlightName::Tint => LineHighlight::Tint,
                    LineHighlightName::Border => LineHighlight::Border,
                    LineHighlightName::Both => LineHighlight::Both,
                }),
        };
        let patch = SettingsPatch {
            theme: self.theme.map(|theme| match theme {
                ThemeName::System => Theme::System,
                ThemeName::Light => Theme::Light,
                ThemeName::Dark => Theme::Dark,
            }),
            read_only_above_bytes: self.files.read_only_above_bytes,
            refuse_above_bytes: self.files.refuse_above_bytes,
            editor: Some(editor),
            eslint: self.languages.eslint,
        };
        Settings::default().apply(&patch)
    }
}

#[derive(Debug)]
pub(crate) enum ReadError {
    Io(IdeError),
    Invalid(serde_json::Error),
}

/// `Ok(None)` when the file doesn't exist.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ReadError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ReadError::Io(IdeError::from_io(&error))),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(ReadError::Invalid)
}

/// Writes pretty JSON to `dir/name` atomically, creating `dir` if needed.
pub(crate) fn write_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<(), IdeError> {
    fs::create_dir_all(dir)?;
    let mut json =
        serde_json::to_vec_pretty(value).map_err(|error| IdeError::Io(error.to_string()))?;
    json.push(b'\n');

    let target = dir.join(name);
    let temp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    Ok(result?)
}
