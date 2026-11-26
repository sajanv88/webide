//! The Tauri commands: thin async wrappers that run a handler off the main
//! thread. The list below is the single source for the invoke handler, the
//! generated TypeScript `CommandMap`, and the generated permissions.
//!
//! Every command that takes input takes one parameter named `args`, so the
//! frontend always calls `invoke(name, { args })`.

use std::sync::Arc;

use tauri::{AppHandle, State, Webview, WebviewWindow, Wry};
use tauri_plugin_dialog::DialogExt;

use crate::dto::{
    AgentConfigArgs, AgentStatusDto, AppInfoDto, AssistantConversationArgs, AssistantDecideArgs,
    AssistantSendArgs, AssistantSetupArgs, AssistantStatusDto, BreakpointDto, CloseGuardArgs,
    CodeActionDto, CommandLineStatusDto, CompletionsDto, DebugBreakpointsArgs, DebugEvaluateArgs,
    DebugReferenceArgs, DebugStartArgs, DebugThreadArgs, DefinitionDto, DirListingDto, FileListDto,
    FileStampDto, GitBranchDto, GitCommitArgs, GitDiffArgs, GitDiffDto, GitPathsArgs, GitStateDto,
    GitSwitchArgs, HoverDto, LspDocumentArgs, LspPositionArgs, LspRangeArgs, LspResolveArgs,
    LspSourceActionArgs, ModelInfoDto, OpenRecentArgs, ProblemDto, ReadFileDto, RecentWorkspaceDto,
    ResolvedCompletionDto, ScopeDto, SearchTextArgs, SettingsDto, SettingsPatchDto, StackFrameDto,
    SubscribeEventsArgs, TerminalDto, TerminalEventDto, TerminalIdArgs, TerminalOpenArgs,
    TerminalResizeArgs, TerminalWriteArgs, TextEditDto, TextSearchDto, VariableDto, WindowStateDto,
    WorkspaceArgs, WorkspaceDto, WorkspacePathArgs, WriteFileArgs,
};
use crate::error::IpcError;
use crate::events::EventHub;
use crate::handlers::{self, IpcState};

/// A command's argument type: `()` when it takes none.
#[cfg(test)]
macro_rules! args_type {
    () => {
        ()
    };
    ($args:ty) => {
        $args
    };
}

/// Declares the commands once. For each `handler` command it also checks, at
/// compile time, that `handlers::<name>` has exactly the declared types.
macro_rules! ipc_commands {
    (
        handler { $( $name:ident ( $($args:ty)? ) -> $output:ty; )+ }
        custom { $( $custom:ident ( $($custom_args:ty)? ) -> $custom_output:ty; )+ }
    ) => {
        /// Every command, in registration order.
        pub const COMMAND_NAMES: &[&str] = &[$(stringify!($name),)+ $(stringify!($custom),)+];

        pub(crate) fn invoke_handler() -> impl Fn(tauri::ipc::Invoke<Wry>) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name,)+ $($custom,)+]
        }

        $(
            const _: fn(&IpcState $(, $args)?) -> Result<$output, IpcError> = handlers::$name;
        )+

        #[cfg(test)]
        pub(crate) fn signatures() -> Vec<crate::export::Signature> {
            vec![
                $(crate::export::Signature::of::<args_type!($($args)?), $output>(stringify!($name)),)+
                $(crate::export::Signature::of::<args_type!($($custom_args)?), $custom_output>(stringify!($custom)),)+
            ]
        }
    };
}

ipc_commands! {
    handler {
        app_info() -> AppInfoDto;
        open_recent_workspace(OpenRecentArgs) -> WorkspaceDto;
        recent_workspaces() -> Vec<RecentWorkspaceDto>;
        current_workspace() -> Option<WorkspaceDto>;
        list_directory(WorkspacePathArgs) -> DirListingDto;
        read_file(WorkspacePathArgs) -> ReadFileDto;
        write_file(WriteFileArgs) -> FileStampDto;
        get_settings() -> SettingsDto;
        update_settings(SettingsPatchDto) -> SettingsDto;
        search_text(SearchTextArgs) -> TextSearchDto;
        list_files(WorkspaceArgs) -> FileListDto;
        git_state(WorkspaceArgs) -> GitStateDto;
        git_diff(GitDiffArgs) -> GitDiffDto;
        git_stage(GitPathsArgs) -> ();
        git_unstage(GitPathsArgs) -> ();
        git_discard(GitPathsArgs) -> ();
        git_commit(GitCommitArgs) -> ();
        git_branches(WorkspaceArgs) -> Vec<GitBranchDto>;
        git_switch_branch(GitSwitchArgs) -> ();
        git_init(WorkspaceArgs) -> ();
        terminal_write(TerminalWriteArgs) -> ();
        terminal_resize(TerminalResizeArgs) -> ();
        terminal_close(TerminalIdArgs) -> ();
        lsp_open(LspDocumentArgs) -> ();
        lsp_change(LspDocumentArgs) -> ();
        lsp_close(WorkspacePathArgs) -> ();
        lsp_problems(WorkspacePathArgs) -> Vec<ProblemDto>;
        lsp_completions(LspPositionArgs) -> CompletionsDto;
        lsp_hover(LspPositionArgs) -> Option<HoverDto>;
        lsp_definition(LspPositionArgs) -> Vec<DefinitionDto>;
        lsp_resolve_completion(LspResolveArgs) -> ResolvedCompletionDto;
        lsp_fixes(LspRangeArgs) -> Vec<CodeActionDto>;
        lsp_source_action(LspSourceActionArgs) -> Option<Vec<TextEditDto>>;
        lsp_restart(WorkspaceArgs) -> ();
        debug_start(DebugStartArgs) -> ();
        debug_stop(WorkspaceArgs) -> ();
        debug_breakpoints(DebugBreakpointsArgs) -> Vec<BreakpointDto>;
        debug_resume(DebugThreadArgs) -> ();
        debug_pause(DebugThreadArgs) -> ();
        debug_stack(DebugThreadArgs) -> Vec<StackFrameDto>;
        debug_scopes(DebugReferenceArgs) -> Vec<ScopeDto>;
        debug_variables(DebugReferenceArgs) -> Vec<VariableDto>;
        debug_evaluate(DebugEvaluateArgs) -> String;
        agent_status() -> AgentStatusDto;
        agent_configure(AgentConfigArgs) -> AgentStatusDto;
        agent_new_token() -> AgentStatusDto;
        assistant_status() -> AssistantStatusDto;
        assistant_save(AssistantSetupArgs) -> AssistantStatusDto;
        assistant_forget() -> AssistantStatusDto;
        assistant_models(AssistantSetupArgs) -> Vec<ModelInfoDto>;
        assistant_check(AssistantSetupArgs) -> String;
        assistant_send(AssistantSendArgs) -> ();
        assistant_stop(AssistantConversationArgs) -> ();
        assistant_decide(AssistantDecideArgs) -> ();
        assistant_clear(AssistantConversationArgs) -> ();
        take_launch_file() -> Option<String>;
        command_line_status() -> CommandLineStatusDto;
        command_line_install() -> CommandLineStatusDto;
        command_line_uninstall() -> CommandLineStatusDto;
    }
    custom {
        pick_workspace() -> Option<WorkspaceDto>;
        subscribe_events(SubscribeEventsArgs) -> ();
        set_close_guard(CloseGuardArgs) -> ();
        close_window() -> ();
        window_minimize() -> ();
        window_toggle_maximize() -> ();
        window_drag() -> ();
        window_close() -> ();
        window_state() -> WindowStateDto;
        terminal_open(TerminalOpenArgs) -> TerminalDto;
    }
}

type Shared<'a> = State<'a, Arc<IpcState>>;
type Hub<'a> = State<'a, Arc<EventHub>>;

/// Runs `job` on the blocking thread pool, so file IO never stalls the UI thread.
async fn run_blocking<T, F>(state: &Shared<'_>, job: F) -> Result<T, IpcError>
where
    T: Send + 'static,
    F: FnOnce(&IpcState) -> Result<T, IpcError> + Send + 'static,
{
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || job(&state))
        .await
        .map_err(|error| IpcError::internal(format!("command task failed: {error}")))?
}

#[tauri::command]
pub async fn app_info(state: Shared<'_>) -> Result<AppInfoDto, IpcError> {
    run_blocking(&state, handlers::app_info).await
}

/// Shows the native folder picker; `None` when the person cancels.
#[tauri::command]
pub async fn pick_workspace(
    app: AppHandle,
    window: WebviewWindow,
    state: Shared<'_>,
) -> Result<Option<WorkspaceDto>, IpcError> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Open folder")
            .set_parent(&window)
            .blocking_pick_folder()
    })
    .await
    .map_err(|error| IpcError::internal(format!("folder picker failed: {error}")))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|error| IpcError::internal(format!("the picked folder has no path: {error}")))?;
    run_blocking(&state, move |state| {
        handlers::open_picked(state, &path).map(Some)
    })
    .await
}

#[tauri::command]
pub async fn open_recent_workspace(
    state: Shared<'_>,
    args: OpenRecentArgs,
) -> Result<WorkspaceDto, IpcError> {
    run_blocking(&state, move |state| {
        handlers::open_recent_workspace(state, args)
    })
    .await
}

#[tauri::command]
pub async fn recent_workspaces(state: Shared<'_>) -> Result<Vec<RecentWorkspaceDto>, IpcError> {
    run_blocking(&state, handlers::recent_workspaces).await
}

#[tauri::command]
pub async fn current_workspace(state: Shared<'_>) -> Result<Option<WorkspaceDto>, IpcError> {
    run_blocking(&state, handlers::current_workspace).await
}

#[tauri::command]
pub async fn list_directory(
    state: Shared<'_>,
    args: WorkspacePathArgs,
) -> Result<DirListingDto, IpcError> {
    run_blocking(&state, move |state| handlers::list_directory(state, args)).await
}

#[tauri::command]
pub async fn read_file(
    state: Shared<'_>,
    args: WorkspacePathArgs,
) -> Result<ReadFileDto, IpcError> {
    run_blocking(&state, move |state| handlers::read_file(state, args)).await
}

#[tauri::command]
pub async fn write_file(state: Shared<'_>, args: WriteFileArgs) -> Result<FileStampDto, IpcError> {
    run_blocking(&state, move |state| handlers::write_file(state, args)).await
}

#[tauri::command]
pub async fn get_settings(state: Shared<'_>) -> Result<SettingsDto, IpcError> {
    run_blocking(&state, handlers::get_settings).await
}

#[tauri::command]
pub async fn update_settings(
    state: Shared<'_>,
    args: SettingsPatchDto,
) -> Result<SettingsDto, IpcError> {
    run_blocking(&state, move |state| handlers::update_settings(state, args)).await
}

#[tauri::command]
pub async fn search_text(
    state: Shared<'_>,
    args: SearchTextArgs,
) -> Result<TextSearchDto, IpcError> {
    run_blocking(&state, move |state| handlers::search_text(state, args)).await
}

#[tauri::command]
pub async fn list_files(state: Shared<'_>, args: WorkspaceArgs) -> Result<FileListDto, IpcError> {
    run_blocking(&state, move |state| handlers::list_files(state, args)).await
}

#[tauri::command]
pub async fn git_state(state: Shared<'_>, args: WorkspaceArgs) -> Result<GitStateDto, IpcError> {
    run_blocking(&state, move |state| handlers::git_state(state, args)).await
}

#[tauri::command]
pub async fn git_diff(state: Shared<'_>, args: GitDiffArgs) -> Result<GitDiffDto, IpcError> {
    run_blocking(&state, move |state| handlers::git_diff(state, args)).await
}

#[tauri::command]
pub async fn git_stage(state: Shared<'_>, args: GitPathsArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::git_stage(state, args)).await
}

#[tauri::command]
pub async fn git_unstage(state: Shared<'_>, args: GitPathsArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::git_unstage(state, args)).await
}

#[tauri::command]
pub async fn git_discard(state: Shared<'_>, args: GitPathsArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::git_discard(state, args)).await
}

#[tauri::command]
pub async fn git_commit(state: Shared<'_>, args: GitCommitArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::git_commit(state, args)).await
}

#[tauri::command]
pub async fn git_branches(
    state: Shared<'_>,
    args: WorkspaceArgs,
) -> Result<Vec<GitBranchDto>, IpcError> {
    run_blocking(&state, move |state| handlers::git_branches(state, args)).await
}

#[tauri::command]
pub async fn git_switch_branch(state: Shared<'_>, args: GitSwitchArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| {
        handlers::git_switch_branch(state, args)
    })
    .await
}

#[tauri::command]
pub async fn git_init(state: Shared<'_>, args: WorkspaceArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::git_init(state, args)).await
}

#[tauri::command]
pub async fn terminal_write(state: Shared<'_>, args: TerminalWriteArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::terminal_write(state, args)).await
}

#[tauri::command]
pub async fn terminal_resize(state: Shared<'_>, args: TerminalResizeArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::terminal_resize(state, args)).await
}

#[tauri::command]
pub async fn terminal_close(state: Shared<'_>, args: TerminalIdArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::terminal_close(state, args)).await
}

/// Starts a terminal; its output and its exit arrive on the channel passed.
#[tauri::command]
pub async fn terminal_open(
    webview: Webview,
    state: Shared<'_>,
    args: TerminalOpenArgs,
) -> Result<TerminalDto, IpcError> {
    let channel = args.channel.channel_on(webview);
    let sink: ide_core::TerminalSink = Arc::new(move |event| {
        // A page that's gone has nobody to tell.
        let _ = channel.send(TerminalEventDto::from(event));
    });
    let (workspace_id, cols, rows) = (args.workspace_id, args.cols, args.rows);
    run_blocking(&state, move |state| {
        handlers::terminal_open(state, &workspace_id, cols, rows, sink)
    })
    .await
}

#[tauri::command]
pub async fn lsp_open(state: Shared<'_>, args: LspDocumentArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::lsp_open(state, args)).await
}

#[tauri::command]
pub async fn lsp_change(state: Shared<'_>, args: LspDocumentArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::lsp_change(state, args)).await
}

#[tauri::command]
pub async fn lsp_close(state: Shared<'_>, args: WorkspacePathArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::lsp_close(state, args)).await
}

#[tauri::command]
pub async fn lsp_problems(
    state: Shared<'_>,
    args: WorkspacePathArgs,
) -> Result<Vec<ProblemDto>, IpcError> {
    run_blocking(&state, move |state| handlers::lsp_problems(state, args)).await
}

#[tauri::command]
pub async fn lsp_completions(
    state: Shared<'_>,
    args: LspPositionArgs,
) -> Result<CompletionsDto, IpcError> {
    run_blocking(&state, move |state| handlers::lsp_completions(state, args)).await
}

#[tauri::command]
pub async fn lsp_hover(
    state: Shared<'_>,
    args: LspPositionArgs,
) -> Result<Option<HoverDto>, IpcError> {
    run_blocking(&state, move |state| handlers::lsp_hover(state, args)).await
}

#[tauri::command]
pub async fn lsp_definition(
    state: Shared<'_>,
    args: LspPositionArgs,
) -> Result<Vec<DefinitionDto>, IpcError> {
    run_blocking(&state, move |state| handlers::lsp_definition(state, args)).await
}

#[tauri::command]
pub async fn lsp_resolve_completion(
    state: Shared<'_>,
    args: LspResolveArgs,
) -> Result<ResolvedCompletionDto, IpcError> {
    run_blocking(&state, move |state| {
        handlers::lsp_resolve_completion(state, args)
    })
    .await
}

#[tauri::command]
pub async fn lsp_fixes(
    state: Shared<'_>,
    args: LspRangeArgs,
) -> Result<Vec<CodeActionDto>, IpcError> {
    run_blocking(&state, move |state| handlers::lsp_fixes(state, args)).await
}

#[tauri::command]
pub async fn lsp_source_action(
    state: Shared<'_>,
    args: LspSourceActionArgs,
) -> Result<Option<Vec<TextEditDto>>, IpcError> {
    run_blocking(&state, move |state| {
        handlers::lsp_source_action(state, args)
    })
    .await
}

#[tauri::command]
pub async fn lsp_restart(state: Shared<'_>, args: WorkspaceArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::lsp_restart(state, args)).await
}

#[tauri::command]
pub async fn debug_start(state: Shared<'_>, args: DebugStartArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::debug_start(state, args)).await
}

#[tauri::command]
pub async fn debug_stop(state: Shared<'_>, args: WorkspaceArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::debug_stop(state, args)).await
}

#[tauri::command]
pub async fn debug_breakpoints(
    state: Shared<'_>,
    args: DebugBreakpointsArgs,
) -> Result<Vec<BreakpointDto>, IpcError> {
    run_blocking(&state, move |state| {
        handlers::debug_breakpoints(state, args)
    })
    .await
}

#[tauri::command]
pub async fn debug_resume(state: Shared<'_>, args: DebugThreadArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::debug_resume(state, args)).await
}

#[tauri::command]
pub async fn debug_pause(state: Shared<'_>, args: DebugThreadArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::debug_pause(state, args)).await
}

#[tauri::command]
pub async fn debug_stack(
    state: Shared<'_>,
    args: DebugThreadArgs,
) -> Result<Vec<StackFrameDto>, IpcError> {
    run_blocking(&state, move |state| handlers::debug_stack(state, args)).await
}

#[tauri::command]
pub async fn debug_scopes(
    state: Shared<'_>,
    args: DebugReferenceArgs,
) -> Result<Vec<ScopeDto>, IpcError> {
    run_blocking(&state, move |state| handlers::debug_scopes(state, args)).await
}

#[tauri::command]
pub async fn debug_variables(
    state: Shared<'_>,
    args: DebugReferenceArgs,
) -> Result<Vec<VariableDto>, IpcError> {
    run_blocking(&state, move |state| handlers::debug_variables(state, args)).await
}

#[tauri::command]
pub async fn debug_evaluate(
    state: Shared<'_>,
    args: DebugEvaluateArgs,
) -> Result<String, IpcError> {
    run_blocking(&state, move |state| handlers::debug_evaluate(state, args)).await
}

#[tauri::command]
pub async fn agent_status(state: Shared<'_>) -> Result<AgentStatusDto, IpcError> {
    run_blocking(&state, handlers::agent_status).await
}

#[tauri::command]
pub async fn agent_configure(
    state: Shared<'_>,
    args: AgentConfigArgs,
) -> Result<AgentStatusDto, IpcError> {
    run_blocking(&state, move |state| handlers::agent_configure(state, args)).await
}

#[tauri::command]
pub async fn agent_new_token(state: Shared<'_>) -> Result<AgentStatusDto, IpcError> {
    run_blocking(&state, handlers::agent_new_token).await
}

#[tauri::command]
pub async fn assistant_status(state: Shared<'_>) -> Result<AssistantStatusDto, IpcError> {
    run_blocking(&state, handlers::assistant_status).await
}

#[tauri::command]
pub async fn assistant_save(
    state: Shared<'_>,
    args: AssistantSetupArgs,
) -> Result<AssistantStatusDto, IpcError> {
    run_blocking(&state, move |state| handlers::assistant_save(state, args)).await
}

#[tauri::command]
pub async fn assistant_forget(state: Shared<'_>) -> Result<AssistantStatusDto, IpcError> {
    run_blocking(&state, handlers::assistant_forget).await
}

#[tauri::command]
pub async fn assistant_models(
    state: Shared<'_>,
    args: AssistantSetupArgs,
) -> Result<Vec<ModelInfoDto>, IpcError> {
    run_blocking(&state, move |state| handlers::assistant_models(state, args)).await
}

#[tauri::command]
pub async fn assistant_check(
    state: Shared<'_>,
    args: AssistantSetupArgs,
) -> Result<String, IpcError> {
    run_blocking(&state, move |state| handlers::assistant_check(state, args)).await
}

#[tauri::command]
pub async fn assistant_send(state: Shared<'_>, args: AssistantSendArgs) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::assistant_send(state, args)).await
}

#[tauri::command]
pub async fn assistant_stop(
    state: Shared<'_>,
    args: AssistantConversationArgs,
) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::assistant_stop(state, args)).await
}

#[tauri::command]
pub async fn assistant_decide(
    state: Shared<'_>,
    args: AssistantDecideArgs,
) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::assistant_decide(state, args)).await
}

#[tauri::command]
pub async fn assistant_clear(
    state: Shared<'_>,
    args: AssistantConversationArgs,
) -> Result<(), IpcError> {
    run_blocking(&state, move |state| handlers::assistant_clear(state, args)).await
}

#[tauri::command]
pub async fn take_launch_file(state: Shared<'_>) -> Result<Option<String>, IpcError> {
    run_blocking(&state, handlers::take_launch_file).await
}

#[tauri::command]
pub async fn command_line_status(state: Shared<'_>) -> Result<CommandLineStatusDto, IpcError> {
    run_blocking(&state, handlers::command_line_status).await
}

#[tauri::command]
pub async fn command_line_install(state: Shared<'_>) -> Result<CommandLineStatusDto, IpcError> {
    run_blocking(&state, handlers::command_line_install).await
}

#[tauri::command]
pub async fn command_line_uninstall(state: Shared<'_>) -> Result<CommandLineStatusDto, IpcError> {
    run_blocking(&state, handlers::command_line_uninstall).await
}

/// Sends backend events (file changes, the close request) to the calling page,
/// over the channel it passes. A later call, from a reloaded page, replaces it.
#[tauri::command]
pub fn subscribe_events(
    webview: Webview,
    hub: Hub<'_>,
    args: SubscribeEventsArgs,
) -> Result<(), IpcError> {
    hub.subscribe(args.channel.channel_on(webview));
    Ok(())
}

/// Whether closing the window now would lose unsaved changes. While it would,
/// closing it sends `closeRequested` instead, and the window stays open.
#[tauri::command]
pub fn set_close_guard(hub: Hub<'_>, args: CloseGuardArgs) -> Result<(), IpcError> {
    hub.set_unsaved(args.unsaved);
    Ok(())
}

/// Closes the calling window without asking again: the page has dealt with
/// its unsaved changes.
#[tauri::command]
pub fn close_window(window: WebviewWindow, hub: Hub<'_>) -> Result<(), IpcError> {
    hub.release_close();
    window
        .destroy()
        .map_err(|error| IpcError::internal(format!("couldn't close the window: {error}")))
}

/// The window's own buttons and title bar, which the page draws.
fn window_failed(what: &str) -> impl FnOnce(tauri::Error) -> IpcError + '_ {
    move |error| IpcError::internal(format!("couldn't {what} the window: {error}"))
}

#[tauri::command]
pub fn window_minimize(window: WebviewWindow) -> Result<(), IpcError> {
    window.minimize().map_err(window_failed("minimize"))
}

/// Maximizes the window, or restores it when it's maximized.
#[tauri::command]
pub fn window_toggle_maximize(window: WebviewWindow) -> Result<(), IpcError> {
    if window.is_maximized().map_err(window_failed("maximize"))? {
        window.unmaximize().map_err(window_failed("restore"))
    } else {
        window.maximize().map_err(window_failed("maximize"))
    }
}

/// Moves the window with the pointer, from a press on the title bar.
#[tauri::command]
pub fn window_drag(window: WebviewWindow) -> Result<(), IpcError> {
    window.start_dragging().map_err(window_failed("move"))
}

/// Asks the window to close, as its system button would: with unsaved
/// changes, the page is asked first (`closeRequested`).
#[tauri::command]
pub fn window_close(window: WebviewWindow) -> Result<(), IpcError> {
    window.close().map_err(window_failed("close"))
}

/// Whether the window is maximized or full screen now.
#[tauri::command]
pub fn window_state(window: WebviewWindow) -> Result<WindowStateDto, IpcError> {
    Ok(crate::window_state(&window.as_ref().window()))
}
