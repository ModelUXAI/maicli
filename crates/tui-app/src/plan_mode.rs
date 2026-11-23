//! Rencana kerja persisten (`/plan`, `/plan-review`).
//!
//! Disimpan sebagai Markdown di `<data>/plan.md` dan disuntikkan ke system
//! prompt. `/plan <teks>` menimpa rencana; `/plan append <teks>` menambah
//! langkah; `/plan clear` menghapus.

use std::path::PathBuf;

/// Path berkas rencana.
pub fn path() -> PathBuf {
    crate::config_root().join("plan.md")
}

/// Muat rencana (kosong bila belum ada).
pub fn muat() -> String {
    std::fs::read_to_string(path()).unwrap_or_default()
}

/// Tulis rencana baru.
pub fn tulis(teks: &str) -> Result<(), String> {
    let target = path();
    if let Some(induk) = target.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, teks).map_err(|e| e.to_string())
}

/// Tambah satu baris ke rencana yang sudah ada.
pub fn tambah(teks: &str) -> Result<(), String> {
    let lama = muat();
    let baru = if lama.trim().is_empty() {
        format!("- {teks}\n")
    } else {
        format!("{}\n- {teks}\n", lama.trim_end())
    };
    tulis(&baru)
}

/// Hapus rencana.
pub fn bersihkan() -> Result<(), String> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// Render blok system prompt.
pub fn blok(rencana: &str) -> String {
    if rencana.trim().is_empty() {
        return String::new();
    }
    format!("\n## Rencana kerja aktif (/plan)\n\n{}", rencana.trim())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn blok_rencana() {
        assert!(blok("- langkah satu").contains("langkah satu"));
        assert!(blok("   ").is_empty());
    }

    #[test]
    fn tambah_ke_rencana_kosong() {
        let lama = muat();
        let baru = if lama.trim().is_empty() {
            format!("- {}\n", "langkah")
        } else {
            format!("{}\n- {}\n", lama.trim_end(), "langkah")
        };
        assert_eq!(baru, "- langkah\n");
    }
}
