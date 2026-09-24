#[cfg(windows)]
fn main() {
    use std::{collections::BTreeMap, path::Path};
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 || args[1] != "--report" {
        eprintln!("Expected --report REPORT STAGED_RUNTIME");
        std::process::exit(2);
    }
    let mut tests = BTreeMap::new();
    let outcome =
        workbench_windows_worker::java::development_probe(Path::new(&args[3]), &mut tests);
    let report = serde_json::json!({"schema_version":1,"scope":"native development Java parser and Lucene recipes; application adapters disabled",
        "complete_release":false,"passed":outcome.is_ok(),"failure":outcome.as_ref().err().map(ToString::to_string),"tests":tests,
        "unverified":["Windows 11 installed artifact","selected release runtime","application integration","independent security approval"]});
    let saved = serde_json::to_vec_pretty(&report)
        .ok()
        .is_some_and(|bytes| std::fs::write(&args[2], bytes).is_ok());
    if !saved || outcome.is_err() {
        eprintln!("Native Java development probe failed; retain evidence.");
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows AppContainer is required; no unconfined fallback.");
    std::process::exit(2);
}
