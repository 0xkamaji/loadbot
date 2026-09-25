#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use loadbot::{
    catalog::Runner,
    interaction::{Interaction, Notice, OperationContext, ToolOperationStage, Unattended},
    launcher::{self, Project},
    operations::{self, CatalogState, ShortcutHelpRequest, ShortcutIdentity},
    paths::Paths,
    process::{
        Control, ExecutionPolicy, InteractiveCommand, InteractiveExecutionOutput,
        InteractiveSession, InteractiveSessionEvent, OperationId,
    },
    recipe::RecipeDefinition,
};
use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tauri::Manager;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

const WORKSPACE_LAYOUT_FILE: &str = "workspace-layout-v1.json";
const MAX_WORKSPACE_LAYOUT_BYTES: usize = 4096;

#[derive(Debug, serde::Serialize)]
struct DesktopError {
    kind: &'static str,
    message: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogContext {
    name: String,
    url: String,
    writable: bool,
    state: &'static str,
    default: bool,
}

#[derive(Debug, serde::Serialize)]
struct ProjectIdentity {
    catalog: String,
    tool: String,
}

#[derive(Debug, serde::Serialize)]
struct CatalogIdentity {
    catalog: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum BackendActivity {
    Progress {
        stage: &'static str,
        catalog: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool: Option<String>,
        detail: Option<String>,
    },
    Log {
        stream: &'static str,
        text: String,
    },
    InteractiveLaunch {
        #[serde(rename = "launchId")]
        launch_id: String,
        label: String,
    },
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct InteractiveLaunchRequest {
    /// Opaque capability issued by a backend operation. It is not an executable.
    launch_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitPushRequest {
    catalog: String,
    tool: String,
    selected_paths: Vec<String>,
    commit_message: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InteractiveSessionStarted {
    session_id: String,
    process_id: String,
    os_process_id: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InteractiveLaunchCapability {
    launch_id: String,
    label: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum BackendInteractiveEvent {
    Output {
        session_id: String,
        bytes: Vec<u8>,
    },
    Exited {
        session_id: String,
        code: u32,
        signal: Option<String>,
        cancelled: bool,
    },
    Failed {
        session_id: String,
        message: String,
    },
}

#[derive(Clone, Default)]
struct InteractiveSessions {
    pending: Arc<Mutex<HashMap<String, PendingInteractiveLaunch>>>,
    active: Arc<Mutex<HashMap<String, InteractiveSession>>>,
}

struct PendingInteractiveLaunch {
    command: InteractiveCommand,
    control: Control,
    operation_id: OperationId,
    completion: Option<mpsc::SyncSender<anyhow::Result<InteractiveExecutionOutput>>>,
}

impl InteractiveSessions {
    fn issue(
        &self,
        command: InteractiveCommand,
        control: Control,
        operation_id: OperationId,
        label: String,
        completion: Option<mpsc::SyncSender<anyhow::Result<InteractiveExecutionOutput>>>,
    ) -> anyhow::Result<InteractiveLaunchCapability> {
        let launch_id = format!("launch-{:016x}", OperationId::random().0);
        self.pending
            .lock()
            .map_err(|_| anyhow::anyhow!("interactive launch registry is unavailable"))?
            .insert(
                launch_id.clone(),
                PendingInteractiveLaunch {
                    command,
                    control: Control {
                        policy: ExecutionPolicy::Interactive,
                        interactive_executor: None,
                        ..control
                    },
                    operation_id,
                    completion,
                },
            );
        Ok(InteractiveLaunchCapability { launch_id, label })
    }

    fn start(
        &self,
        launch_id: &str,
        observer: Arc<dyn Fn(BackendInteractiveEvent) + Send + Sync>,
    ) -> anyhow::Result<InteractiveSessionStarted> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| anyhow::anyhow!("interactive launch registry is unavailable"))?
            .remove(launch_id)
            .context("interactive launch is unavailable or has already been used")?;
        // The observer thread must not keep the registry (and therefore its
        // own session handle) alive after application state is dropped.
        let active_registry = Arc::downgrade(&self.active);
        let completion = Arc::new(Mutex::new(pending.completion));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let observer_completion = completion.clone();
        let observer_captured = captured.clone();
        let session = InteractiveSession::start(
            pending.command,
            &pending.control,
            pending.operation_id,
            Arc::new(move |event| {
                let terminal = matches!(
                    event,
                    InteractiveSessionEvent::Exited { .. } | InteractiveSessionEvent::Failed { .. }
                );
                if let InteractiveSessionEvent::Output { bytes, .. } = &event
                    && let Ok(mut output) = observer_captured.lock()
                {
                    const MAX_CAPTURE: usize = 4 * 1024 * 1024;
                    let remaining = MAX_CAPTURE.saturating_sub(output.len());
                    output.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                }
                let event = backend_interactive_event(event);
                let session_id = event.session_id().to_owned();
                observer(event.clone());
                if terminal
                    && let Ok(mut sender) = observer_completion.lock()
                    && let Some(sender) = sender.take()
                {
                    let result = match event {
                        BackendInteractiveEvent::Exited {
                            code,
                            signal,
                            cancelled,
                            ..
                        } => Ok(InteractiveExecutionOutput {
                            status: loadbot::process::InteractiveExitStatus { code, signal },
                            output: observer_captured
                                .lock()
                                .map(|output| output.clone())
                                .unwrap_or_default(),
                            cancelled,
                        }),
                        BackendInteractiveEvent::Failed { message, .. } => {
                            Err(anyhow::anyhow!(message))
                        }
                        BackendInteractiveEvent::Output { .. } => unreachable!(),
                    };
                    let _ = sender.send(result);
                }
                if terminal
                    && let Some(active_registry) = active_registry.upgrade()
                    && let Ok(mut active) = active_registry.lock()
                {
                    active.remove(&session_id);
                }
            }),
        );
        let session = match session {
            Ok(session) => session,
            Err(error) => {
                if let Ok(mut sender) = completion.lock()
                    && let Some(sender) = sender.take()
                {
                    let _ = sender.send(Err(anyhow::anyhow!("{error:#}")));
                }
                return Err(error);
            }
        };
        let session_id = session_key(session.process_id());
        let started = InteractiveSessionStarted {
            session_id: session_id.clone(),
            process_id: process_key(session.process_id()),
            os_process_id: session.os_process_id(),
        };
        self.active
            .lock()
            .map_err(|_| anyhow::anyhow!("interactive session registry is unavailable"))?
            .insert(session_id.clone(), session.clone());
        // A very short child may finish before insertion; do not retain a stale handle.
        if session.is_finished() {
            self.active
                .lock()
                .map_err(|_| anyhow::anyhow!("interactive session registry is unavailable"))?
                .remove(&session_id);
        }
        Ok(started)
    }

    fn session(&self, session_id: &str) -> anyhow::Result<InteractiveSession> {
        self.active
            .lock()
            .map_err(|_| anyhow::anyhow!("interactive session registry is unavailable"))?
            .get(session_id)
            .cloned()
            .context("interactive session is not active")
    }

    fn execute(
        &self,
        command: InteractiveCommand,
        control: Control,
        operation_id: OperationId,
        label: String,
        activity: Channel<BackendActivity>,
    ) -> anyhow::Result<InteractiveExecutionOutput> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let launch = self.issue(command, control.clone(), operation_id, label, Some(sender))?;
        if activity
            .send(BackendActivity::InteractiveLaunch {
                launch_id: launch.launch_id.clone(),
                label: launch.label,
            })
            .is_err()
        {
            self.pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&launch.launch_id));
            anyhow::bail!("could not deliver the interactive launch to the GUI");
        }

        loop {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if control.cancellation.is_cancelled() {
                        self.pending
                            .lock()
                            .ok()
                            .and_then(|mut pending| pending.remove(&launch.launch_id));
                        return Err(loadbot::process::Cancelled.into());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("interactive session completion was disconnected")
                }
            }
        }
    }

    #[cfg(test)]
    fn register_test_launch(&self, launch_id: &str, command: InteractiveCommand) {
        self.pending.lock().unwrap().insert(
            launch_id.to_owned(),
            PendingInteractiveLaunch {
                command,
                control: Control {
                    policy: ExecutionPolicy::Interactive,
                    ..Control::default()
                },
                operation_id: OperationId::random(),
                completion: None,
            },
        );
    }
}

impl BackendInteractiveEvent {
    fn session_id(&self) -> &str {
        match self {
            Self::Output { session_id, .. }
            | Self::Exited { session_id, .. }
            | Self::Failed { session_id, .. } => session_id,
        }
    }
}

fn session_key(process_id: loadbot::process::ProcessId) -> String {
    format!("session-{:016x}", process_id.0)
}

fn process_key(process_id: loadbot::process::ProcessId) -> String {
    format!("process-{:016x}", process_id.0)
}

fn backend_interactive_event(event: InteractiveSessionEvent) -> BackendInteractiveEvent {
    match event {
        InteractiveSessionEvent::Output { process_id, bytes } => BackendInteractiveEvent::Output {
            session_id: session_key(process_id),
            bytes,
        },
        InteractiveSessionEvent::Exited {
            process_id,
            status,
            cancelled,
        } => BackendInteractiveEvent::Exited {
            session_id: session_key(process_id),
            code: status.code,
            signal: status.signal,
            cancelled,
        },
        InteractiveSessionEvent::Failed {
            process_id,
            diagnostic,
        } => BackendInteractiveEvent::Failed {
            session_id: session_key(process_id),
            message: diagnostic,
        },
    }
}

struct DesktopInteraction {
    activity: Option<Channel<BackendActivity>>,
}

impl Interaction for DesktopInteraction {
    fn notice(&mut self, notice: &Notice) {
        if let (Some(channel), Some(activity)) = (&self.activity, backend_activity(notice)) {
            let _ = channel.send(activity);
        }
    }
}

fn backend_activity(notice: &Notice) -> Option<BackendActivity> {
    Some(match notice {
        Notice::CatalogSyncStarted { name } => BackendActivity::Progress {
            stage: "validating",
            catalog: name.clone(),
            tool: None,
            detail: None,
        },
        Notice::CatalogSyncRepositoryChecked { name } => BackendActivity::Progress {
            stage: "repository-checked",
            catalog: name.clone(),
            tool: None,
            detail: None,
        },
        Notice::CatalogSyncUpdateStarted { name } => BackendActivity::Progress {
            stage: "updating-repository",
            catalog: name.clone(),
            tool: None,
            detail: None,
        },
        Notice::CatalogCurrent { name, new_commit } => BackendActivity::Progress {
            stage: "current",
            catalog: name.clone(),
            tool: None,
            detail: Some(new_commit.clone()),
        },
        Notice::CatalogSynced {
            name,
            old_commit,
            new_commit,
        } => BackendActivity::Progress {
            stage: "updated",
            catalog: name.clone(),
            tool: None,
            detail: Some(format!("{old_commit} → {new_commit}")),
        },
        Notice::ToolOperationStage {
            operation: _,
            stage,
            name,
            catalog_name,
        } => BackendActivity::Progress {
            stage: match stage {
                ToolOperationStage::ValidatingCheckout => "validating-checkout",
                ToolOperationStage::InspectingRepository => "inspecting-repository",
                ToolOperationStage::AwaitingCommit => "awaiting-commit",
                ToolOperationStage::StagingChanges => "staging-changes",
                ToolOperationStage::CreatingCommit => "creating-commit",
                ToolOperationStage::CloningProject => "cloning-project",
                ToolOperationStage::ValidatingFreshCheckout => "validating-fresh-checkout",
                ToolOperationStage::FetchingAndUpdating => "fetching-and-updating",
                ToolOperationStage::PushingCommits => "pushing-commits",
                ToolOperationStage::RemovingCheckout => "removing-checkout",
                ToolOperationStage::ReplacingCheckout => "replacing-checkout",
            },
            catalog: catalog_name.clone(),
            tool: Some(name.clone()),
            detail: None,
        },
        _ => return None,
    })
}

fn backend_process_activity(event: loadbot::process::Event) -> Option<BackendActivity> {
    use loadbot::process::{Event, Stream};
    let log = |stream, text| BackendActivity::Log { stream, text };
    Some(match event {
        Event::Starting {
            program,
            arguments,
            directory,
            ..
        } => {
            let command = std::iter::once(program)
                .chain(arguments)
                .map(|part| quote_log_argument(&part.to_string_lossy()))
                .collect::<Vec<_>>()
                .join(" ");
            let text = directory.map_or(command.clone(), |path| {
                format!("{command}\nworking directory: {}", path.display())
            });
            log("command", text)
        }
        Event::Output { stream, bytes, .. } => log(
            match stream {
                Stream::Stdout => "stdout",
                Stream::Stderr => "stderr",
            },
            String::from_utf8_lossy(&bytes).into_owned(),
        ),
        Event::Exited { status, .. } => log("system", format!("process exited with {status}")),
        Event::Cancelled { .. } => log("system", "process cancelled".into()),
        Event::Failed { diagnostic, .. } => log("system", diagnostic),
        Event::InteractiveStarted { .. } => log("system", "interactive process started".into()),
        Event::InteractiveExited { status, .. } => log(
            "system",
            if status.success() {
                "interactive process exited successfully".into()
            } else {
                format!("interactive process exited with code {}", status.code)
            },
        ),
        Event::InteractiveCancelled { .. } => log("system", "interactive process cancelled".into()),
        Event::OperationStarted { .. }
        | Event::OperationFinished { .. }
        | Event::Started { .. } => {
            return None;
        }
    })
}

#[tauri::command]
fn start_loadbot_interactive_session(
    request: InteractiveLaunchRequest,
    on_event: Channel<BackendInteractiveEvent>,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<InteractiveSessionStarted, DesktopError> {
    sessions
        .start(
            &request.launch_id,
            Arc::new(move |event| {
                let _ = on_event.send(event);
            }),
        )
        .map_err(interactive_error)
}

#[tauri::command]
fn send_loadbot_interactive_input(
    session_id: String,
    input: String,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<(), DesktopError> {
    // `input` is intentionally opaque and is never formatted, retained, or emitted.
    sessions
        .session(&session_id)
        .and_then(|session| session.send_input(input.as_bytes()))
        .map_err(interactive_error)
}

#[tauri::command]
fn resize_loadbot_interactive_session(
    session_id: String,
    rows: u16,
    columns: u16,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<(), DesktopError> {
    sessions
        .session(&session_id)
        .and_then(|session| session.resize(rows, columns))
        .map_err(interactive_error)
}

#[tauri::command]
fn terminate_loadbot_interactive_session(
    session_id: String,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<(), DesktopError> {
    sessions
        .session(&session_id)
        .and_then(|session| session.terminate())
        .map_err(interactive_error)
}

fn interactive_error(error: anyhow::Error) -> DesktopError {
    DesktopError {
        kind: "interactive-session",
        message: format!("{error:#}"),
    }
}

fn quote_log_argument(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._/:=@".contains(character))
    {
        value.to_owned()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[tauri::command]
async fn read_loadbot_inventory() -> Result<Vec<Project>, DesktopError> {
    // Inventory and catalog context deliberately share the same path discovery
    // helper, so the installed GUI resolves exactly the state used by the CLI.
    run_loadbot_worker("inventory query", move |paths, context| {
        launcher::read_project_inventory(paths, context)
    })
    .await
}

#[tauri::command]
async fn open_loadbot_project(catalog: String, tool: String) -> Result<(), DesktopError> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<()> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::background(&mut policy);
            let directory =
                launcher::resolve_project_directory(&paths, &catalog, &tool, &mut context)?;
            open_directory(&directory)
        })();
        result.map_err(|error| DesktopError {
            kind: "operation",
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("project-folder worker failed: {error}"),
    })?
}

#[tauri::command]
async fn create_loadbot_project_terminal_launch(
    catalog: String,
    tool: String,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<InteractiveLaunchCapability, DesktopError> {
    let sessions = sessions.inner().clone();
    run_loadbot_worker("project terminal", move |paths, context| {
        let command = operations::project_terminal_command(paths, &tool, &catalog, context)?;
        sessions.issue(
            command,
            context.process.clone(),
            OperationId::random(),
            format!("Project terminal — {tool}"),
            None,
        )
    })
    .await
}

#[tauri::command]
async fn read_loadbot_catalogs() -> Result<Vec<CatalogContext>, DesktopError> {
    run_loadbot_worker("catalog query", move |paths, context| {
        Ok(operations::catalog_list(paths, context)?
            .into_iter()
            .map(|catalog| CatalogContext {
                name: catalog.name,
                url: catalog.source.url,
                writable: catalog.source.writable,
                state: match catalog.state {
                    CatalogState::Missing => "missing",
                    CatalogState::Installed => "installed",
                    CatalogState::Mismatch => "mismatch",
                },
                default: catalog.default,
            })
            .collect())
    })
    .await
}

#[tauri::command]
async fn add_loadbot_catalog(
    name: String,
    url: String,
    writable: bool,
) -> Result<CatalogIdentity, DesktopError> {
    run_loadbot_worker("catalog add", move |paths, context| {
        operations::catalog_add(paths, &name, url, writable, context)?;
        Ok(CatalogIdentity { catalog: name })
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn add_loadbot_project(
    catalog: String,
    name: String,
    url: String,
    revision: Option<String>,
    commit: bool,
    push: bool,
) -> Result<ProjectIdentity, DesktopError> {
    let identity = ProjectIdentity {
        catalog: catalog.clone(),
        tool: name.clone(),
    };
    run_loadbot_worker("project add", move |paths, context| {
        operations::tool_add(paths, &catalog, &name, url, revision, commit, push, context)?;
        Ok(identity)
    })
    .await
}

async fn project_operation(
    label: &'static str,
    catalog: String,
    tool: String,
    activity: Channel<BackendActivity>,
    operation: fn(
        &Paths,
        &str,
        Option<&str>,
        &mut OperationContext<'_>,
    ) -> anyhow::Result<loadbot::interaction::MutationOutcome>,
) -> Result<ProjectIdentity, DesktopError> {
    let identity = ProjectIdentity {
        catalog: catalog.clone(),
        tool: tool.clone(),
    };
    run_loadbot_worker_with_activity(label, Some(activity), move |paths, context| {
        operation(paths, &tool, Some(&catalog), context)?;
        Ok(identity)
    })
    .await
}

#[tauri::command]
async fn pull_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project pull",
        catalog,
        tool,
        on_activity,
        operations::tool_pull,
    )
    .await
}

#[tauri::command]
async fn update_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project update",
        catalog,
        tool,
        on_activity,
        operations::tool_update,
    )
    .await
}

#[tauri::command]
async fn inspect_loadbot_project_push(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<operations::ToolPushInspection, DesktopError> {
    run_loadbot_worker_with_activity(
        "project push inspection",
        Some(on_activity),
        move |paths, context| operations::tool_push_inspect(paths, &tool, Some(&catalog), context),
    )
    .await
}

enum ProjectPush {
    ExistingCommits,
    Commit {
        selected_paths: Vec<String>,
        message: String,
    },
}

async fn project_push_operation(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
    sessions: InteractiveSessions,
    push: ProjectPush,
) -> Result<ProjectIdentity, DesktopError> {
    let identity = ProjectIdentity {
        catalog: catalog.clone(),
        tool: tool.clone(),
    };
    let launch_activity = on_activity.clone();
    let session_label = format!("Git push — {tool}");
    run_loadbot_worker_with_activity("project push", Some(on_activity), move |paths, context| {
        let session_control = context.process.clone();
        context.process.interactive_executor = Some(Arc::new(move |command, operation_id| {
            sessions.execute(
                command,
                session_control.clone(),
                operation_id,
                session_label.clone(),
                launch_activity.clone(),
            )
        }));
        match push {
            ProjectPush::ExistingCommits => {
                operations::tool_push(paths, &tool, Some(&catalog), context)?;
            }
            ProjectPush::Commit {
                selected_paths,
                message,
            } => {
                operations::tool_commit_and_push(
                    paths,
                    &tool,
                    Some(&catalog),
                    &selected_paths,
                    &message,
                    context,
                )?;
            }
        }
        Ok(identity)
    })
    .await
}

#[tauri::command]
async fn push_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<ProjectIdentity, DesktopError> {
    project_push_operation(
        catalog,
        tool,
        on_activity,
        sessions.inner().clone(),
        ProjectPush::ExistingCommits,
    )
    .await
}

#[tauri::command]
async fn commit_and_push_loadbot_project(
    request: CommitPushRequest,
    on_activity: Channel<BackendActivity>,
    sessions: tauri::State<'_, InteractiveSessions>,
) -> Result<ProjectIdentity, DesktopError> {
    project_push_operation(
        request.catalog,
        request.tool,
        on_activity,
        sessions.inner().clone(),
        ProjectPush::Commit {
            selected_paths: request.selected_paths,
            message: request.commit_message,
        },
    )
    .await
}

#[tauri::command]
async fn remove_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project remove",
        catalog,
        tool,
        on_activity,
        operations::tool_remove,
    )
    .await
}

#[tauri::command]
async fn reinstall_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project reinstall",
        catalog,
        tool,
        on_activity,
        operations::tool_reinstall,
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn add_loadbot_shortcut(
    catalog: String,
    tool: String,
    name: String,
    path: String,
    description: Option<String>,
    runner: Option<Runner>,
) -> Result<ShortcutIdentity, DesktopError> {
    run_loadbot_worker("shortcut add", move |paths, context| {
        operations::shortcut_add(
            paths,
            &catalog,
            &tool,
            &name,
            &path,
            description,
            runner,
            context,
        )
    })
    .await
}

#[tauri::command]
async fn add_loadbot_recipe_shortcut(
    catalog: String,
    tool: String,
    name: String,
    description: Option<String>,
    recipe: RecipeDefinition,
) -> Result<ShortcutIdentity, DesktopError> {
    run_loadbot_worker("Recipe shortcut add", move |paths, context| {
        operations::shortcut_add_recipe(paths, &catalog, &tool, &name, description, recipe, context)
    })
    .await
}

#[tauri::command]
async fn update_loadbot_recipe_shortcut(
    catalog: String,
    tool: String,
    name: String,
    description: Option<String>,
    recipe: RecipeDefinition,
) -> Result<ShortcutIdentity, DesktopError> {
    run_loadbot_worker("Recipe shortcut update", move |paths, context| {
        operations::shortcut_update_recipe(
            paths,
            &catalog,
            &tool,
            &name,
            description,
            recipe,
            context,
        )
    })
    .await
}

#[tauri::command]
async fn delete_loadbot_shortcuts(
    identities: Vec<ShortcutIdentity>,
) -> Result<usize, DesktopError> {
    run_loadbot_worker("shortcut delete", move |paths, context| {
        operations::shortcut_delete_many(paths, &identities, context)
    })
    .await
}

async fn choose_project_path(
    app: tauri::AppHandle,
    catalog: String,
    tool: String,
    kind: operations::ProjectPathKind,
) -> Result<Option<String>, DesktopError> {
    let root_catalog = catalog.clone();
    let root_tool = tool.clone();
    let root = run_loadbot_worker("project path lookup", move |paths, context| {
        operations::installed_tool_path(paths, &root_tool, &root_catalog, context)
    })
    .await?;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let picker = app.dialog().file().set_directory(root);
        match kind {
            operations::ProjectPathKind::File => picker.blocking_pick_file(),
            operations::ProjectPathKind::Directory => picker.blocking_pick_folder(),
        }
        .map(|path| path.into_path())
        .transpose()
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("project picker worker failed: {error}"),
    })?
    .map_err(|error| DesktopError {
        kind: "operation",
        message: format!("selected path is not a native filesystem path: {error}"),
    })?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    run_loadbot_worker("project path validation", move |paths, context| {
        operations::portable_project_path(paths, &catalog, &tool, &selected, kind, context)
            .map(Some)
    })
    .await
}

#[tauri::command]
async fn choose_loadbot_project_file(
    app: tauri::AppHandle,
    catalog: String,
    tool: String,
) -> Result<Option<String>, DesktopError> {
    choose_project_path(app, catalog, tool, operations::ProjectPathKind::File).await
}

#[tauri::command]
async fn choose_loadbot_project_directory(
    app: tauri::AppHandle,
    catalog: String,
    tool: String,
) -> Result<Option<String>, DesktopError> {
    choose_project_path(app, catalog, tool, operations::ProjectPathKind::Directory).await
}

#[tauri::command]
async fn view_loadbot_shortcut_help(
    request: ShortcutHelpRequest,
) -> Result<launcher::HelpResult, DesktopError> {
    run_loadbot_worker("shortcut help", move |paths, context| {
        operations::shortcut_help(paths, &request, context)
    })
    .await
}

#[tauri::command]
async fn sync_loadbot_catalog(
    catalog: String,
    on_activity: Channel<BackendActivity>,
) -> Result<(), DesktopError> {
    run_loadbot_worker_with_activity("catalog sync", Some(on_activity), move |paths, context| {
        operations::catalog_sync(paths, &catalog, context)?;
        Ok(())
    })
    .await
}

async fn run_loadbot_worker<T, F>(label: &'static str, operation: F) -> Result<T, DesktopError>
where
    T: Send + 'static,
    F: FnOnce(&Paths, &mut OperationContext<'_>) -> anyhow::Result<T> + Send + 'static,
{
    run_loadbot_worker_with_activity(label, None, operation).await
}

async fn run_loadbot_worker_with_activity<T, F>(
    label: &'static str,
    activity: Option<Channel<BackendActivity>>,
    operation: F,
) -> Result<T, DesktopError>
where
    T: Send + 'static,
    F: FnOnce(&Paths, &mut OperationContext<'_>) -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<T> {
            let paths = Paths::discover()?;
            let process_activity = activity.clone();
            let mut policy = DesktopInteraction { activity };
            let mut context = OperationContext::background(&mut policy);
            if let Some(channel) = process_activity {
                context.process.observer = Some(Arc::new(move |event| {
                    if let Some(activity) = backend_process_activity(event) {
                        let _ = channel.send(activity);
                    }
                }));
            }
            context.run(|context| operation(&paths, context)).result
        })();
        result.map_err(|error| DesktopError {
            kind: if error
                .downcast_ref::<loadbot::process::Cancelled>()
                .is_some()
            {
                "cancelled"
            } else {
                "operation"
            },
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("{label} worker failed: {error}"),
    })?
}

#[tauri::command]
fn read_loadbot_workspace_layout(app: tauri::AppHandle) -> Result<Option<String>, DesktopError> {
    let result = workspace_layout_path(&app).and_then(|path| read_workspace_layout(&path));
    result.map_err(desktop_error)
}

#[tauri::command]
fn write_loadbot_workspace_layout(
    app: tauri::AppHandle,
    contents: String,
) -> Result<(), DesktopError> {
    let result =
        workspace_layout_path(&app).and_then(|path| write_workspace_layout(&path, &contents));
    result.map_err(desktop_error)
}

fn desktop_error(error: anyhow::Error) -> DesktopError {
    DesktopError {
        kind: "storage",
        message: format!("{error:#}"),
    }
}

fn workspace_layout_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_local_data_dir()?.join(WORKSPACE_LAYOUT_FILE))
}

fn read_workspace_layout(path: &Path) -> anyhow::Result<Option<String>> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("refusing symlink workspace layout {}", path.display());
    }
    match fs::read_to_string(path) {
        Ok(contents) if contents.len() <= MAX_WORKSPACE_LAYOUT_BYTES => Ok(Some(contents)),
        Ok(_) => anyhow::bail!("workspace layout is unexpectedly large"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("could not read workspace layout {}", path.display())),
    }
}

fn write_workspace_layout(path: &Path, contents: &str) -> anyhow::Result<()> {
    if contents.len() > MAX_WORKSPACE_LAYOUT_BYTES {
        anyhow::bail!("workspace layout is unexpectedly large");
    }
    let parent = path.parent().context("workspace layout has no parent")?;
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("refusing symlink workspace layout {}", path.display());
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".loadbot-workspace-")
        .tempfile_in(parent)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn open_directory(path: &Path) -> anyhow::Result<()> {
    let mut command = directory_open_command(path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "could not open project directory {} with the system file manager",
                path.display()
            )
        })?;
    Ok(())
}

fn directory_open_command(path: &Path) -> Command {
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let mut command = Command::new("false");
    command.arg(path);
    command
}

fn main() {
    // The same native host and qualified semantic capabilities serve Windows and Linux.
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(InteractiveSessions::default())
        .invoke_handler(tauri::generate_handler![
            start_loadbot_interactive_session,
            send_loadbot_interactive_input,
            resize_loadbot_interactive_session,
            terminate_loadbot_interactive_session,
            read_loadbot_inventory,
            read_loadbot_catalogs,
            open_loadbot_project,
            create_loadbot_project_terminal_launch,
            add_loadbot_catalog,
            add_loadbot_project,
            pull_loadbot_project,
            update_loadbot_project,
            inspect_loadbot_project_push,
            push_loadbot_project,
            commit_and_push_loadbot_project,
            remove_loadbot_project,
            reinstall_loadbot_project,
            add_loadbot_shortcut,
            add_loadbot_recipe_shortcut,
            update_loadbot_recipe_shortcut,
            delete_loadbot_shortcuts,
            choose_loadbot_project_file,
            choose_loadbot_project_directory,
            view_loadbot_shortcut_help,
            sync_loadbot_catalog,
            read_loadbot_workspace_layout,
            write_loadbot_workspace_layout
        ])
        .run(tauri::generate_context!())
        .expect("could not launch the Loadbot desktop window");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folder_is_one_literal_argument_without_a_shell() {
        let path = Path::new("project with spaces;and-metacharacters");
        let command = directory_open_command(path);
        #[cfg(target_os = "windows")]
        assert_eq!(command.get_program(), "explorer.exe");
        #[cfg(target_os = "linux")]
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
    }

    #[cfg(unix)]
    #[test]
    fn backend_issued_terminal_and_auth_launches_have_distinct_sessions() {
        let sessions = InteractiveSessions::default();
        let command = |label: &str| {
            let mut command = InteractiveCommand::new("sh");
            command.args(["-c", &format!("printf '{label}\\n'; sleep 1")]);
            command
        };
        let terminal = sessions
            .issue(
                command("terminal"),
                Control::default(),
                OperationId::random(),
                "Project terminal — demo".into(),
                None,
            )
            .unwrap();
        let auth = sessions
            .issue(
                command("auth"),
                Control::default(),
                OperationId::random(),
                "Git push — demo".into(),
                None,
            )
            .unwrap();
        assert_ne!(terminal.launch_id, auth.launch_id);
        let terminal_session = sessions
            .start(&terminal.launch_id, Arc::new(|_| {}))
            .unwrap();
        let auth_session = sessions.start(&auth.launch_id, Arc::new(|_| {})).unwrap();
        assert_ne!(terminal_session.session_id, auth_session.session_id);
        assert!(sessions.session(&terminal_session.session_id).is_ok());
        assert!(sessions.session(&auth_session.session_id).is_ok());
        sessions
            .session(&terminal_session.session_id)
            .unwrap()
            .terminate()
            .unwrap();
        assert!(sessions.session(&auth_session.session_id).is_ok());
        sessions
            .session(&auth_session.session_id)
            .unwrap()
            .terminate()
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dropping_the_session_registry_terminates_a_live_project_shell() {
        let sessions = InteractiveSessions::default();
        let active_registry = Arc::downgrade(&sessions.active);
        let mut command = InteractiveCommand::new("sh");
        command.args(["-c", "sleep 30"]);
        let launch = sessions
            .issue(
                command,
                Control::default(),
                OperationId::random(),
                "Project terminal — demo".into(),
                None,
            )
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        sessions
            .start(
                &launch.launch_id,
                Arc::new(move |event| {
                    let _ = sender.send(event);
                }),
            )
            .unwrap();
        drop(sessions);
        assert!(active_registry.upgrade().is_none());
        loop {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                BackendInteractiveEvent::Exited { cancelled, .. } => {
                    assert!(cancelled);
                    break;
                }
                BackendInteractiveEvent::Failed { message, .. } => panic!("{message}"),
                BackendInteractiveEvent::Output { .. } => {}
            }
        }
    }

    #[test]
    fn native_activity_maps_typed_core_sync_notices_without_cli_text() {
        let checked = backend_activity(&Notice::CatalogSyncRepositoryChecked {
            name: "personal".into(),
        })
        .unwrap();
        let BackendActivity::Progress {
            stage,
            catalog,
            detail,
            ..
        } = checked
        else {
            panic!("expected progress activity")
        };
        assert_eq!(stage, "repository-checked");
        assert_eq!(catalog, "personal");
        assert!(detail.is_none());

        let updated = backend_activity(&Notice::CatalogSynced {
            name: "personal".into(),
            old_commit: "abc".into(),
            new_commit: "def".into(),
        })
        .unwrap();
        let BackendActivity::Progress { stage, detail, .. } = updated else {
            panic!("expected progress activity")
        };
        assert_eq!(stage, "updated");
        assert_eq!(detail.as_deref(), Some("abc → def"));

        let project = backend_activity(&Notice::ToolOperationStage {
            operation: loadbot::interaction::ToolOperation::Reinstall,
            stage: ToolOperationStage::ReplacingCheckout,
            name: "demo".into(),
            catalog_name: "personal".into(),
        })
        .unwrap();
        let BackendActivity::Progress {
            stage,
            catalog,
            tool,
            detail,
        } = project
        else {
            panic!("expected progress activity")
        };
        assert_eq!(stage, "replacing-checkout");
        assert_eq!(catalog, "personal");
        assert_eq!(tool.as_deref(), Some("demo"));
        assert!(detail.is_none());
    }

    #[test]
    fn native_activity_maps_real_process_commands_and_output_as_verbose_logs() {
        let command = backend_process_activity(loadbot::process::Event::Starting {
            operation_id: OperationId::random(),
            process_id: loadbot::process::ProcessId::random(),
            program: "git".into(),
            arguments: ["fetch", "origin"].into_iter().map(Into::into).collect(),
            directory: Some(PathBuf::from("catalog path")),
        })
        .unwrap();
        let BackendActivity::Log { stream, text } = command else {
            panic!("expected log activity")
        };
        assert_eq!(stream, "command");
        assert_eq!(text, "git fetch origin\nworking directory: catalog path");

        let stderr = backend_process_activity(loadbot::process::Event::Output {
            operation_id: OperationId::random(),
            process_id: loadbot::process::ProcessId::random(),
            stream: loadbot::process::Stream::Stderr,
            bytes: b"Permission denied (publickey).\n".to_vec(),
        })
        .unwrap();
        let BackendActivity::Log { stream, text } = stderr else {
            panic!("expected log activity")
        };
        assert_eq!(stream, "stderr");
        assert_eq!(text, "Permission denied (publickey).\n");
    }

    #[test]
    fn workspace_layout_is_gui_local_atomic_and_opaque() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(WORKSPACE_LAYOUT_FILE);
        assert_eq!(read_workspace_layout(&path).unwrap(), None);
        let first = r#"{"version":1,"projects":320,"shortcuts":240,"terminal":180}"#;
        write_workspace_layout(&path, first).unwrap();
        assert_eq!(
            read_workspace_layout(&path).unwrap().as_deref(),
            Some(first)
        );
        let malformed = "not json; validation belongs to the presentation layer";
        write_workspace_layout(&path, malformed).unwrap();
        assert_eq!(
            read_workspace_layout(&path).unwrap().as_deref(),
            Some(malformed)
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn interactive_bridge_rejects_unissued_launch_capabilities() {
        let sessions = InteractiveSessions::default();
        let error = sessions
            .start("frontend-supplied-program", Arc::new(|_| {}))
            .unwrap_err();
        assert!(format!("{error:#}").contains("launch is unavailable"));
    }

    #[test]
    fn interactive_events_use_the_frontend_wire_contract() {
        use tauri::ipc::InvokeResponseBody;

        let (sender, receiver) = mpsc::channel();
        let channel = Channel::<BackendInteractiveEvent>::new(move |body| {
            let InvokeResponseBody::Json(json) = body else {
                panic!("expected JSON interactive event")
            };
            sender.send(json).unwrap();
            Ok(())
        });
        let event = BackendInteractiveEvent::Output {
            session_id: "session-1".into(),
            bytes: b"prompt> ".to_vec(),
        };
        channel.send(event).unwrap();
        let json = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(json.contains(r#""kind":"output""#));
        assert!(json.contains(r#""sessionId":"session-1""#));
        assert!(!json.contains("session_id"));
        assert!(json.contains(r#""bytes":[112,114,111,109,112,116,62,32]"#));
    }

    #[cfg(unix)]
    #[test]
    fn interactive_bridge_routes_output_input_exit_and_removes_the_session() {
        use std::{sync::mpsc, time::Duration};

        let sessions = InteractiveSessions::default();
        let mut command = InteractiveCommand::new("sh");
        command.args([
            "-c",
            "printf 'bridge-ready\\n'; IFS= read -r value; printf 'bridge:%s\\n' \"$value\"; stty size",
        ]);
        sessions.register_test_launch("issued-by-backend", command);
        let (sender, receiver) = mpsc::channel();
        let started = sessions
            .start(
                "issued-by-backend",
                Arc::new(move |event| {
                    let _ = sender.send(event);
                }),
            )
            .unwrap();
        let first = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let BackendInteractiveEvent::Output { bytes, .. } = first else {
            panic!("expected immediate startup output")
        };
        assert!(String::from_utf8_lossy(&bytes).contains("bridge-ready"));
        sessions
            .session(&started.session_id)
            .unwrap()
            .resize(31, 99)
            .unwrap();
        sessions
            .session(&started.session_id)
            .unwrap()
            .send_input(b"opaque-value\n")
            .unwrap();
        let mut output = Vec::new();
        let exit = loop {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                BackendInteractiveEvent::Output { bytes, .. } => output.extend(bytes),
                event @ BackendInteractiveEvent::Exited { .. } => break event,
                BackendInteractiveEvent::Failed { message, .. } => panic!("{message}"),
            }
        };
        assert!(String::from_utf8_lossy(&output).contains("bridge:opaque-value"));
        assert!(String::from_utf8_lossy(&output).contains("31 99"));
        assert!(matches!(
            exit,
            BackendInteractiveEvent::Exited { code: 0, .. }
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while sessions.session(&started.session_id).is_ok() && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert!(sessions.session(&started.session_id).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn backend_issued_launch_completes_only_after_the_pty_exits() {
        use tauri::ipc::InvokeResponseBody;

        let sessions = InteractiveSessions::default();
        let worker_sessions = sessions.clone();
        let (launch_sender, launch_receiver) = mpsc::channel();
        let activity = Channel::<BackendActivity>::new(move |body| {
            let InvokeResponseBody::Json(json) = body else {
                panic!("expected JSON activity")
            };
            launch_sender.send(json).unwrap();
            Ok(())
        });
        let worker = std::thread::spawn(move || {
            let mut command = InteractiveCommand::new("sh");
            command.args(["-c", "printf 'push-finished\\n'"]);
            worker_sessions.execute(
                command,
                Control::default(),
                OperationId::random(),
                "Git push — demo".into(),
                activity,
            )
        });

        let activity = launch_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(activity.contains("interactive-launch"));
        assert!(activity.contains("Git push — demo"));
        let launch_id = sessions
            .pending
            .lock()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        assert!(!worker.is_finished());
        sessions.start(&launch_id, Arc::new(|_| {})).unwrap();
        let result = worker.join().unwrap().unwrap();
        assert!(result.status.success());
        assert!(!result.cancelled);
        assert!(String::from_utf8_lossy(&result.output).contains("push-finished"));
    }
}
