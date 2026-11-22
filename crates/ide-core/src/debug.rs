//! Debugging: running a program under a debugger, and what it reports.
//! Lines and columns here are 1-based, as debuggers and people count them.

use std::sync::Arc;

use crate::IdeError;
use crate::path::RelPath;
use crate::workspace::WorkspaceRoot;

/// Where a stack frame is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameSource {
    /// A file in the workspace: it can be opened.
    Workspace(RelPath),
    /// A file outside it, or code with no file (Node's own): only named.
    Outside(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// The debugger's id, for its scopes.
    pub id: i64,
    pub name: String,
    pub source: Option<FrameSource>,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub name: String,
    /// For its variables; 0 when it has none.
    pub reference: i64,
    /// A scope with many variables (the globals): best shown closed.
    pub expensive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub kind: Option<String>,
    /// For its children (an object's properties); 0 when it has none.
    pub reference: i64,
}

/// A breakpoint, as the debugger placed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// The line asked for.
    pub line: u32,
    /// Where it was placed, once the code is loaded and it's bound.
    pub verified: bool,
    /// The line it moved to, when that isn't the one asked for.
    pub actual_line: Option<u32>,
}

/// What a debug session reports, from its own threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugEvent {
    /// The program wrote this (stdout, stderr), or the debugger said it.
    Output { category: String, text: String },
    /// Paused: at a breakpoint, after a step, on `debugger;`, on an exception.
    Stopped {
        thread_id: i64,
        reason: String,
        description: Option<String>,
    },
    /// Running again.
    Continued,
    /// The program ended; nothing follows.
    Terminated,
}

pub type DebugSink = Arc<dyn Fn(DebugEvent) + Send + Sync>;

/// How to step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Over,
    Into,
    Out,
}

/// A running program under a debugger. Dropping it ends both.
pub trait DebugSession: Send + Sync {
    /// The breakpoints in one file, replacing the ones it had.
    fn set_breakpoints(&self, path: &RelPath, lines: &[u32]) -> Result<Vec<Breakpoint>, IdeError>;
    fn resume(&self, thread_id: i64) -> Result<(), IdeError>;
    fn step(&self, thread_id: i64, step: Step) -> Result<(), IdeError>;
    fn pause(&self, thread_id: i64) -> Result<(), IdeError>;
    fn stack(&self, thread_id: i64) -> Result<Vec<StackFrame>, IdeError>;
    fn scopes(&self, frame_id: i64) -> Result<Vec<Scope>, IdeError>;
    fn variables(&self, reference: i64) -> Result<Vec<Variable>, IdeError>;
    /// An expression's value, in a frame (or globally).
    fn evaluate(&self, expression: &str, frame_id: Option<i64>) -> Result<String, IdeError>;
}

/// Starts programs under a debugger.
pub trait DebugAdapters: Send + Sync {
    /// Whether it can debug this file (by its extension).
    fn handles(&self, path: &RelPath) -> bool;
    /// Runs `program` in the workspace folder, with `breakpoints` (per file)
    /// set before it starts. Its events go to `sink`.
    fn launch(
        &self,
        root: &WorkspaceRoot,
        program: &RelPath,
        breakpoints: &[(RelPath, Vec<u32>)],
        sink: DebugSink,
    ) -> Result<Box<dyn DebugSession>, IdeError>;
}
