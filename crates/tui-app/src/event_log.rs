//! Perekam transkrip event agen ke berkas JSONL (RENCANA C-12).
//!
//! Dengan `--event-log <FILE>`, setiap `AgentEvent` yang mengalir ke UI juga
//! ditulis sebagai satu baris JSON ke berkas. Hasilnya bisa dibandingkan dengan
//! transkrip acuan memakai `maicli --parity-expected/--parity-actual`.
//!
//! Pencatatan bersifat best-effort: kegagalan menulis log tidak boleh
//! menggagalkan sesi pengguna (dicatat di `tracing`).

use std::path::PathBuf;

use tui_core::agent::event_channel;
use tui_wire::AgentEvent;

/// Kanal event; bila `log_path` diisi, event juga direkam ke berkas JSONL.
pub fn channel(
    log_path: Option<PathBuf>,
) -> (
    tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    Option<tokio::sync::oneshot::Receiver<()>>,
) {
    let Some(log_path) = log_path else {
        let (tx, rx) = event_channel();
        return (tx, rx, None);
    };
    let (inner_tx, mut inner_rx) = event_channel();
    let (outer_tx, outer_rx) = event_channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if let Some(parent) = log_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            Ok(file) => Some(tokio::io::BufWriter::new(file)),
            Err(err) => {
                tracing::warn!(target: "maicli::event_log", "log event tidak bisa dibuka: {err}");
                None
            }
        };
        use tokio::io::AsyncWriteExt;
        while let Some(event) = inner_rx.recv().await {
            if let Some(writer) = file.as_mut() {
                match serde_json::to_string(&event) {
                    Ok(line) => {
                        let written = async {
                            writer.write_all(line.as_bytes()).await?;
                            writer.write_all(b"\n").await
                        }
                        .await;
                        if let Err(err) = written {
                            tracing::warn!(target: "maicli::event_log", "gagal menulis log event: {err}");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(target: "maicli::event_log", "event tidak bisa diserialisasi: {err}");
                    }
                }
            }
            if outer_tx.send(event).is_err() {
                break;
            }
        }
        if let Some(mut writer) = file {
            let _ = writer.flush().await;
        }
        let _ = done_tx.send(());
    });
    (inner_tx, outer_rx, Some(done_rx))
}

/// Tunggu penulis log menyelesaikan flush terakhir (maks 2 detik).
///
/// Proses bisa keluar sebelum task penulis selesai; memanggil ini di akhir mode
/// membuat berkas `--event-log` lengkap dan deterministik.
pub async fn flush(done: Option<tokio::sync::oneshot::Receiver<()>>) {
    if let Some(done) = done {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), done).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("maicli-eventlog-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("events.jsonl")
    }

    #[tokio::test]
    async fn event_direkam_dan_diteruskan() {
        let path = temp("rekam");
        let (tx, mut rx, _done) = channel(Some(path.clone()));
        tx.send(AgentEvent::AgentStart).unwrap();
        tx.send(AgentEvent::TurnStart { index: 1 }).unwrap();
        assert_eq!(rx.recv().await, Some(AgentEvent::AgentStart));
        assert_eq!(rx.recv().await, Some(AgentEvent::TurnStart { index: 1 }));
        drop(tx);
        // Tunggu forwarder selesai menulis.
        for _ in 0..50 {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.lines().count() >= 2 {
                let events: Vec<AgentEvent> = text
                    .lines()
                    .map(|line| serde_json::from_str(line).expect("event valid"))
                    .collect();
                assert_eq!(
                    events,
                    vec![AgentEvent::AgentStart, AgentEvent::TurnStart { index: 1 }]
                );
                let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("event tidak terekam dalam 1 detik");
    }

    #[tokio::test]
    async fn tanpa_path_tidak_ada_berkas() {
        let (tx, mut rx, _done) = channel(None);
        tx.send(AgentEvent::AgentStart).unwrap();
        assert_eq!(rx.recv().await, Some(AgentEvent::AgentStart));
    }

    #[tokio::test]
    async fn direktori_log_dibuat_otomatis() {
        let dir =
            std::env::temp_dir().join(format!("maicli-eventlog-nested-{}", std::process::id()));
        let path = dir.join("sub").join("events.jsonl");
        let (tx, mut rx, _done) = channel(Some(path.clone()));
        tx.send(AgentEvent::AgentStart).unwrap();
        assert!(rx.recv().await.is_some());
        drop(tx);
        for _ in 0..50 {
            if path.exists() {
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("direktori log tidak dibuat");
    }
}
