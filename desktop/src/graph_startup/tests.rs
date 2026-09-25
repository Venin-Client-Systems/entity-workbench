use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use workbench_core::store::Workspace;

const ID: &str = "org.entityworkbench.graphstartupproof.465be89b-cf9c-45b2-9064-19d50ddc839f";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    local: PathBuf,
    resources: PathBuf,
    executable: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "ew-startup-source-{}-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let contents = root.join("Synthetic.app/Contents");
        let executable = contents.join("MacOS/synthetic");
        let resources = contents.join("Resources");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::create_dir(&resources).unwrap();
        fs::write(&executable, b"inert source-test file; never executed").unwrap();
        fs::create_dir(root.join("Support")).unwrap();
        Self {
            local: root.join("Support").join(ID),
            root,
            resources,
            executable,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn identifier_accepts_only_canonical_proof_namespace_uuid() {
    assert!(identifier(ID));
    for value in [
        "org.entityworkbench.desktop".into(),
        PREFIX.into(),
        ID.to_uppercase(),
        ID.replace('-', ""),
        format!("{ID}.extra"),
        ID.replace("465be89b", "465be89g"),
        ID.replace("465be89b", "{465be89b"),
    ] {
        assert!(!identifier(&value), "{value}");
    }
}

#[cfg(unix)]
#[test]
fn fresh_bundle_guard_reads_only_and_refuses_existing_case_without_mutation() {
    let f = Fixture::new();
    guard(ID, &f.local, &f.resources, &f.executable).unwrap();
    assert!(!f.local.exists());
    fs::create_dir(&f.local).unwrap();
    let marker = f.local.join("retained");
    fs::write(&marker, b"preserve").unwrap();
    assert!(guard(ID, &f.local, &f.resources, &f.executable).is_err());
    assert_eq!(fs::read(marker).unwrap(), b"preserve");
    assert!(!f.local.join("workspaces").exists());
}

#[cfg(unix)]
#[test]
fn app_data_reservation_is_exclusive_and_keeps_a_racing_existing_directory() {
    let f = Fixture::new();
    guard(ID, &f.local, &f.resources, &f.executable).unwrap();
    reserve_fresh_app_data(&f.local).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&f.local).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    fs::write(f.local.join("retained"), b"preserve").unwrap();
    assert!(reserve_fresh_app_data(&f.local).is_err());
    assert_eq!(fs::read(f.local.join("retained")).unwrap(), b"preserve");
    assert!(!f.local.join("workspaces").exists());
}

#[cfg(windows)]
#[test]
fn windows_absolute_bundle_paths_are_explicitly_refused_without_writes() {
    let f = Fixture::new();
    assert!(f.local.is_absolute());
    assert_eq!(
        guard(ID, &f.local, &f.resources, &f.executable)
            .unwrap_err()
            .0,
        "path_shape"
    );
    assert!(!f.local.exists());
}

#[test]
fn wrong_identifier_cargo_output_or_resource_location_never_creates_workspace() {
    let f = Fixture::new();
    for (id, local, resources, exe) in [
        (
            "org.entityworkbench.desktop",
            f.local.clone(),
            f.resources.clone(),
            f.executable.clone(),
        ),
        (
            ID,
            f.root.join("Support/other"),
            f.resources.clone(),
            f.executable.clone(),
        ),
        (
            ID,
            f.local.clone(),
            f.root.join("Support"),
            f.executable.clone(),
        ),
    ] {
        assert!(guard(id, &local, &resources, &exe).is_err());
    }
    let target = f.root.join("target/release");
    fs::create_dir_all(&target).unwrap();
    let exe = target.join("synthetic");
    fs::write(&exe, b"inert").unwrap();
    assert!(guard(ID, &f.local, &target, &exe).is_err());
    assert!(guard(ID, Path::new("relative"), &f.resources, &f.executable).is_err());
    assert!(!f.local.exists());
}

#[cfg(unix)]
#[test]
fn linked_and_dangling_app_data_or_bundle_entries_are_refused_before_creation() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    symlink(f.root.join("absent-target"), &f.local).unwrap();
    assert!(guard(ID, &f.local, &f.resources, &f.executable).is_err());
    fs::remove_file(&f.local).unwrap();
    let alias = f.root.join("alias");
    symlink(&f.root, &alias).unwrap();
    assert!(guard(
        ID,
        &alias.join("Support").join(ID),
        &f.resources,
        &f.executable
    )
    .is_err());
    let previous = f.resources.with_file_name("RetainedResources");
    fs::rename(&f.resources, &previous).unwrap();
    symlink(&previous, &f.resources).unwrap();
    assert!(guard(ID, &f.local, &f.resources, &f.executable).is_err());
    assert!(!f.local.exists());
}

fn page() -> GraphJobPage {
    GraphJobPage {
        schema_version: 1,
        workspace_revision: 0,
        availability: GraphAvailability::Ready,
        total_count: 0,
        rows: vec![],
        next_cursor: None,
    }
}
#[test]
fn unavailable_or_nonempty_catalogue_is_never_a_successful_startup() {
    assert!(empty_ready(&page()));
    let mut p = page();
    p.availability = GraphAvailability::RuntimeUnavailable;
    assert!(!empty_ready(&p));
    p = page();
    p.availability = GraphAvailability::SyntheticFixture;
    assert!(!empty_ready(&p));
    p = page();
    p.availability = GraphAvailability::RecoveryRequired;
    assert!(!empty_ready(&p));
    p = page();
    p.total_count = 1;
    assert!(!empty_ready(&p));
    p = page();
    p.workspace_revision = 1;
    assert!(!empty_ready(&p));
    p = page();
    p.schema_version = 2;
    assert!(!empty_ready(&p));
    p = page();
    p.next_cursor = Some("unexpected".into());
    assert!(!empty_ready(&p));
}

#[test]
fn page_and_window_observations_must_both_arrive_once_and_shutdown_is_one_shot() {
    for first in [true, false] {
        let p = Observation::new();
        assert_eq!(p.update(first, !first, true), None);
        assert_eq!(p.update(!first, first, true), Some("bundled_window_ready"));
        assert_eq!(p.update(true, true, true), None);
        assert_eq!(p.begin_shutdown(), Some(true));
        assert_eq!(p.begin_shutdown(), None);
        assert_eq!(p.update(true, true, false), None);
    }
    let p = Observation::new();
    p.window_created("main");
    assert_eq!(p.begin_shutdown(), Some(false));
}

#[test]
fn wrong_page_or_window_stays_failed_even_after_a_valid_observation() {
    for (label, url) in [
        ("other", "tauri://localhost/"),
        ("main", "http://localhost:1420/"),
        ("main", "https://example.invalid/"),
        ("main", "tauri://localhost/other"),
        ("main", "tauri://localhost/?supplied=1"),
    ] {
        let p = Observation::new();
        p.window_created("main");
        p.page_load(label, url, true);
        p.page_load("main", "tauri://localhost/", true);
        assert_eq!(p.begin_shutdown(), Some(false));
    }
    let p = Observation::new();
    p.window_created("main");
    p.page_load("main", "tauri://localhost/", false);
    assert_eq!(p.begin_shutdown(), Some(false));
}

#[test]
fn locked_default_document_maps_to_the_mac_protocol_origin() {
    assert_eq!(
        tauri::utils::config::WebviewUrl::default(),
        tauri::utils::config::WebviewUrl::App("index.html".into())
    );
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    assert_eq!(config["build"]["frontendDist"], "../ui/dist");
    assert!(config["app"]["windows"][0].get("url").is_none());
    // Tauri 2.11.6 manager/webview.rs keeps the base protocol URL for index.html.
    let url = tauri::Url::parse("tauri://localhost").unwrap();
    let p = Observation::new();
    p.window_created("main");
    p.page_load("main", url.as_str(), true);
    assert_eq!(p.begin_shutdown(), Some(true));
}

#[test]
fn locked_resource_merge_requires_removing_the_inherited_map_key() {
    let f = Fixture::new();
    fs::write(
        f.root.join("tauri.conf.json"),
        include_str!("../../tauri.macos.conf.json"),
    )
    .unwrap();
    // Only inert configuration text is read; no resource path is opened or staged.
    for remove_inherited in [false, true] {
        let mut resources = serde_json::json!({"../isolated-proof/python/":"engines/python/"});
        if remove_inherited {
            resources["../runtime/staged/engines/"] = serde_json::Value::Null;
        }
        let patch = serde_json::json!({"identifier":ID,"bundle":{"resources":resources}});
        fs::write(
            f.root.join("tauri.macos.conf.json"),
            serde_json::to_vec(&patch).unwrap(),
        )
        .unwrap();
        let (merged, _) =
            tauri::utils::config::parse::read_from(tauri::utils::platform::Target::MacOS, &f.root)
                .unwrap();
        let map = merged["bundle"]["resources"].as_object().unwrap();
        assert_eq!(map.len(), if remove_inherited { 1 } else { 2 });
        assert_eq!(map["../isolated-proof/python/"], "engines/python/");
        assert_eq!(
            map.contains_key("../runtime/staged/engines/"),
            !remove_inherited
        );
        let _: tauri::utils::config::Config = serde_json::from_value(merged).unwrap();
    }
}

#[test]
fn ordinary_coordinator_remains_unavailable_and_empty_canonical_initialization_is_exact() {
    let f = Fixture::new();
    let workspace = Workspace::open(f.root.join("separate-source-fixture")).unwrap();
    assert_eq!(workspace.revision().unwrap(), 0);
    let c = JobCoordinator::start(workspace, 1).unwrap();
    assert!(!read_empty(&c));
    let summary = c.dispatch_summary(Command::View {}).unwrap();
    assert_eq!(summary["workspace"]["revision"], 0);
    let graph = c
        .dispatch_summary(Command::PageGraphJobs {
            request: GraphJobPageRequest {
                page_size: 1,
                cursor: None,
            },
            expected_revision: Some(0),
        })
        .unwrap();
    assert_eq!(graph["total_count"], 0);
    assert_eq!(graph["availability"], "runtime_unavailable");
    c.shutdown().unwrap();
}
