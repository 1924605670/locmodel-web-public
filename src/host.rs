use serde::Serialize;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tokio::sync::broadcast;

static CURRENT: OnceLock<Mutex<Option<Host>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Host>> {
    CURRENT.get_or_init(|| Mutex::new(None))
}

/// 进程内事件总线。桌面版原来用 Tauri 窗口转发，网页版用同一套事件名推给浏览器。
#[derive(Clone)]
pub struct Host {
    tx: broadcast::Sender<String>,
    root: PathBuf,
}

impl Host {
    pub fn new(root: PathBuf) -> Self {
        let (tx, _) = broadcast::channel(512);
        Self { tx, root }
    }

    pub fn install(self) -> Self {
        *slot().lock().unwrap() = Some(self.clone());
        self
    }

    pub fn current() -> Self {
        slot()
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Self::new(PathBuf::from(".")))
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn picture_dir(&self) -> PathBuf {
        self.root.join("images")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn emit<S: Serialize>(&self, event: &str, payload: S) -> Result<(), String> {
        let message = serde_json::to_string(&json!({"event": event, "payload": payload}))
            .map_err(|e| e.to_string())?;
        let _ = self.tx.send(message);
        Ok(())
    }

    pub fn exit(&self, code: i32) -> ! {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_keeps_event_name() {
        let host = Host::new(PathBuf::from("."));
        let mut rx = host.subscribe();
        host.emit("server-log", "ready").unwrap();
        let message = rx.blocking_recv().unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["event"], "server-log");
        assert_eq!(value["payload"], "ready");
    }
}
