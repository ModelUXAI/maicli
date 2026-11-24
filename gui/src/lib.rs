//! Shell desktop maicli (Tauri 2) di atas inti Rust yang sudah ada.
//!
//! GUI tidak menulis ulang logika agen: ia menjalankan `maicli --mode rpc`
//! sebagai child process dan meneruskan frame JSONL ke frontend TypeScript.

use std::process::{Child, Command, Stdio};
use tauri::{Emitter, Manager};

pub mod commands;
use std::sync::Mutex;

/// State proses RPC yang melayani frontend.
pub struct RpcState {
    child: Mutex<Option<Child>>,
}

impl Default for RpcState {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcState {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
        }
    }

    pub fn start(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let mut guard = self.child.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Ok(());
        }
        let exe = std::env::var("MAICLI_BIN").unwrap_or_else(|_| "maicli".to_string());
        let mut child = Command::new(exe)
            .arg("--mode")
            .arg("rpc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("gagal menjalankan maicli RPC: {e}"))?;
        // Baca stdout child baris per baris dan teruskan ke frontend sebagai event.
        if let Some(stdout) = child.stdout.take() {
            let handle = app.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                {
                    let _ = handle.emit("rpc-line", line);
                }
            });
        }
        *guard = Some(child);
        Ok(())
    }
}

impl Drop for RpcState {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Jalankan aplikasi. `rpc_send` dipanggil frontend lewat Tauri event bridge
/// yang didaftarkan di `setup`, menghindari duplikasi makro `tauri::command`.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .manage(RpcState::new())
        .invoke_handler(tauri::generate_handler![commands::rpc_send])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or("jendela main tidak ada")?;
            window.set_title("maicli — ModeluXAI")?;
            window.show()?;
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_baru_tanpa_child() {
        let state = RpcState::new();
        assert!(state.child.lock().is_ok_and(|g| g.is_none()));
    }
}
