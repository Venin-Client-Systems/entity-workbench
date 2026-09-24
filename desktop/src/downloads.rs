//! Only exports created by the bundled interface may enter the native download path.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{webview::DownloadEvent, Emitter, Manager, Url, Webview};

#[derive(Default)]
pub struct Downloads(Mutex<PendingDownloads>);
const COMPLETION_LIMIT: Duration = Duration::from_secs(120);
#[derive(Default)]
struct PendingDownloads(HashMap<String, (String, PathBuf, Instant)>);
impl PendingDownloads {
    fn expire(&mut self, now: Instant) {
        self.0.retain(|_, (_, _, started)| {
            now.saturating_duration_since(*started) < COMPLETION_LIMIT
        });
    }
    fn start(&mut self, url: &str, name: String, destination: PathBuf, now: Instant) -> bool {
        self.expire(now);
        if self.0.len() >= 8 || self.0.contains_key(url) {
            return false;
        }
        self.0.insert(url.to_owned(), (name, destination, now));
        true
    }
    fn finish(&mut self, url: &str, now: Instant) -> Option<(String, PathBuf)> {
        self.expire(now);
        self.0.remove(url).map(|(name, path, _)| (name, path))
    }
}

fn local_blob(url: &Url) -> bool {
    if url.scheme() != "blob" {
        return false;
    }
    let Ok(origin) = Url::parse(url.path()) else {
        return false;
    };
    let bundled = (origin.scheme() == "tauri" && origin.host_str() == Some("localhost"))
        || (origin.scheme() == "http" && origin.host_str() == Some("tauri.localhost"));
    let development = cfg!(debug_assertions)
        && origin.scheme() == "http"
        && matches!(origin.host_str(), Some("localhost" | "127.0.0.1"))
        && origin.port() == Some(1420);
    origin.username().is_empty()
        && origin.password().is_none()
        && origin.query().is_none()
        && origin.fragment().is_none()
        && (bundled && origin.port().is_none() || development)
}
fn export_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    // Include native collision suffixes, e.g. "transactions (1).json".
    if name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_. ()".contains(&b))
    {
        return None;
    }
    if (name.starts_with("assessment-") && name.ends_with(".html"))
        || (name.starts_with("transactions") && name.ends_with(".json"))
    {
        Some(name.to_owned())
    } else {
        None
    }
}
impl Downloads {
    pub fn handle(&self, webview: Webview, event: DownloadEvent<'_>) -> bool {
        let Ok(mut pending) = self.0.lock() else {
            return false;
        };
        match event {
            DownloadEvent::Requested { url, destination } => {
                let name = export_name(destination);
                let allowed = local_blob(&url)
                    && name.is_some()
                    && webview.app_handle().path().download_dir().ok().as_deref()
                        == destination.parent();
                let allowed = allowed
                    && pending.start(
                        url.as_str(),
                        name.unwrap(),
                        destination.clone(),
                        Instant::now(),
                    );
                if !allowed {
                    let _ = webview.emit(
                        "workbench-download",
                        serde_json::json!({"url":url.as_str(),"success":false,"name":null}),
                    );
                }
                allowed
            }
            DownloadEvent::Finished { url, success, path } => {
                if let Some((name, expected_path)) = pending.finish(url.as_str(), Instant::now()) {
                    let success = success && path.as_ref().is_none_or(|p| p == &expected_path);
                    let _ = webview.emit(
                        "workbench-download",
                        serde_json::json!({"url":url.as_str(),"success":success,"name":name}),
                    );
                }
                true
            }
            _ => false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_callbacks_expire_and_late_completion_cannot_claim_a_new_export() {
        let mut pending = PendingDownloads::default();
        let now = Instant::now();
        for n in 0..8 {
            assert!(pending.start(
                &format!("blob:{n}"),
                "transactions.json".into(),
                PathBuf::from("transactions.json"),
                now
            ));
        }
        assert!(!pending.start(
            "blob:extra",
            "transactions.json".into(),
            PathBuf::from("transactions.json"),
            now
        ));
        let later = now + COMPLETION_LIMIT;
        assert!(pending.start(
            "blob:new",
            "transactions (1).json".into(),
            PathBuf::from("transactions (1).json"),
            later
        ));
        assert!(pending.finish("blob:0", later).is_none());
        assert_eq!(
            pending.finish("blob:new", later).unwrap().0,
            "transactions (1).json"
        );
        assert!(pending.finish("blob:new", later).is_none());
    }
    #[test]
    fn network_file_foreign_and_credentialled_blob_origins_are_refused() {
        for value in [
            "https://example.com/report.html",
            "file:///tmp/report.html",
            "blob:https://example.com/x",
            "blob:tauri://localhost.evil/x",
            "blob:tauri://user@localhost/x",
            "blob:tauri://localhost:12/x",
            "blob:null/x",
        ] {
            assert!(!local_blob(&Url::parse(value).unwrap()), "{value}");
        }
        for value in [
            "blob:tauri://localhost/1234",
            "blob:http://tauri.localhost/1234",
        ] {
            assert!(local_blob(&Url::parse(value).unwrap()));
        }
    }
    #[test]
    fn only_known_inert_export_types_have_native_names() {
        assert_eq!(
            export_name(Path::new("assessment-1234 (1).html")).as_deref(),
            Some("assessment-1234 (1).html")
        );
        assert_eq!(
            export_name(Path::new("transactions (2).json")).as_deref(),
            Some("transactions (2).json")
        );
        for name in [
            "payload.exe",
            "assessment-1.svg",
            "transactions.js",
            "assessment-\u{202e}1.html",
            "transactions\n.json",
        ] {
            assert!(export_name(Path::new(name)).is_none());
        }
    }
}
