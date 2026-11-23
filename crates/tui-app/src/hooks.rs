//! External hooks dari config (`[hooks]`).
//!
//! Hook dijalankan sebagai proses shell dengan konteks JSON di stdin. Output
//! dibatasi dan tidak pernah dianggap sebagai instruksi sistem. Eksekusi
//! manual tersedia lewat `/hook-test <event>`.

use std::time::Duration;

use tokio::io::AsyncWriteExt;

/// Hook `before_tool` dari daftar perintah.
fn hook_sebelum(perintah: Vec<String>) -> tui_core::BeforeToolHook {
    std::sync::Arc::new(move |nama, argumen| {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            for perintah in &perintah {
                let perintah = perintah.clone();
                let konteks = serde_json::json!({
                    "event": "before_tool",
                    "tool": nama,
                    "args": argumen.chars().take(2000).collect::<String>(),
                })
                .to_string();
                handle.spawn(async move {
                    let _ = jalankan(&perintah, &konteks).await;
                });
            }
        }
    })
}

/// Hook `after_tool` dari daftar perintah.
fn hook_sesudah(perintah: Vec<String>) -> tui_core::AfterToolHook {
    std::sync::Arc::new(move |nama, is_error, cuplikan| {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            for perintah in &perintah {
                let perintah = perintah.clone();
                let konteks = serde_json::json!({
                    "event": "after_tool",
                    "tool": nama,
                    "is_error": is_error,
                    "preview": cuplikan.chars().take(2000).collect::<String>(),
                })
                .to_string();
                handle.spawn(async move {
                    let _ = jalankan(&perintah, &konteks).await;
                });
            }
        }
    })
}

/// Suntikkan instruksi untuk kata kunci yang muncul di pesan pengguna.
pub fn hook_keywords(
    peta: std::collections::BTreeMap<String, String>,
) -> std::sync::Arc<tui_core::ContextSyncFn> {
    std::sync::Arc::new(move |pesan: &[tui_wire::Message]| {
        let teks: String = pesan.iter().rev().find_map(|m| match m {
            tui_wire::Message::User { content } => Some(content.clone()),
            _ => None,
        })?;
        let mut instruksi = Vec::new();
        for (kata, isi) in &peta {
            if !kata.trim().is_empty()
                && teks
                    .to_ascii_lowercase()
                    .contains(&kata.to_ascii_lowercase())
            {
                instruksi.push(isi.trim().to_string());
            }
        }
        if instruksi.is_empty() {
            return None;
        }
        let mut baru = pesan.to_vec();
        baru.push(tui_wire::Message::system(format!(
            "Instruksi dari magic keyword:\n{}",
            instruksi.join("\n")
        )));
        Some(baru)
    })
}

/// Pasang hook eksternal ke `AgentSession` (mode teks/TUI/RPC).
pub fn pasang(session: &mut tui_core::AgentSession, config: &tui_config::HooksConfig) {
    if !config.before_tool.is_empty() {
        session.set_before_tool(hook_sebelum(config.before_tool.clone()));
    }
    if !config.after_tool.is_empty() {
        session.set_after_tool(hook_sesudah(config.after_tool.clone()));
    }
    if !config.magic_keywords.is_empty() {
        session.set_context_sync(hook_keywords(config.magic_keywords.clone()));
    }
    if !config.ttsr_rules.is_empty() {
        session.set_ttsr_rules(config.ttsr_rules.clone());
    }
}

/// Pasang hook eksternal ke `tui_sdk::Session` (mode serve).
pub fn pasang_sdk(session: &mut tui_sdk::Session, config: &tui_config::HooksConfig) {
    if !config.before_tool.is_empty() {
        session.set_before_tool(hook_sebelum(config.before_tool.clone()));
    }
    if !config.after_tool.is_empty() {
        session.set_after_tool(hook_sesudah(config.after_tool.clone()));
    }
    if !config.magic_keywords.is_empty() {
        session.set_context_sync(hook_keywords(config.magic_keywords.clone()));
    }
}

/// Jalankan satu hook; mengembalikan stdout+stderr yang sudah dipotong.
pub async fn jalankan(perintah: &str, konteks: &str) -> Result<String, String> {
    if perintah.trim().is_empty() {
        return Err("perintah hook kosong".to_string());
    }
    let mut command = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", perintah]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", perintah]);
        c
    };
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("gagal menjalankan hook: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(konteks.as_bytes()).await;
    }
    let hasil = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await;
    match hasil {
        Err(_) => Err("hook melewati batas 5 dtk".to_string()),
        Ok(Err(err)) => Err(format!("hook gagal: {err}")),
        Ok(Ok(out)) => {
            let mut teks = String::from_utf8_lossy(&out.stdout).into_owned();
            teks.push_str(&String::from_utf8_lossy(&out.stderr));
            let potong: String = teks.chars().take(2048).collect();
            if out.status.success() {
                Ok(potong)
            } else {
                Err(format!("hook keluar dengan {}: {potong}", out.status))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[tokio::test]
    async fn hook_menerima_konteks_stdin() {
        if cfg!(windows) {
            return;
        }
        let hasil = jalankan("cat", "{\"event\":\"uji\"}").await.unwrap();
        assert!(hasil.contains("uji"), "{hasil}");
    }

    #[tokio::test]
    async fn hook_kosong_ditolak() {
        assert!(jalankan("   ", "{}").await.is_err());
    }
    #[test]
    fn keyword_menyuntikkan_instruksi() {
        let peta =
            std::collections::BTreeMap::from([("urgent".to_string(), "jawab singkat".to_string())]);
        let hook = hook_keywords(peta);
        let pesan = vec![tui_wire::Message::user("ini URGENT sekali")];
        let hasil = hook(&pesan).expect("harus menyuntikkan");
        assert!(
            hasil
                .iter()
                .any(|m| matches!(m, tui_wire::Message::System { .. }))
        );
        let biasa = vec![tui_wire::Message::user("biasa")];
        assert!(hook(&biasa).is_none());
    }
}
