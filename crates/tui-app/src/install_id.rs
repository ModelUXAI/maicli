//! Id instalasi stabil, lokal, tanpa telemetri jaringan.
//!
//! Dipakai untuk korelasi log/bug report. Berkas dibuat sekali di `<data>/install-id`
//! dengan izin paling ketat; tidak pernah dikirim ke mana pun oleh kode ini.

use std::path::PathBuf;

/// Path berkas id instalasi.
pub fn path() -> PathBuf {
    crate::config_root().join("install-id")
}

/// Validasi bentuk id heksadesimal 16–32 karakter.
pub fn valid(id: &str) -> bool {
    (16..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Baca atau buat id instalasi.
pub fn dapatkan() -> Result<String, String> {
    if let Ok(ada) = std::fs::read_to_string(path()) {
        let id = ada.trim().to_string();
        if valid(&id) {
            return Ok(id);
        }
    }
    let id = format!(
        "{:016x}{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
        std::process::id()
    );
    let target = path();
    if let Some(induk) = target.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, &id).map_err(|e| e.to_string())?;
    tui_wire::secure_file::restrict_to_current_user(&target).map_err(|e| e.to_string())?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn validasi_id() {
        assert!(valid("0123456789abcdef"));
        assert!(!valid("pendek"));
        assert!(!valid("zzzzzzzzzzzzzzzz"));
    }
}
