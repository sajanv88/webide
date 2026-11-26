use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ide_core::settings::MIB;
use ide_core::workspace::MAX_RECENT;
use ide_core::{
    EditorSettings, IdeError, Settings, SettingsPatch, Theme, Whitespace, WorkspaceStore,
};

use crate::JsonWorkspaceStore;

static NOW: AtomicU64 = AtomicU64::new(1_000);
fn tick() -> u64 {
    NOW.fetch_add(1, Ordering::SeqCst)
}

struct Fixture {
    _dir: tempfile::TempDir,
    base: PathBuf,
    store: JsonWorkspaceStore,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let store = JsonWorkspaceStore::with_clock(base.join("config"), tick);
        Self {
            _dir: dir,
            base,
            store,
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn config(&self, name: &str) -> PathBuf {
        self.base.join("config").join(name)
    }
}

#[test]
fn opening_canonicalises_and_records_the_folder() {
    let fx = Fixture::new();
    let folder = fx.folder("project");
    let messy = folder.join("..").join("project");

    let root = fx.store.open(&messy).unwrap();
    assert_eq!(root.path(), fs::canonicalize(&folder).unwrap());
    assert_eq!(root.name(), "project");

    let recent = fx.store.recent().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].path, root.path());
    assert_eq!(recent[0].name, "project");
}

#[test]
fn opening_rejects_what_isnt_a_folder() {
    let fx = Fixture::new();
    let file = fx.base.join("file.txt");
    fs::write(&file, "x").unwrap();
    let cases: &[(&Path, &str)] = &[
        (&file, "NotADirectory"),
        (Path::new("definitely/missing"), "NotFound"),
    ];
    for (path, expected) in cases {
        let outcome = fx.store.open(path);
        assert!(
            format!("{outcome:?}").contains(expected),
            "{path:?}: {outcome:?}"
        );
    }
    assert!(fx.store.recent().unwrap().is_empty());
}

#[test]
fn recent_is_most_recent_first_deduplicated_and_capped() {
    let fx = Fixture::new();
    let a = fx.folder("a");
    let b = fx.folder("b");
    fx.store.open(&a).unwrap();
    fx.store.open(&b).unwrap();
    fx.store.open(&a).unwrap();
    let names: Vec<_> = fx
        .store
        .recent()
        .unwrap()
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, ["a", "b"]);

    for index in 0..MAX_RECENT + 3 {
        fx.store.open(&fx.folder(&format!("p{index}"))).unwrap();
    }
    let recent = fx.store.recent().unwrap();
    assert_eq!(recent.len(), MAX_RECENT);
    assert_eq!(recent[0].name, format!("p{}", MAX_RECENT + 2));
    assert!(
        recent
            .windows(2)
            .all(|pair| pair[0].opened_at_ms > pair[1].opened_at_ms)
    );
}

#[test]
fn a_corrupt_recent_file_is_an_empty_list() {
    let fx = Fixture::new();
    fs::create_dir_all(fx.base.join("config")).unwrap();
    fs::write(fx.config("recent-workspaces.json"), "{ not json").unwrap();
    assert!(fx.store.recent().unwrap().is_empty());
    fx.store.open(&fx.folder("a")).unwrap();
    assert_eq!(fx.store.recent().unwrap().len(), 1);
}

#[test]
fn settings_default_then_persist_patches() {
    let fx = Fixture::new();
    assert_eq!(fx.store.settings().unwrap(), Settings::default());

    let patch = SettingsPatch {
        theme: Some(Theme::Dark),
        read_only_above_bytes: Some(2 * MIB),
        ..Default::default()
    };
    let saved = fx.store.update_settings(&patch).unwrap();
    assert_eq!(saved.theme, Theme::Dark);
    assert_eq!(saved.read_only_above_bytes, 2 * MIB);

    let reopened = JsonWorkspaceStore::new(fx.base.join("config"));
    assert_eq!(reopened.settings().unwrap(), saved);
    let json = fs::read_to_string(fx.config("settings.json")).unwrap();
    assert!(json.contains("\"theme\": \"dark\""), "{json}");
    assert!(json.contains("\"readOnlyAboveBytes\": 2097152"), "{json}");
    assert!(json.contains("\"lineHighlight\": \"border\""), "{json}");
}

#[test]
fn invalid_patches_change_nothing() {
    let fx = Fixture::new();
    let bad = SettingsPatch {
        read_only_above_bytes: Some(1),
        ..Default::default()
    };
    assert!(matches!(
        fx.store.update_settings(&bad),
        Err(IdeError::InvalidSettings(_))
    ));
    assert!(!fx.config("settings.json").exists());
}

#[test]
fn settings_files_are_read_leniently_or_set_aside() {
    let cases: &[(&str, &str, Option<Settings>)] = &[
        (
            "unknown keys are ignored",
            r#"{"theme":"light","future":{"x":1}}"#,
            Some(Settings {
                theme: Theme::Light,
                ..Settings::default()
            }),
        ),
        (
            "missing keys take defaults",
            "{}",
            Some(Settings::default()),
        ),
        (
            "partial sections merge",
            r#"{"files":{"refuseAboveBytes":31457280}}"#,
            Some(Settings {
                refuse_above_bytes: 30 * MIB,
                ..Settings::default()
            }),
        ),
        (
            "editor settings",
            r#"{"editor":{"rulerColumn":100,"whitespace":"all"}}"#,
            Some(Settings {
                editor: EditorSettings {
                    ruler_column: Some(100),
                    whitespace: Whitespace::All,
                    ..EditorSettings::default()
                },
                ..Settings::default()
            }),
        ),
        (
            "ruler out of range",
            r#"{"editor":{"rulerColumn":2}}"#,
            None,
        ),
        ("bad JSON", "{ theme: dark", None),
        ("bad value type", r#"{"theme":3}"#, None),
        ("unknown theme", r#"{"theme":"purple"}"#, None),
        (
            "out of range",
            r#"{"files":{"readOnlyAboveBytes":1}}"#,
            None,
        ),
    ];
    for (label, contents, expected) in cases {
        let fx = Fixture::new();
        fs::create_dir_all(fx.base.join("config")).unwrap();
        fs::write(fx.config("settings.json"), contents).unwrap();

        let settings = fx.store.settings().unwrap();
        match expected {
            Some(expected) => {
                assert_eq!(&settings, expected, "{label}");
                assert!(fx.config("settings.json").exists(), "{label}: file kept");
            }
            None => {
                assert_eq!(settings, Settings::default(), "{label}");
                assert!(!fx.config("settings.json").exists(), "{label}: moved aside");
                assert_eq!(
                    fs::read_to_string(fx.config("settings.invalid.json")).unwrap(),
                    *contents,
                    "{label}"
                );
            }
        }
    }
}

#[test]
fn nothing_is_trusted_until_the_person_trusts_it() {
    let f = Fixture::new();
    let root = f.store.open(&f.folder("projects/app")).unwrap();
    assert!(!f.store.is_trusted(&root).unwrap());

    f.store.set_trusted(root.path(), true).unwrap();
    assert!(f.store.is_trusted(&root).unwrap());
    // A store reading the same folder agrees: it's kept.
    let again = JsonWorkspaceStore::with_clock(f.base.join("config"), tick);
    assert!(again.is_trusted(&root).unwrap());
    // A sibling isn't.
    let other = f.store.open(&f.folder("projects/other")).unwrap();
    assert!(!f.store.is_trusted(&other).unwrap());
    // A folder whose name starts the same isn't inside it.
    let lookalike = f.store.open(&f.folder("projects/app-two")).unwrap();
    assert!(!f.store.is_trusted(&lookalike).unwrap());
}

#[test]
fn trusting_a_parent_covers_its_folders_and_distrusting_takes_it_back() {
    let f = Fixture::new();
    let app = f.store.open(&f.folder("projects/app")).unwrap();
    let other = f.store.open(&f.folder("projects/other")).unwrap();
    f.store.set_trusted(app.path(), true).unwrap();
    f.store.set_trusted(&f.base.join("projects"), true).unwrap();
    assert!(f.store.is_trusted(&other).unwrap());
    // One entry is kept, the parent: the folder in it is covered.
    let kept = fs::read_to_string(f.base.join("config").join("trusted-folders.json")).unwrap();
    assert_eq!(kept.matches("projects").count(), 1, "{kept}");

    // Not trusting the folder takes away the parent's trust, which covered it.
    f.store.set_trusted(app.path(), false).unwrap();
    assert!(!f.store.is_trusted(&app).unwrap());
    assert!(!f.store.is_trusted(&other).unwrap());
}

#[test]
fn an_unreadable_trust_file_trusts_nothing() {
    let f = Fixture::new();
    let root = f.store.open(&f.folder("app")).unwrap();
    f.store.set_trusted(root.path(), true).unwrap();
    fs::write(
        f.base.join("config").join("trusted-folders.json"),
        "{ not json",
    )
    .unwrap();
    assert!(!f.store.is_trusted(&root).unwrap());
}
