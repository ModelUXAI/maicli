//! Pembanding transkrip untuk harness parity (RENCANA C-12).
//!
//! Menerima dua berkas JSONL berisi event agen (satu event per baris, format
//! `AgentEvent` serial), menormalisasi keduanya lewat `tui_wire::parity`, lalu
//! melaporkan perbedaan pertama beserta event yang hilang/berlebih.

use std::path::Path;

use tui_wire::parity::{ParityDiff, compare, normalize};
use tui_wire::{AgentEvent, ErrorCode, MxError};

/// Baca transkrip JSONL dan kembalikan event ternormalisasi.
fn read_transcript(path: &Path) -> Result<Vec<String>, MxError> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        MxError::new(
            ErrorCode::SessionCorrupt,
            format!("{}: transkrip tidak bisa dibaca: {err}", path.display()),
        )
    })?;
    let mut normalized = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: AgentEvent = serde_json::from_str(line).map_err(|err| {
            MxError::new(
                ErrorCode::SessionCorrupt,
                format!(
                    "{}: baris {} bukan event agen: {err}",
                    path.display(),
                    index + 1
                ),
            )
        })?;
        if let Some(normalized_event) = normalize(&event) {
            normalized.push(normalized_event);
        }
    }
    Ok(normalized)
}

/// Bandingkan dua transkrip dan kembalikan laporan diff.
pub fn compare_files(
    scenario: &str,
    expected: &Path,
    actual: &Path,
) -> Result<ParityDiff, MxError> {
    let left = read_transcript(expected)?;
    let right = read_transcript(actual)?;
    Ok(compare(scenario, &left, &right))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("maicli-parity-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn event_line(event: &AgentEvent) -> String {
        serde_json::to_string(event).expect("serial")
    }

    #[test]
    fn transkrip_identik_dinyatakan_bersih() {
        let dir = temp("sama");
        let event = AgentEvent::TurnStart { index: 1 };
        let line = event_line(&event);
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        std::fs::write(&a, format!("{line}\n")).expect("tulis");
        std::fs::write(&b, format!("{line}\n\n")).expect("tulis");
        let diff = compare_files("teks-murni", &a, &b).expect("banding");
        assert!(diff.is_clean(), "{diff:?}");
        assert_eq!(diff.report(), "teks-murni: identik");
    }

    #[test]
    fn perbedaan_event_dilaporkan_dengan_indeks() {
        let dir = temp("beda");
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        let lines_a = [
            AgentEvent::AgentStart,
            AgentEvent::TurnStart { index: 1 },
            AgentEvent::TurnEnd { index: 1 },
        ];
        let lines_b = [
            AgentEvent::AgentStart,
            AgentEvent::TurnStart { index: 2 },
            AgentEvent::TurnEnd { index: 1 },
        ];
        std::fs::write(
            &a,
            lines_a
                .iter()
                .map(|event| event_line(event) + "\n")
                .collect::<String>(),
        )
        .expect("tulis a");
        std::fs::write(
            &b,
            lines_b
                .iter()
                .map(|event| event_line(event) + "\n")
                .collect::<String>(),
        )
        .expect("tulis b");
        let diff = compare_files("mutasi", &a, &b).expect("banding");
        assert!(!diff.is_clean());
        assert_eq!(diff.first_mismatch, Some(1));
        let report = diff.report();
        assert!(
            report.contains("mutasi") && report.contains("#1"),
            "{report}"
        );
        assert!(report.contains("turn_start:1"), "{report}");
        assert!(report.contains("turn_start:2"), "{report}");
    }

    #[test]
    fn baris_rusak_dilaporkan_dengan_nomor_baris() {
        let dir = temp("rusak");
        let a = dir.join("a.jsonl");
        std::fs::write(&a, "{bukan event}\n").expect("tulis");
        let err = compare_files("rusak", &a, &a).expect_err("harus error");
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
        assert!(err.message.contains("baris 1"), "{}", err.message);
    }

    #[test]
    fn berkas_tidak_ada_dilaporkan() {
        let dir = temp("tidak-ada");
        let missing = dir.join("tidak-ada.jsonl");
        let err = compare_files("x", &missing, &missing).expect_err("harus error");
        assert_eq!(err.code, ErrorCode::SessionCorrupt);
    }
}
