//! Resolusi id model dan cache discovery.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tui_wire::ErrorCode;

use crate::client::ModelInfo;
use crate::error::GatewayError;

/// Cocokkan id yang diminta pengguna dengan daftar model gateway.
///
/// Urutan: sama persis → sama tanpa peduli huruf besar/kecil → id tanpa prefiks
/// provider → unik lewat substring. Ambigu atau kosong = error bernama, bukan
/// tebakan (`AGENTS.md` §3: jangan menebak id model).
pub fn match_model(requested: &str, available: &[String]) -> Result<String, GatewayError> {
    let wanted = requested.trim();
    if wanted.is_empty() {
        return Err(GatewayError {
            code: ErrorCode::ModelUnknown,
            status: None,
            message: "id model kosong".to_string(),
            retryable: false,
            retry_after: None,
        });
    }
    if available.is_empty() {
        return Err(unknown(&format!(
            "daftar model kosong; tidak bisa mencocokkan '{wanted}'"
        )));
    }
    if let Some(hit) = available.iter().find(|id| id.as_str() == wanted) {
        return Ok(hit.clone());
    }
    let lower = wanted.to_ascii_lowercase();
    if let Some(hit) = available.iter().find(|id| id.to_ascii_lowercase() == lower) {
        return Ok(hit.clone());
    }
    let bare = lower.rsplit('/').next().unwrap_or(&lower).to_string();
    if let Some(hit) = available.iter().find(|id| {
        id.rsplit('/')
            .next()
            .unwrap_or(id)
            .eq_ignore_ascii_case(&bare)
    }) {
        return Ok(hit.clone());
    }
    let candidates: Vec<&String> = available
        .iter()
        .filter(|id| id.to_ascii_lowercase().contains(&lower))
        .collect();
    match candidates.len() {
        1 => candidates
            .first()
            .map(|id| (*id).clone())
            .ok_or_else(|| unknown(wanted)),
        0 => Err(unknown(&format!(
            "tidak ada model yang cocok dengan '{wanted}'"
        ))),
        _ => {
            let preview: Vec<String> = candidates.iter().take(5).map(|id| (*id).clone()).collect();
            Err(unknown(&format!(
                "'{wanted}' cocok dengan {} model ({}); tulis id yang lebih spesifik",
                candidates.len(),
                preview.join(", ")
            )))
        }
    }
}

fn unknown(message: &str) -> GatewayError {
    GatewayError {
        code: ErrorCode::ModelUnknown,
        status: None,
        message: message.to_string(),
        retryable: false,
        retry_after: None,
    }
}

/// Cache hasil `GET /models` dengan TTL, waktu di-inject agar bisa diuji.
#[derive(Debug)]
pub struct DiscoveryCache {
    ttl: Duration,
    entry: Mutex<Option<(Instant, Vec<ModelInfo>)>>,
}

impl DiscoveryCache {
    /// Cache dengan TTL tertentu (omp memakai 24 jam).
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entry: Mutex::new(None),
        }
    }

    /// Isi cache bila masih segar pada waktu `now`.
    pub fn get(&self, now: Instant) -> Option<Vec<ModelInfo>> {
        let guard = self.entry.lock().ok()?;
        let (stored_at, models) = guard.as_ref()?;
        if now.duration_since(*stored_at) < self.ttl {
            Some(models.clone())
        } else {
            None
        }
    }

    /// Simpan hasil discovery.
    pub fn store(&self, now: Instant, models: Vec<ModelInfo>) {
        if let Ok(mut guard) = self.entry.lock() {
            *guard = Some((now, models));
        }
    }

    /// Buang isi cache (dipakai `/model refresh`).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.entry.lock() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn cocok_persis() {
        let available = ids(&["deepseek-v4.1-flash", "lain"]);
        assert_eq!(
            match_model("deepseek-v4.1-flash", &available).unwrap(),
            "deepseek-v4.1-flash"
        );
    }

    #[test]
    fn cocok_tanpa_peduli_huruf_besar() {
        let available = ids(&["DeepSeek-V4.1-Flash"]);
        assert_eq!(
            match_model("deepseek-v4.1-flash", &available).unwrap(),
            "DeepSeek-V4.1-Flash"
        );
    }

    #[test]
    fn cocok_lewat_id_tanpa_prefiks_provider() {
        let available = ids(&["openrouter/deepseek-v4.1-flash"]);
        assert_eq!(
            match_model("deepseek-v4.1-flash", &available).unwrap(),
            "openrouter/deepseek-v4.1-flash"
        );
    }

    #[test]
    fn cocok_lewat_substring_unik() {
        let available = ids(&["glm-5-air", "deepseek-v4.1-flash"]);
        assert_eq!(
            match_model("v4.1", &available).unwrap(),
            "deepseek-v4.1-flash"
        );
    }

    #[test]
    fn substring_ambigu_ditolak() {
        let available = ids(&["glm-5-air", "glm-5-pro"]);
        let err = match_model("glm-5", &available).unwrap_err();
        assert_eq!(err.code, ErrorCode::ModelUnknown);
        assert!(err.message.contains("2 model"));
    }

    #[test]
    fn tidak_ada_dan_kosong_ditolak() {
        let available = ids(&["glm-5-air"]);
        assert!(match_model("tidak-ada", &available).is_err());
        assert!(match_model("   ", &available).is_err());
        assert!(match_model("apa-saja", &[]).is_err());
    }

    #[test]
    fn cache_menghormati_ttl() {
        let cache = DiscoveryCache::new(Duration::from_secs(60));
        let start = Instant::now();
        cache.store(start, vec![ModelInfo { id: "m1".into() }]);
        assert!(cache.get(start + Duration::from_secs(59)).is_some());
        assert!(cache.get(start + Duration::from_secs(60)).is_none());
        cache.invalidate();
        assert!(cache.get(start).is_none());
    }
}
