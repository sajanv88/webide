//! Workspace tasks.
//!
//! `cargo xtask check-boundaries` reads `cargo metadata` and fails when a crate
//! breaks the dependency rules in plan.md:
//!
//! - Only `ide-ipc` and the composition root (`ide-desktop`) may reach `tauri`,
//!   directly or transitively.
//! - `ide-core` depends on no workspace crate and no async runtime.
//! - Feature crates depend on `ide-core` only, among workspace crates.
//! - `ide-ipc` depends on `ide-core` only, among workspace crates.
//! - Every `ide-*` crate is classified here, so a new one can't slip past the rules.
//!
//! `cargo xtask check-csp` fails when the production CSP stops being strict (see `csp.rs`).
//!
//! `cargo xtask check-contracts` fails when the committed contracts differ from
//! what `pnpm contracts:generate` writes now (see `contracts.rs`).

mod contracts;
mod csp;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;

const CORE: &str = "ide-core";
const IPC: &str = "ide-ipc";
const APP: &str = "ide-desktop";
const TOOLING: &[&str] = &["xtask"];
/// Built now, or reserved for later milestones (they're features when they appear).
const FEATURES: &[&str] = &[
    "ide-fs",
    "ide-watcher",
    "ide-workspace",
    "ide-lsp",
    "ide-terminal",
    "ide-git",
    "ide-debug",
    "ide-mcp",
    "ide-search",
];
const TAURI_ALLOWED: &[&str] = &[IPC, APP];
const ASYNC_RUNTIMES: &[&str] = &["tokio", "async-std", "smol", "async-executor"];

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check-boundaries") => match load_graph() {
            Ok(graph) => report("crate boundaries", &check(&graph)),
            Err(error) => fail(&error),
        },
        Some("check-csp") => match load_tauri_config() {
            Ok(config) => report("production CSP", &csp::violations(&config)),
            Err(error) => fail(&error),
        },
        Some("check-contracts") => match contracts::differences(&workspace_root()) {
            Ok(differences) => {
                let code = report("generated contracts", &differences);
                if !differences.is_empty() {
                    eprintln!("run `pnpm contracts:generate` and commit the result");
                }
                code
            }
            Err(error) => fail(&error),
        },
        _ => {
            eprintln!("usage: cargo xtask <check-boundaries|check-csp|check-contracts>");
            ExitCode::from(2)
        }
    }
}

fn fail(error: &str) -> ExitCode {
    eprintln!("xtask: {error}");
    ExitCode::FAILURE
}

fn report(label: &str, violations: &[String]) -> ExitCode {
    if violations.is_empty() {
        println!("{label}: ok");
        return ExitCode::SUCCESS;
    }
    for violation in violations {
        eprintln!("{label} violation: {violation}");
    }
    ExitCode::FAILURE
}

fn workspace_root() -> PathBuf {
    let xtask_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask_dir.parent().unwrap_or(xtask_dir).to_path_buf()
}

fn load_tauri_config() -> Result<Value, String> {
    let path = workspace_root()
        .join("apps")
        .join("desktop")
        .join("src-tauri")
        .join("tauri.conf.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))
}

/// Which crates depend on which, by package name.
#[derive(Debug, Default)]
struct Graph {
    members: BTreeSet<String>,
    /// Normal and build dependencies: what ends up in (or builds) the crate.
    runtime: BTreeMap<String, BTreeSet<String>>,
    /// Every dependency, dev included.
    all: BTreeMap<String, BTreeSet<String>>,
}

impl Graph {
    fn direct_members(&self, name: &str) -> BTreeSet<&str> {
        self.all
            .get(name)
            .into_iter()
            .flatten()
            .filter(|dep| self.members.contains(*dep))
            .map(String::as_str)
            .collect()
    }

    /// The shortest runtime path from `from` to `target`, if any.
    fn path_to(&self, from: &str, target: &str) -> Option<Vec<String>> {
        let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
        let mut queue = VecDeque::from([from]);
        let mut seen = BTreeSet::from([from]);
        while let Some(node) = queue.pop_front() {
            if node == target {
                let mut path = vec![node.to_owned()];
                let mut at = node;
                while let Some(prev) = parent.get(at) {
                    path.push((*prev).to_owned());
                    at = prev;
                }
                path.reverse();
                return Some(path);
            }
            for dep in self.runtime.get(node).into_iter().flatten() {
                if seen.insert(dep.as_str()) {
                    parent.insert(dep.as_str(), node);
                    queue.push_back(dep.as_str());
                }
            }
        }
        None
    }
}

fn check(graph: &Graph) -> Vec<String> {
    let mut violations = Vec::new();

    for member in &graph.members {
        let name = member.as_str();
        let known = name == CORE
            || name == IPC
            || name == APP
            || FEATURES.contains(&name)
            || TOOLING.contains(&name);
        if !known {
            violations.push(format!(
                "`{name}` isn't classified; add it to xtask/src/main.rs (feature, core, ipc or tooling)"
            ));
        }

        if !TAURI_ALLOWED.contains(&name)
            && let Some(path) = graph.path_to(name, "tauri")
        {
            violations.push(format!(
                "`{name}` reaches tauri ({}); only {} may",
                path.join(" -> "),
                TAURI_ALLOWED.join(" and ")
            ));
        }
    }

    if graph.members.contains(CORE) {
        for dep in graph.direct_members(CORE) {
            violations.push(format!(
                "`{CORE}` depends on `{dep}`; it must depend on no workspace crate"
            ));
        }
        for runtime in ASYNC_RUNTIMES {
            if let Some(path) = graph.path_to(CORE, runtime) {
                violations.push(format!(
                    "`{CORE}` reaches the async runtime `{runtime}` ({})",
                    path.join(" -> ")
                ));
            }
        }
    }

    for name in FEATURES.iter().copied().chain([IPC]) {
        if !graph.members.contains(name) {
            continue;
        }
        for dep in graph.direct_members(name) {
            if dep != CORE {
                violations.push(format!(
                    "`{name}` depends on `{dep}`; it may depend on `{CORE}` only (wire crates together in `{APP}`)"
                ));
            }
        }
    }

    violations
}

fn load_graph() -> Result<Graph, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .output()
        .map_err(|error| format!("could not run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata returned invalid JSON: {error}"))?;
    graph_from_metadata(&metadata)
}

fn graph_from_metadata(metadata: &Value) -> Result<Graph, String> {
    let field = |value: &Value, key: &str| -> Result<Value, String> {
        value
            .get(key)
            .cloned()
            .ok_or_else(|| format!("cargo metadata has no `{key}`"))
    };

    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for package in field(metadata, "packages")?
        .as_array()
        .into_iter()
        .flatten()
    {
        if let (Some(id), Some(name)) = (package["id"].as_str(), package["name"].as_str()) {
            names.insert(id.to_owned(), name.to_owned());
        }
    }
    let name_of = |id: &str| {
        names
            .get(id)
            .cloned()
            .ok_or_else(|| format!("unknown package id {id}"))
    };

    let mut graph = Graph::default();
    for id in field(metadata, "workspace_members")?
        .as_array()
        .into_iter()
        .flatten()
    {
        graph
            .members
            .insert(name_of(id.as_str().unwrap_or_default())?);
    }

    let resolve = field(metadata, "resolve")?;
    for node in field(&resolve, "nodes")?.as_array().into_iter().flatten() {
        let from = name_of(node["id"].as_str().unwrap_or_default())?;
        for dep in node["deps"].as_array().into_iter().flatten() {
            let to = name_of(dep["pkg"].as_str().unwrap_or_default())?;
            let kinds: Vec<&str> = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|kind| kind["kind"].as_str().unwrap_or("normal"))
                .collect();
            graph
                .all
                .entry(from.clone())
                .or_default()
                .insert(to.clone());
            if kinds.iter().any(|kind| *kind != "dev") {
                graph.runtime.entry(from.clone()).or_default().insert(to);
            }
        }
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a graph from `(from, to, is_dev)` edges; every `ide-*`/`xtask` name is a member.
    fn graph(edges: &[(&str, &str, bool)]) -> Graph {
        let mut graph = Graph::default();
        for (from, to, is_dev) in edges {
            for name in [from, to] {
                if name.starts_with("ide-") || *name == "xtask" {
                    graph.members.insert((*name).to_owned());
                }
            }
            graph
                .all
                .entry((*from).to_owned())
                .or_default()
                .insert((*to).to_owned());
            if !is_dev {
                graph
                    .runtime
                    .entry((*from).to_owned())
                    .or_default()
                    .insert((*to).to_owned());
            }
        }
        graph
    }

    const OK_EDGES: &[(&str, &str, bool)] = &[
        ("ide-core", "thiserror", false),
        ("ide-fs", "ide-core", false),
        ("ide-ipc", "ide-core", false),
        ("ide-ipc", "tauri", false),
        ("tauri", "tokio", false),
        ("ide-desktop", "ide-ipc", false),
        ("ide-desktop", "ide-fs", false),
        ("ide-desktop", "tauri", false),
    ];

    #[test]
    fn the_intended_graph_passes() {
        assert_eq!(check(&graph(OK_EDGES)), Vec::<String>::new());
    }

    #[test]
    fn each_broken_rule_is_reported() {
        let cases: &[(&str, (&str, &str, bool), &str)] = &[
            (
                "feature uses tauri",
                ("ide-fs", "tauri", false),
                "`ide-fs` reaches tauri",
            ),
            (
                "feature uses tauri via a crate",
                ("ide-fs", "tauri-utils-wrapper", false),
                "`ide-fs` reaches tauri",
            ),
            (
                "feature uses feature",
                ("ide-fs", "ide-workspace", false),
                "`ide-fs` depends on `ide-workspace`",
            ),
            (
                "feature dev-uses feature",
                ("ide-fs", "ide-watcher", true),
                "`ide-fs` depends on `ide-watcher`",
            ),
            (
                "ipc uses feature",
                ("ide-ipc", "ide-fs", false),
                "`ide-ipc` depends on `ide-fs`",
            ),
            (
                "core uses a crate",
                ("ide-core", "ide-fs", false),
                "`ide-core` depends on `ide-fs`",
            ),
            (
                "core uses tokio",
                ("ide-core", "tokio", false),
                "`ide-core` reaches the async runtime `tokio`",
            ),
            (
                "unclassified crate",
                ("ide-mystery", "ide-core", false),
                "`ide-mystery` isn't classified",
            ),
        ];
        for (label, edge, expected) in cases {
            let mut edges = OK_EDGES.to_vec();
            edges.push(*edge);
            if edge.1 == "tauri-utils-wrapper" {
                edges.push(("tauri-utils-wrapper", "tauri", false));
            }
            let violations = check(&graph(&edges));
            assert!(
                violations.iter().any(|v| v.contains(expected)),
                "{label}: expected a violation containing {expected:?}, got {violations:?}"
            );
        }
    }

    #[test]
    fn dev_only_tauri_is_not_a_runtime_dependency() {
        let mut edges = OK_EDGES.to_vec();
        edges.push(("xtask", "tauri", true));
        assert_eq!(check(&graph(&edges)), Vec::<String>::new());
    }
}
