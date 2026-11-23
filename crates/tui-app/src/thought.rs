//! Kontrol internal `think`/`yield` (hidden) dan guided goal.
//!
//! `think` menyimpan catatan penalaran singkat; `yield` menandai bahwa agen
//! menunggu masukan. Blok ini disuntikkan ke konteks, bukan ditampilkan ke
//! pengguna sebagai pesan biasa.

/// Satu catatan kontrol agen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thought {
    pub kind: &'static str,
    pub text: String,
}

/// Parse perintah teks `think: ...` / `yield: ...`.
pub fn parse(teks: &str) -> Option<Thought> {
    let t = teks.trim();
    if let Some(isi) = t
        .strip_prefix("think:")
        .or_else(|| t.strip_prefix("think "))
    {
        let isi = isi.trim();
        if !isi.is_empty() {
            return Some(Thought {
                kind: "think",
                text: isi.to_string(),
            });
        }
    }
    if t == "yield" || t.starts_with("yield:") {
        return Some(Thought {
            kind: "yield",
            text: t.trim_start_matches("yield:").trim().to_string(),
        });
    }
    None
}

/// Render blok kontrol untuk konteks.
pub fn blok(thought: &Thought) -> String {
    format!(
        "\n\n## Kontrol agen ({})\n\n{}",
        thought.kind,
        if thought.text.is_empty() {
            "(tanpa teks)"
        } else {
            &thought.text
        }
    )
}

/// Prompt langkah pertama untuk `/guided-goal`.
pub fn guided_prompt(tujuan: &str) -> String {
    format!(
        "Tujuan: {tujuan}\n\nSusun rencana singkat, lalu kerjakan langkah pertama yang aman. \
         Berhenti dan laporkan bila butuh keputusan pengguna."
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_think_yield_dan_guided() {
        assert_eq!(parse("think: cek bug").unwrap().kind, "think");
        assert_eq!(parse("yield").unwrap().kind, "yield");
        assert!(parse("biasa").is_none());
        assert!(
            blok(&Thought {
                kind: "think",
                text: "x".into()
            })
            .contains("Kontrol agen")
        );
        assert!(guided_prompt("fitur").contains("langkah pertama"));
    }
}
