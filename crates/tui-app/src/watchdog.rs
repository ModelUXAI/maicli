//! Advisor watchdog + aturan TTSR sederhana.
//!
//! Watchdog memeriksa teks kandidat terhadap pola larangan dan meminta turn
//! berhenti sebelum aksi berbahaya. Aturan TTSR disuntikkan ke konteks agar
//! model mengikutinya. Ini bukan stream-token abort penuh.

/// Apakah teks memicu aturan larangan (case-insensitive).
pub fn perlu_stop(teks: &str, aturan: &[String]) -> Option<String> {
    let rendah = teks.to_ascii_lowercase();
    aturan
        .iter()
        .find(|p| !p.trim().is_empty() && rendah.contains(&p.to_ascii_lowercase()))
        .cloned()
}

/// Parse daftar aturan dari argumen dipisah koma.
pub fn parse_aturan(arg: &str) -> Vec<String> {
    arg.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Blok konteks aturan TTSR.
pub fn blok(aturan: &[String]) -> String {
    if aturan.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n## Aturan TTSR (larangan)\n");
    for a in aturan {
        out.push_str(&format!("- Jangan lakukan/ghasilkan: {a}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn watchdog_dan_blok() {
        let aturan = vec!["rm -rf".to_string(), "DROP TABLE".to_string()];
        assert_eq!(perlu_stop("jangan rm -rf /", &aturan).unwrap(), "rm -rf");
        assert!(perlu_stop("aman", &aturan).is_none());
        assert!(blok(&aturan).contains("DROP TABLE"));
        assert_eq!(parse_aturan("a, b ,").len(), 2);
    }
}
