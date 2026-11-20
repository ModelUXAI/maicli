//! Inisialisasi logging ke berkas.
//!
//! Jalur TUI tidak boleh menulis ke stdout/stderr, jadi seluruh log diarahkan
//! ke `logs/maicli-YYYY-MM-DD.log` di direktori config.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Pasang subscriber yang menulis ke berkas harian; kembalikan guard agar
/// penulisan tidak hilang saat proses berakhir.
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    init_with(log_dir, None)
}

/// Seperti [`init`], tetapi filter bisa ditentukan langsung (dipakai test).
pub fn init_with(log_dir: &Path, level: Option<&str>) -> Option<WorkerGuard> {
    if std::fs::create_dir_all(log_dir).is_err() {
        return None;
    }
    let appender = tracing_appender::rolling::daily(log_dir, "maicli.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = match level {
        Some(level) => EnvFilter::new(level),
        None => EnvFilter::try_from_env("MAICLI_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
    };
    let built = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
    if built.is_err() {
        return None;
    }
    Some(guard)
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn log_ditulis_ke_berkas_dan_bisa_dibaca_ulang() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("maicli-log-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let guard = init_with(&dir, Some("debug")).expect("subscriber terpasang");
        tracing::debug!(target: "maicli::test", "penanda-log-uji");
        drop(guard);
        let mut found = false;
        for _ in 0..20 {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                    if text.contains("penanda-log-uji") {
                        found = true;
                    }
                }
            }
            if found {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(found, "log harus memuat pesan uji");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
