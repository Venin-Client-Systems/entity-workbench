//! Explicit packaged startup proof. No path selector, worker submission or UI injection.
#[cfg(feature = "development-graph-startup-proof")]
use std::sync::Arc;
use std::{
    fs,
    path::{Component, Path},
    sync::Mutex,
};
#[cfg(feature = "development-graph-startup-proof")]
use tauri::Manager;
use workbench_core::{
    coordinator::JobCoordinator,
    domain::Command,
    graph_api::{GraphAvailability, GraphJobPage, GraphJobPageRequest},
};
#[cfg(feature = "development-graph-startup-proof")]
use workbench_core::{engines::CancellationToken, store::Workspace};

const PREFIX: &str = "org.entityworkbench.graphstartupproof.";
#[derive(Debug)]
pub(crate) struct Refusal(&'static str);
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Graph startup proof refused: {}", self.0)
    }
}
impl std::error::Error for Refusal {}
type Result<T> = std::result::Result<T, Refusal>;

fn require(ok: bool, stage: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Refusal(stage))
    }
}
fn identifier(value: &str) -> bool {
    let Some(uuid) = value.strip_prefix(PREFIX) else {
        return false;
    };
    // Canonical UUID text only, without accepting uppercase, braces, URNs or compact forms.
    uuid.len() == 36
        && uuid.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
fn ordinary_ancestors(path: &Path, absent_leaf: bool) -> Result<()> {
    require(
        path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
        "path_shape",
    )?;
    for (i, ancestor) in path.ancestors().enumerate() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                require(!metadata.file_type().is_symlink(), "linked_path")?;
                require(!(absent_leaf && i == 0), "existing_app_data")?;
                require(i == 0 || metadata.is_dir(), "path_ancestor")?;
            }
            Err(error) if absent_leaf && i == 0 && error.kind() == std::io::ErrorKind::NotFound => {
            }
            Err(_) => return Err(Refusal("path_unavailable")),
        }
    }
    Ok(())
}

/// Read-only checks only. Called before Workspace::open or any directory creation.
fn guard(id: &str, local: &Path, resources: &Path, executable: &Path) -> Result<()> {
    require(
        identifier(id) && local.file_name().and_then(|p| p.to_str()) == Some(id),
        "proof_identifier",
    )?;
    ordinary_ancestors(local, true)?;
    ordinary_ancestors(resources, false)?;
    ordinary_ancestors(executable, false)?;
    let macos = executable.parent().ok_or(Refusal("bundle_layout"))?;
    let contents = macos.parent().ok_or(Refusal("bundle_layout"))?;
    let bundle = contents.parent().ok_or(Refusal("bundle_layout"))?;
    require(
        macos.file_name().is_some_and(|n| n == "MacOS")
            && contents.file_name().is_some_and(|n| n == "Contents")
            && bundle.extension().is_some_and(|n| n == "app")
            && resources == contents.join("Resources"),
        "bundle_layout",
    )?;
    require(
        fs::symlink_metadata(resources).is_ok_and(|m| m.is_dir())
            && fs::symlink_metadata(executable).is_ok_and(|m| m.is_file()),
        "bundle_file_types",
    )
}

#[cfg(any(unix, feature = "development-graph-startup-proof"))]
fn reserve_fresh_app_data(local: &Path) -> Result<()> {
    // A second launch must not adopt a directory created after the read-only guard.
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    };
    builder
        .create(local)
        .map_err(|_| Refusal("app_data_reservation"))
}

fn empty_ready(page: &GraphJobPage) -> bool {
    page.schema_version == 1
        && page.availability == GraphAvailability::Ready
        && page.workspace_revision == 0
        && page.total_count == 0
        && page.rows.is_empty()
        && page.next_cursor.is_none()
}
fn read_empty(coordinator: &JobCoordinator) -> bool {
    coordinator
        .dispatch_summary(Command::PageGraphJobs {
            request: GraphJobPageRequest {
                page_size: 1,
                cursor: None,
            },
            expected_revision: Some(0),
        })
        .ok()
        .and_then(|v| serde_json::from_value::<GraphJobPage>(v).ok())
        .is_some_and(|p| empty_ready(&p))
}

#[derive(Default)]
struct State {
    created: bool,
    loaded: bool,
    ready_emitted: bool,
    failed: bool,
    shutdown_started: bool,
}
pub(crate) struct Observation {
    state: Mutex<State>,
}
fn emit(event: &'static str, details: serde_json::Value) {
    // Fixed labels/booleans only; no paths, app/job identifiers, URLs or error bodies.
    eprintln!(
        "GRAPH_STARTUP_PROOF {}",
        serde_json::json!({"schema_version":1,
        "kind":"development_graph_startup_proof","event":event,"details":details})
    );
}
impl Observation {
    fn new() -> Self {
        Self {
            state: Mutex::new(State::default()),
        }
    }
    fn update(&self, created: bool, loaded: bool, valid: bool) -> Option<&'static str> {
        let mut state = self.state.lock().unwrap_or_else(|e| {
            let mut s = e.into_inner();
            s.failed = true;
            s
        });
        if state.shutdown_started {
            return None;
        }
        if !valid {
            if state.failed {
                return None;
            }
            state.failed = true;
            return Some("window_observation_refused");
        }
        state.created |= created;
        state.loaded |= loaded;
        if !state.failed && state.created && state.loaded && !state.ready_emitted {
            state.ready_emitted = true;
            return Some("bundled_window_ready");
        }
        None
    }
    pub(crate) fn window_created(&self, label: &str) {
        if let Some(event) = self.update(true, false, label == "main") {
            emit(
                event,
                serde_json::json!({"window_created":true,"bundled_document_finished":event=="bundled_window_ready"}),
            );
        }
    }
    pub(crate) fn page_load(&self, label: &str, url: &str, finished: bool) {
        // Locked macOS Tauri uses this exact internal origin for index.html.
        // No local dev server, external URL, alternate document or supplied script.
        let valid = label == "main" && matches!(url, "tauri://localhost/" | "tauri://localhost");
        if let Some(event) = self.update(false, finished, valid) {
            emit(
                event,
                serde_json::json!({"window_created":event=="bundled_window_ready","bundled_document_finished":finished && valid}),
            );
        }
    }
    fn begin_shutdown(&self) -> Option<bool> {
        let mut state = self.state.lock().unwrap_or_else(|e| {
            let mut s = e.into_inner();
            s.failed = true;
            s
        });
        if state.shutdown_started {
            return None;
        }
        state.shutdown_started = true;
        Some(state.ready_emitted && !state.failed)
    }
}

#[cfg(feature = "development-graph-startup-proof")]
pub(crate) fn start<R: tauri::Runtime>(
    app: &tauri::App<R>,
) -> Result<(JobCoordinator, Arc<Observation>)> {
    let attempt = (|| {
        require(
            cfg!(all(target_os = "macos", target_arch = "aarch64")),
            "unsupported_host",
        )?;
        let local = app
            .path()
            .app_local_data_dir()
            .map_err(|_| Refusal("app_data_path"))?;
        let resources = app
            .path()
            .resource_dir()
            .map_err(|_| Refusal("resource_path"))?;
        let executable =
            tauri::utils::platform::current_exe().map_err(|_| Refusal("executable_path"))?;
        guard(&app.config().identifier, &local, &resources, &executable)?;
        reserve_fresh_app_data(&local)?;
        let mut workspace = Workspace::open(local.join("workspaces/default"))
            .map_err(|_| Refusal("workspace_initialization"))?;
        workspace.attach_runtime(workbench_core::engines::Runtime {
            root: resources.join("engines"),
        });
        let coordinator = JobCoordinator::start_with_development_app_resources(
            workspace,
            2,
            &resources,
            &CancellationToken::default(),
        )
        .map_err(|_| Refusal("resource_constructor"))?;
        if !read_empty(&coordinator) {
            coordinator
                .shutdown()
                .map_err(|_| Refusal("startup_shutdown"))?;
            return Err(Refusal("empty_ready_catalogue"));
        }
        emit(
            "constructor_ready",
            serde_json::json!({"resource_location":"app_bundle","availability":"ready",
            "workspace_revision":0,"graph_job_count":0}),
        );
        Ok((coordinator, Arc::new(Observation::new())))
    })();
    if let Err(error) = &attempt {
        emit("startup_refused", serde_json::json!({"stage":error.0}));
    }
    attempt
}

#[cfg(feature = "development-graph-startup-proof")]
pub(crate) fn shutdown(coordinator: &JobCoordinator, observation: &Observation) {
    let Some(ready) = observation.begin_shutdown() else {
        return;
    };
    let unchanged = read_empty(coordinator);
    let joined = coordinator.shutdown().is_ok();
    emit(
        "shutdown",
        serde_json::json!({"joined":joined,"unchanged_empty_graph_catalogue":unchanged,
        "bundled_window_ready":ready,"passed":joined && unchanged && ready}),
    );
}

#[cfg(test)]
mod tests;
