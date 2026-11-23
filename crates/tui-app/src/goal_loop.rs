//! Parser goal/loop/ratchet (dasar fitur goals/loop).
//!
//! Command memakai format:
//! - `/goal <teks>` — tujuan persisten;
//! - `/loop <n> <prompt>` — ulangi prompt n kali via antrean follow-up;
//! - `/ratchet [prompt]` — satu langkah lanjutan.
//!
//! Modul ini hanya mem-parse; eksekusi antrean dilakukan pemanggil.

/// Path goal persisten.
pub fn path() -> std::path::PathBuf {
    crate::config_root().join("goal.md")
}

/// Muat goal.
pub fn muat() -> String {
    std::fs::read_to_string(path()).unwrap_or_default()
}

/// Simpan goal.
pub fn tulis(teks: &str) -> Result<(), String> {
    let target = path();
    if let Some(induk) = target.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, teks).map_err(|e| e.to_string())
}

/// Hapus goal.
pub fn bersihkan() -> Result<(), String> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Parse `/loop <n> <prompt>`.
pub fn parse_loop(arg: &str) -> Result<(u32, String), String> {
    let (angka, prompt) = arg
        .split_once(char::is_whitespace)
        .ok_or_else(|| "pakai: /loop <n> <prompt>".to_string())?;
    let n: u32 = angka
        .parse()
        .map_err(|_| "jumlah loop harus angka".to_string())?;
    if n == 0 || n > 20 {
        return Err("jumlah loop harus 1..20".to_string());
    }
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("prompt loop tidak boleh kosong".to_string());
    }
    Ok((n, prompt.to_string()))
}

/// Parse `/ratchet [prompt]`; default = lanjutkan goal.
pub fn parse_ratchet(arg: &str, goal: &str) -> Result<String, String> {
    let teks = arg.trim();
    let prompt = if teks.is_empty() { goal.trim() } else { teks };
    if prompt.is_empty() {
        return Err("ratchet butuh prompt atau goal aktif".to_string());
    }
    Ok(prompt.to_string())
}

/// Blok system prompt untuk goal aktif.
pub fn blok(goal: &str) -> String {
    if goal.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n## Goal aktif (/goal)\n\n{}", goal.trim())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_loop_dan_ratchet() {
        assert_eq!(
            parse_loop("3 perbaiki bug").unwrap(),
            (3, "perbaiki bug".into())
        );
        assert!(parse_loop("0 x").is_err());
        assert!(parse_loop("21 x").is_err());
        assert_eq!(
            parse_ratchet("", "selesaikan fitur").unwrap(),
            "selesaikan fitur"
        );
        assert_eq!(parse_ratchet("langkah", "goal").unwrap(), "langkah");
    }
}
