use crate::Host;
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex,
    },
    time::Duration,
};

static NEXT: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<HashMap<u64, mpsc::SyncSender<Vec<PathBuf>>>>> = Mutex::new(None);

fn pending() -> std::sync::MutexGuard<'static, Option<HashMap<u64, mpsc::SyncSender<Vec<PathBuf>>>>>
{
    let mut guard = PENDING.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

fn ask(kind: &str, extensions: &[&str], file_name: Option<&str>) -> Vec<PathBuf> {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::sync_channel(1);
    pending().as_mut().unwrap().insert(id, tx);
    let root = Host::current().root().to_path_buf();
    let _ = Host::current().emit(
        "web-dialog",
        json!({
            "id": id,
            "kind": kind,
            "extensions": extensions,
            "fileName": file_name,
            "dataDir": root,
        }),
    );
    match rx.recv_timeout(Duration::from_secs(600)) {
        Ok(paths) => paths,
        Err(_) => {
            pending().as_mut().unwrap().remove(&id);
            Vec::new()
        }
    }
}

pub fn resolve(id: u64, paths: Vec<PathBuf>) -> Result<(), String> {
    let tx = pending()
        .as_mut()
        .unwrap()
        .remove(&id)
        .ok_or("对话框已关闭")?;
    tx.send(paths).map_err(|_| "对话框已关闭".to_string())
}

pub fn pick_file(extensions: &[&str]) -> Option<PathBuf> {
    ask("file", extensions, None).into_iter().next()
}

pub fn pick_files(extensions: &[&str]) -> Vec<PathBuf> {
    ask("files", extensions, None)
}

pub fn pick_folder() -> Option<PathBuf> {
    ask("folder", &[], None).into_iter().next()
}

pub fn save_file(name: &str) -> Option<PathBuf> {
    let path = ask("save", &[], Some(name)).into_iter().next()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn folder_dialog_returns_the_path_the_browser_chose() {
        let dir = tempfile::tempdir().unwrap();
        let host = Host::new(dir.path().to_path_buf()).install();
        let mut rx = host.subscribe();
        let worker = thread::spawn(|| pick_folder());
        let message = rx.blocking_recv().unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();
        let id = value["payload"]["id"].as_u64().unwrap();
        resolve(id, vec![dir.path().join("models")]).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), dir.path().join("models"));
    }
}
