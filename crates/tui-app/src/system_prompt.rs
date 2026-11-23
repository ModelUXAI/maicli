//! Override system prompt pengguna (`/system-prompt`).
//!
//! Disimpan di `<data>/system-prompt.md` dan disuntikkan setelah prompt dasar;
//! `clear` menghapus override.

/// Path berkas override.
pub fn path() -> std::path::PathBuf {
    crate::config_root().join("system-prompt.md")
}

/// Muat override (kosong bila tidak ada).
pub fn muat() -> String {
    std::fs::read_to_string(path()).unwrap_or_default()
}

/// Tulis override.
pub fn tulis(teks: &str) -> Result<(), String> {
    let target = path();
    if let Some(induk) = target.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, teks).map_err(|e| e.to_string())
}

/// Hapus override.
pub fn bersihkan() -> Result<(), String> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Render blok prompt; kosong bila tidak ada override.
pub fn blok(teks: &str) -> String {
    if teks.trim().is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Instruksi pengguna (/system-prompt)\n\n{}",
            teks.trim()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blok_prompt() {
        assert!(blok("jawab singkat").contains("jawab singkat"));
        assert!(blok("  ").is_empty());
    }
}
