//! Cache daftar model gateway di berkas (RENCANA C-02: cache di memori + file).
//!
//! Cache disimpan di `<config root>/models-cache.json` dengan TTL 24 jam.
//! Tujuan: `/model` tetap punya daftar tanpa memanggil jaringan setiap start.
//! File rusak atau kedaluwarsa dianggap tidak ada (fail-open ke fetch), bukan
//! error yang menghalangi aplikasi.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Umur maksimum cache (24 jam) sesuai C-02.
pub const MODELS_CACHE_TTL_MS: u64 = 24 * 60 * 60 * 1_000;

/// Isi file cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelsCacheFile {
    /// Waktu pengambilan (epoch milidetik).
    pub fetched_at_ms: u64,
    /// Id model yang tersedia.
    pub models: Vec<String>,
}

/// Baca cache bila ada dan masih segar.
pub fn load(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let file: ModelsCacheFile = serde_json::from_str(&text).ok()?;
    if file.models.is_empty() {
        return None;
    }
    if now_ms().saturating_sub(file.fetched_at_ms) > MODELS_CACHE_TTL_MS {
        return None;
    }
    Some(file.models)
}

/// Tulis cache secara atomik (tmp + rename).
pub fn save(path: &Path, models: &[String]) -> Result<(), std::io::Error> {
    if models.is_empty() {
        return Ok(());
    }
    let file = ModelsCacheFile {
        fetched_at_ms: now_ms(),
        models: models.to_vec(),
    };
    let text = serde_json::to_string(&file).map_err(std::io::Error::other)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text.as_bytes())?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("maicli-models-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("models-cache.json")
    }

    #[test]
    fn simpan_dan_muat_bolak_balik() {
        let path = temp_path("bulat");
        let models = vec!["m1".to_string(), "m2".to_string()];
        save(&path, &models).expect("simpan");
        assert_eq!(load(&path), Some(models));
    }

    #[test]
    fn cache_kedaluwarsa_diabaikan() {
        let path = temp_path("basi");
        let old = ModelsCacheFile {
            fetched_at_ms: now_ms().saturating_sub(MODELS_CACHE_TTL_MS + 1_000),
            models: vec!["m1".to_string()],
        };
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, serde_json::to_string(&old).expect("json")).expect("tulis");
        assert_eq!(load(&path), None, "lebih tua dari TTL harus diabaikan");
    }

    #[test]
    fn file_rusak_atau_kosong_dianggap_tidak_ada() {
        let path = temp_path("rusak");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, "{bukan json").expect("tulis");
        assert_eq!(load(&path), None);
        std::fs::write(&path, "{\"fetched_at_ms\":1,\"models\":[]}").expect("tulis");
        assert_eq!(load(&path), None, "daftar kosong tidak disimpan/dipakai");
        assert_eq!(load(&temp_path("tidak-ada")), None);
    }
}
