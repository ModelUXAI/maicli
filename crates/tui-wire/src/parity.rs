//! Normalisasi event dan pembanding transkrip untuk harness parity.
//!
//! Harness differential membandingkan urutan event, bukan teks bebas: id acak
//! (`tool_call_id`), timestamp, dan detail yang tidak stabil dibuang, sedangkan
//! hal yang mengubah perilaku (urutan turn, teks delta, tool yang dipanggil,
//! status error) dipertahankan. Modul ini murni supaya bisa diuji deterministik;
//! menjalankan harness-nya sendiri ada di luar v1 (butuh omp + gateway).

use crate::event::{AgentEvent, ProviderEvent};

/// Ubah satu event agen menjadi bentuk ternormalisasi yang stabil.
///
/// `None` berarti event tersebut tidak relevan untuk parity (mis. pembaruan
/// status murni UI) dan tidak ikut dibandingkan.
pub fn normalize(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::AgentStart => Some("agent_start".to_string()),
        // Perintah UI murni — tidak relevan untuk parity transkrip.
        AgentEvent::ClearTimeline => None,
        AgentEvent::AgentEnd { turns } => Some(format!("agent_end:{turns}")),
        AgentEvent::TurnStart { index } => Some(format!("turn_start:{index}")),
        AgentEvent::TurnEnd { index } => Some(format!("turn_end:{index}")),
        AgentEvent::MessageStart => Some("message_start".to_string()),
        AgentEvent::MessageUpdate(inner) => match inner {
            ProviderEvent::TextDelta { text } => Some(format!("text_delta:{text}")),
            ProviderEvent::TextEnd => Some("text_end".to_string()),
            ProviderEvent::ToolCallStart { name, .. } => Some(format!("toolcall_start:{name}")),
            ProviderEvent::Done { .. } => None,
            ProviderEvent::Error { code, .. } => Some(format!("provider_error:{code}")),
            _ => None,
        },
        AgentEvent::MessageEnd { message } => Some(format!(
            "message_end:{}:{}",
            message.error.as_deref().unwrap_or("ok"),
            stop_reason_label(message.stop_reason)
        )),
        AgentEvent::ToolExecutionStart {
            tool_name, preview, ..
        } => Some(format!("tool_start:{tool_name}:{preview}")),
        AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            ..
        } => Some(format!("tool_end:{tool_name}:{is_error}")),
        // Event khusus UI tidak mengubah perilaku percakapan.
        AgentEvent::Notice { .. }
        | AgentEvent::SessionSwitched { .. }
        | AgentEvent::SessionsListed { .. }
        | AgentEvent::ContextUsage { .. } => None,
    }
}

/// Label stabil untuk stop reason (ikut dibandingkan karena membedakan perilaku).
fn stop_reason_label(reason: crate::message::StopReason) -> &'static str {
    match reason {
        crate::message::StopReason::Stop => "stop",
        crate::message::StopReason::ToolUse => "tooluse",
        crate::message::StopReason::Length => "length",
        crate::message::StopReason::Error => "error",
        crate::message::StopReason::Aborted => "aborted",
    }
}

/// Normalisasi seluruh event, membuang yang tidak relevan.
pub fn normalize_all(events: &[AgentEvent]) -> Vec<String> {
    events.iter().filter_map(normalize).collect()
}

/// Hasil pembandingan dua transkrip ternormalisasi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityDiff {
    /// Nama skenario yang dibandingkan.
    pub scenario: String,
    /// Indeks event pertama yang berbeda.
    pub first_mismatch: Option<usize>,
    /// Event yang hanya ada di transkrip acuan (hilang pada implementasi).
    pub missing: Vec<String>,
    /// Event yang hanya ada di transkrip hasil (kelebihan pada implementasi).
    pub extra: Vec<String>,
}

impl ParityDiff {
    /// Tidak ada perbedaan berarti.
    pub fn is_clean(&self) -> bool {
        self.first_mismatch.is_none() && self.missing.is_empty() && self.extra.is_empty()
    }

    /// Laporan yang menyebut skenario dan event yang berbeda.
    pub fn report(&self) -> String {
        if self.is_clean() {
            return format!("{}: identik", self.scenario);
        }
        let mut text = format!("{}: berbeda", self.scenario);
        if let Some(index) = self.first_mismatch {
            text.push_str(&format!("\n  event pertama berbeda: #{index}"));
        }
        if !self.missing.is_empty() {
            text.push_str("\n  hilang:");
            for event in &self.missing {
                text.push_str(&format!("\n    - {event}"));
            }
        }
        if !self.extra.is_empty() {
            text.push_str("\n  kelebihan:");
            for event in &self.extra {
                text.push_str(&format!("\n    + {event}"));
            }
        }
        text
    }
}

/// Bandingkan transkrip `expected` (acuan) dengan `actual` (implementasi).
pub fn compare(scenario: &str, expected: &[String], actual: &[String]) -> ParityDiff {
    let first_mismatch = expected
        .iter()
        .zip(actual.iter())
        .position(|(left, right)| left != right);
    // Bila prefix sama tetapi panjang berbeda, titik berbeda adalah ujung prefix.
    let first_mismatch = first_mismatch.or_else(|| {
        if expected.len() == actual.len() {
            None
        } else {
            Some(expected.len().min(actual.len()))
        }
    });
    let missing = expected
        .iter()
        .filter(|event| !actual.contains(event))
        .cloned()
        .collect();
    let extra = actual
        .iter()
        .filter(|event| !expected.contains(event))
        .cloned()
        .collect();
    ParityDiff {
        scenario: scenario.to_string(),
        first_mismatch,
        missing,
        extra,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::message::{AssistantMessage, StopReason};

    fn started(tool: &str, preview: &str) -> AgentEvent {
        AgentEvent::ToolExecutionStart {
            tool_call_id: "id-acak-1".to_string(),
            tool_name: tool.to_string(),
            preview: preview.to_string(),
        }
    }

    #[test]
    fn normalisasi_membuang_id_acak() {
        let a = normalize(&started("bash", "dir")).unwrap();
        let b = normalize(&AgentEvent::ToolExecutionStart {
            tool_call_id: "id-lain".to_string(),
            tool_name: "bash".to_string(),
            preview: "dir".to_string(),
        })
        .unwrap();
        assert_eq!(a, b, "id tool_call tidak boleh ikut dibandingkan");
        assert_eq!(a, "tool_start:bash:dir");
    }

    #[test]
    fn normalisasi_mempertahankan_urutan_dan_status() {
        let events = vec![
            AgentEvent::AgentStart,
            AgentEvent::TurnStart { index: 1 },
            AgentEvent::MessageUpdate(ProviderEvent::TextDelta {
                text: "halo".to_string(),
            }),
            AgentEvent::ToolExecutionEnd {
                tool_call_id: "x".to_string(),
                tool_name: "bash".to_string(),
                is_error: true,
                preview: "gagal".to_string(),
            },
            AgentEvent::TurnEnd { index: 1 },
            AgentEvent::AgentEnd { turns: 1 },
        ];
        assert_eq!(
            normalize_all(&events),
            vec![
                "agent_start",
                "turn_start:1",
                "text_delta:halo",
                "tool_end:bash:true",
                "turn_end:1",
                "agent_end:1"
            ]
        );
        // Event UI murni tidak ikut.
        assert_eq!(
            normalize(&AgentEvent::ContextUsage {
                tokens: 1,
                window: 2
            }),
            None
        );
    }

    #[test]
    fn compare_identik_untuk_urutan_sama() {
        let expected = vec!["agent_start".to_string(), "turn_start:1".to_string()];
        let diff = compare("teks-murni", &expected, &expected);
        assert!(diff.is_clean(), "{diff:?}");
        assert_eq!(diff.report(), "teks-murni: identik");
    }

    #[test]
    fn compare_menunjuk_event_pertama_yang_berbeda() {
        let expected = normalize_all(&[
            AgentEvent::AgentStart,
            AgentEvent::TurnStart { index: 1 },
            AgentEvent::MessageUpdate(ProviderEvent::TextDelta {
                text: "halo".to_string(),
            }),
            AgentEvent::AgentEnd { turns: 1 },
        ]);
        let actual = normalize_all(&[
            AgentEvent::AgentStart,
            AgentEvent::TurnStart { index: 1 },
            AgentEvent::MessageUpdate(ProviderEvent::TextDelta {
                text: "hai".to_string(),
            }),
            AgentEvent::AgentEnd { turns: 1 },
        ]);
        let diff = compare("mutasi-teks", &expected, &actual);
        assert!(!diff.is_clean());
        assert_eq!(diff.first_mismatch, Some(2));
        let report = diff.report();
        assert!(report.contains("mutasi-teks"), "{report}");
        assert!(report.contains("#2"), "{report}");
        assert!(report.contains("text_delta:halo"), "{report}");
        assert!(report.contains("text_delta:hai"), "{report}");
    }

    #[test]
    fn compare_mendeteksi_event_hilang_dan_kelebihan() {
        let expected = vec!["agent_start".to_string(), "turn_start:1".to_string()];
        let actual = vec!["agent_start".to_string(), "turn_start:2".to_string()];
        let diff = compare("beda-turn", &expected, &actual);
        assert_eq!(diff.missing, vec!["turn_start:1".to_string()]);
        assert_eq!(diff.extra, vec!["turn_start:2".to_string()]);
        let report = diff.report();
        assert!(report.contains("hilang"), "{report}");
        assert!(report.contains("kelebihan"), "{report}");
    }

    #[test]
    fn message_end_membawa_status_error() {
        let mut message = AssistantMessage::text("x", StopReason::Error);
        message.error = Some("gagal".to_string());
        let normalized = normalize(&AgentEvent::MessageEnd { message }).unwrap();
        assert_eq!(normalized, "message_end:gagal:error");
    }
}
