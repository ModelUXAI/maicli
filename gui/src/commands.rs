//! Command Tauri yang menjembatani frontend ke proses RPC maicli.

use crate::RpcState;
use std::io::Write;

/// Kirim satu baris JSON ke child `maicli --mode rpc`.
#[tauri::command]
pub fn rpc_send(
    line: String,
    state: tauri::State<'_, RpcState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.start(&app)?;
    let mut guard = state.child.lock().map_err(|e| e.to_string())?;
    let child = guard.as_mut().ok_or("proses RPC tidak aktif")?;
    let stdin = child.stdin.as_mut().ok_or("stdin RPC tidak tersedia")?;
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("gagal menulis ke RPC: {e}"))
}
