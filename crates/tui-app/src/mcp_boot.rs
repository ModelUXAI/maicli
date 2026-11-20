//! Menghubungkan server MCP yang dikonfigurasi saat boot aplikasi.
//!
//! Kegagalan satu server **tidak** menggagalkan aplikasi: server yang gagal
//! dicatat sebagai catatan yang ditampilkan ke pengguna, tool server itu tidak
//! didaftarkan, dan sesi berjalan dengan tool yang tersisa.

use std::sync::Arc;
use std::time::Duration;

use tui_config::McpConfig;
use tui_mcp::{ServerSpec, connect_http_with, connect_sse_with, connect_stdio};
use tui_tools::Tool;

/// Hasil koneksi MCP saat boot.
#[derive(Default)]
pub struct McpBoot {
    /// Tool MCP yang siap didaftarkan ke registry sesi.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Catatan status untuk ditampilkan (sukses/gagal/nonaktif).
    pub notes: Vec<String>,
    /// Penerima notifikasi gabungan (server, notifikasi); mode meneruskannya
    /// ke timeline sebagai `[mcp:<server>] …`. `None` bila tak ada server.
    pub notif_rx: Option<tokio::sync::mpsc::UnboundedReceiver<(String, tui_mcp::McpNotification)>>,
}

/// Penerus notifikasi MCP → event agen `[mcp:<server>] <metode> <params>`.
///
/// Anti-deadlock: task **berhenti sendiri** setelah `BATAS_IDLE` detik tanpa
/// notifikasi — tidak pernah menahan tx event selamanya (printer yang menunggu
/// semua tx drop tidak akan deadlock; notifikasi hanya relevan saat turn
/// berjalan).
const BATAS_IDLE: u64 = 2;
const MAKS_IDLE: u32 = 3;

pub fn spawn_event_forward(
    mut notif_rx: tokio::sync::mpsc::UnboundedReceiver<(String, tui_mcp::McpNotification)>,
    events: tokio::sync::mpsc::UnboundedSender<tui_wire::AgentEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = 0u32;
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(BATAS_IDLE), notif_rx.recv())
                .await
            {
                Ok(Some((server, notifikasi))) => {
                    idle = 0;
                    let detail = serde_json::to_string(&notifikasi.params).unwrap_or_default();
                    let _ = events.send(tui_wire::AgentEvent::Notice {
                        text: format!("[mcp:{server}] {} {detail}", notifikasi.method),
                    });
                }
                Ok(None) => break, // kanal tutup (semua server drop)
                Err(_) => {
                    idle += 1;
                    if idle >= MAKS_IDLE {
                        break;
                    }
                }
            }
        }
    })
}

/// Hubungkan semua server MCP yang `enabled`.
pub async fn connect_all(config: &McpConfig) -> McpBoot {
    let mut boot = McpBoot::default();
    let (notif_tx, notif_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ada_server = false;
    for (nama, server) in &config.servers {
        if !server.enabled {
            boot.notes
                .push(format!("server '{nama}' dinonaktifkan di config"));
            continue;
        }
        let detik = server.timeout_secs.unwrap_or(config.timeout_secs).max(1);
        let batas = Duration::from_secs(detik);
        if let Some(url) = &server.url {
            let transport = server.transport.as_deref().unwrap_or("http");
            // Header kustom; nilai `env:NAMA` di-resolve dari environment.
            let mut headers: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (kunci, nilai) in &server.headers {
                headers.insert(kunci.clone(), tui_mcp::resolve_header(nilai));
            }
            let headers = Arc::new(headers);
            let hasil = match transport {
                "sse" => connect_sse_with(url.clone(), nama.clone(), batas, headers).await,
                _ => connect_http_with(url.clone(), nama.clone(), batas, headers).await,
            };
            match hasil {
                Ok(koneksi) => {
                    let jumlah = koneksi.tools().len();
                    // Langganan notifikasi HTTP/SSE (kanal kosong di tahap ini)
                    // supaya pemantau klien tetap terpasang.
                    let sub = koneksi.subscribe().await;
                    drop(sub);
                    boot.tools.extend(koneksi.into_tools());
                    boot.notes
                        .push(format!("server '{nama}' ({transport}): {jumlah} tool"));
                    ada_server = true;
                }
                Err(err) => boot.notes.push(format!(
                    "server '{nama}' tidak terhubung: {}: {}",
                    err.code, err.message
                )),
            }
            continue;
        }
        let mut spec = ServerSpec::new(nama.clone(), server.command.clone()).timeout(batas);
        for arg in &server.args {
            spec = spec.arg(arg.clone());
        }
        for (kunci, nilai) in &server.env {
            spec = spec.env(kunci.clone(), nilai.clone());
        }
        match connect_stdio(spec).await {
            Ok(koneksi) => {
                let jumlah = koneksi.tools().len();
                // Langgan notifikasi stdio; diteruskan ke kanal gabungan.
                let mut sub = koneksi.subscribe().await;
                let tx = notif_tx.clone();
                let server = nama.clone();
                tokio::spawn(async move {
                    while let Some(n) = sub.recv().await {
                        let _ = tx.send((server.clone(), n));
                    }
                });
                boot.tools.extend(koneksi.into_tools());
                boot.notes.push(format!("server '{nama}': {jumlah} tool"));
                ada_server = true;
            }
            Err(err) => boot.notes.push(format!(
                "server '{nama}' tidak terhubung: {}: {}",
                err.code, err.message
            )),
        }
    }
    if ada_server {
        boot.notif_rx = Some(notif_rx);
    } else {
        drop(notif_tx);
    }
    boot
}

#[cfg(test)]
mod tests {
    // Test gagal keras bila asumsinya tidak berlaku; itu tujuannya.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use tui_config::McpServerConfig;

    fn server(command: &str) -> McpServerConfig {
        McpServerConfig {
            command: command.to_string(),
            enabled: true,
            ..McpServerConfig::default()
        }
    }

    #[tokio::test]
    async fn config_kosong_tanpa_catatan() {
        let boot = connect_all(&McpConfig::default()).await;
        assert!(boot.tools.is_empty());
        assert!(boot.notes.is_empty());
    }

    #[tokio::test]
    async fn server_nonaktif_dicatat_tanpa_spawn() {
        let mut config = McpConfig::default();
        let mati = McpServerConfig {
            command: "perintah-tidak-ada-xyz".to_string(),
            enabled: false,
            ..McpServerConfig::default()
        };
        config.servers.insert("mati".to_string(), mati);
        let boot = connect_all(&config).await;
        assert!(boot.tools.is_empty());
        assert_eq!(boot.notes.len(), 1);
        assert!(boot.notes[0].contains("dinonaktifkan"), "{}", boot.notes[0]);
    }

    #[tokio::test]
    async fn server_gagal_spawn_dicatat_tanpa_menjatuhkan_boot() {
        let mut config = McpConfig {
            timeout_secs: 2,
            ..McpConfig::default()
        };
        config
            .servers
            .insert("rusak".to_string(), server("perintah-tidak-ada-xyz"));
        let boot = connect_all(&config).await;
        assert!(boot.tools.is_empty());
        assert_eq!(boot.notes.len(), 1);
        assert!(
            boot.notes[0].contains("tidak terhubung"),
            "{}",
            boot.notes[0]
        );
    }
}
