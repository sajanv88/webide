use crate::IdeError;
use crate::fs::ReadLimits;

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * KIB;

/// The smallest "read only above" accepted.
pub const MIN_READ_ONLY_ABOVE: u64 = 64 * KIB;
/// The largest file that may be opened at all, whatever the settings say.
pub const MAX_REFUSE_ABOVE: u64 = 256 * MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The lowest column a ruler may be drawn at.
pub const MIN_RULER_COLUMN: u16 = 10;
/// The highest.
pub const MAX_RULER_COLUMN: u16 = 500;

/// Where spaces and tabs are drawn as marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Whitespace {
    None,
    /// In selected text only.
    #[default]
    Selection,
    All,
}

/// How the cursor's line is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineHighlight {
    None,
    Tint,
    #[default]
    Border,
    Both,
}

/// How the editor draws the text's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSettings {
    /// A thin line down each level of indentation.
    pub indent_guides: bool,
    /// A line down the editor at this column, from 1; `None` for none.
    pub ruler_column: Option<u16>,
    pub whitespace: Whitespace,
    pub line_highlight: LineHighlight,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            indent_guides: true,
            ruler_column: None,
            whitespace: Whitespace::default(),
            line_highlight: LineHighlight::default(),
        }
    }
}

/// The user's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub theme: Theme,
    /// Files larger than this open read-only.
    pub read_only_above_bytes: u64,
    /// Files larger than this don't open.
    pub refuse_above_bytes: u64,
    pub editor: EditorSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            read_only_above_bytes: MIB,
            refuse_above_bytes: 20 * MIB,
            editor: EditorSettings::default(),
        }
    }
}

/// A change to some settings. `None` leaves a setting as it is; the editor's
/// settings change together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsPatch {
    pub theme: Option<Theme>,
    pub read_only_above_bytes: Option<u64>,
    pub refuse_above_bytes: Option<u64>,
    pub editor: Option<EditorSettings>,
}

impl Settings {
    /// These settings with `patch` applied, or why the result would be invalid.
    pub fn apply(&self, patch: &SettingsPatch) -> Result<Self, IdeError> {
        let next = Self {
            theme: patch.theme.unwrap_or(self.theme),
            read_only_above_bytes: patch
                .read_only_above_bytes
                .unwrap_or(self.read_only_above_bytes),
            refuse_above_bytes: patch.refuse_above_bytes.unwrap_or(self.refuse_above_bytes),
            editor: patch.editor.unwrap_or(self.editor),
        };
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), IdeError> {
        if self.read_only_above_bytes < MIN_READ_ONLY_ABOVE {
            return Err(IdeError::InvalidSettings(format!(
                "readOnlyAboveBytes must be at least {MIN_READ_ONLY_ABOVE}"
            )));
        }
        if self.refuse_above_bytes > MAX_REFUSE_ABOVE {
            return Err(IdeError::InvalidSettings(format!(
                "refuseAboveBytes must be at most {MAX_REFUSE_ABOVE}"
            )));
        }
        if self.read_only_above_bytes > self.refuse_above_bytes {
            return Err(IdeError::InvalidSettings(
                "readOnlyAboveBytes must not be larger than refuseAboveBytes".to_owned(),
            ));
        }
        if let Some(column) = self.editor.ruler_column
            && !(MIN_RULER_COLUMN..=MAX_RULER_COLUMN).contains(&column)
        {
            return Err(IdeError::InvalidSettings(format!(
                "rulerColumn must be from {MIN_RULER_COLUMN} to {MAX_RULER_COLUMN}"
            )));
        }
        Ok(())
    }

    pub fn read_limits(&self) -> ReadLimits {
        ReadLimits {
            read_only_above: self.read_only_above_bytes,
            refuse_above: self.refuse_above_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn applies_and_validates_patches() {
        let base = Settings::default();
        let cases: &[(SettingsPatch, bool)] = &[
            (SettingsPatch::default(), true),
            (
                SettingsPatch {
                    theme: Some(Theme::Dark),
                    ..Default::default()
                },
                true,
            ),
            (
                SettingsPatch {
                    read_only_above_bytes: Some(2 * MIB),
                    ..Default::default()
                },
                true,
            ),
            (
                SettingsPatch {
                    read_only_above_bytes: Some(KIB),
                    ..Default::default()
                },
                false,
            ),
            (
                SettingsPatch {
                    refuse_above_bytes: Some(MAX_REFUSE_ABOVE + 1),
                    ..Default::default()
                },
                false,
            ),
            (
                SettingsPatch {
                    read_only_above_bytes: Some(30 * MIB),
                    ..Default::default()
                },
                false,
            ),
            (
                SettingsPatch {
                    read_only_above_bytes: Some(30 * MIB),
                    refuse_above_bytes: Some(40 * MIB),
                    ..Default::default()
                },
                true,
            ),
        ];
        for (patch, ok) in cases {
            assert_eq!(base.apply(patch).is_ok(), *ok, "{patch:?}");
        }
        for (column, ok) in [
            (Some(80), true),
            (None, true),
            (Some(9), false),
            (Some(501), false),
        ] {
            let patch = SettingsPatch {
                editor: Some(EditorSettings {
                    ruler_column: column,
                    ..EditorSettings::default()
                }),
                ..Default::default()
            };
            assert_eq!(base.apply(&patch).is_ok(), ok, "{column:?}");
        }
        let dark = base.apply(&SettingsPatch {
            theme: Some(Theme::Dark),
            ..Default::default()
        });
        assert_eq!(dark.unwrap().theme, Theme::Dark);
    }
}
