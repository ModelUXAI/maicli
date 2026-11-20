//! Pembaca SSE yang tahan terhadap chunk terpotong di mana pun.
//!
//! Satu chunk HTTP tidak dijamin berisi satu event, jadi decoder menyimpan
//! sisa baris dan hanya mengeluarkan payload setelah blok kosong penutup
//! (`\n\n`) tiba. `[DONE]` diteruskan sebagai payload khusus.

/// Satu payload SSE (gabungan semua baris `data:` dalam satu event).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsePayload {
    /// Data JSON siap diparse.
    Data(String),
    /// Penanda akhir stream.
    Done,
}

/// Decoder inkremental untuk aliran SSE.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    /// Buat decoder kosong.
    pub fn new() -> Self {
        Self::default()
    }

    /// Tambahkan potongan bytes; kembalikan payload yang sudah lengkap.
    pub fn push(&mut self, chunk: &str) -> Vec<SsePayload> {
        self.buffer.push_str(chunk);
        self.normalize_newlines();
        let mut out = Vec::new();
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer.drain(..pos + 2);
            if let Some(payload) = parse_block(&block) {
                out.push(payload);
            }
        }
        out
    }

    /// Samakan CRLF menjadi LF, tetapi tahan `\r` di ujung buffer.
    ///
    /// `\r` dan `\n` bisa tiba di chunk berbeda; menormalkan `\r` lebih awal
    /// akan mengubah satu akhir baris menjadi dua dan memalsukan batas event.
    fn normalize_newlines(&mut self) {
        let ends_with_cr = self.buffer.ends_with('\r');
        let body = if ends_with_cr {
            &self.buffer[..self.buffer.len() - 1]
        } else {
            &self.buffer[..]
        };
        let mut normalized = body.replace("\r\n", "\n").replace('\r', "\n");
        if ends_with_cr {
            normalized.push('\r');
        }
        self.buffer = normalized;
    }

    /// Sisa buffer yang belum membentuk event lengkap (untuk diagnostik).
    pub fn pending(&self) -> &str {
        &self.buffer
    }
}

fn parse_block(block: &str) -> Option<SsePayload> {
    let mut data: Option<String> = None;
    for line in block.split('\n') {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            if rest.trim() == "[DONE]" {
                return Some(SsePayload::Done);
            }
            data = Some(match data {
                Some(mut existing) => {
                    existing.push('\n');
                    existing.push_str(rest);
                    existing
                }
                None => rest.to_string(),
            });
        }
    }
    match data {
        // Baris `data:` kosong tidak membawa payload; jangan diteruskan sebagai
        // JSON kosong yang akan gagal diparse di lapisan atas.
        Some(text) if text.trim().is_empty() => None,
        Some(text) => Some(SsePayload::Data(text)),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_terbelah_antar_chunk_tidak_memalsukan_event() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: satu\r").is_empty());
        let out = decoder.push("\ndata: dua\r\n\r\n");
        assert_eq!(out, vec![SsePayload::Data("satu\ndua".into())]);
        assert!(decoder.pending().is_empty());
    }

    #[test]
    fn payload_kosong_tidak_diteruskan() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data:\n\n").is_empty());
        let out = decoder.push("data: {\"a\":1}\n\n");
        assert_eq!(out, vec![SsePayload::Data("{\"a\":1}".into())]);
    }

    #[test]
    fn done_dengan_spasi_tetap_dikenali() {
        let mut decoder = SseDecoder::new();
        assert_eq!(decoder.push("data:  [DONE] \n\n"), vec![SsePayload::Done]);
    }

    #[test]
    fn chunk_terbelah_di_tengah_baris() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: {\"a\":").is_empty());
        assert!(decoder.push("1}").is_empty());
        let out = decoder.push("\n\n");
        assert_eq!(out, vec![SsePayload::Data("{\"a\":1}".into())]);
    }

    #[test]
    fn dua_event_dalam_satu_chunk() {
        let mut decoder = SseDecoder::new();
        let out = decoder.push("data: satu\n\ndata: dua\n\n");
        assert_eq!(
            out,
            vec![
                SsePayload::Data("satu".into()),
                SsePayload::Data("dua".into())
            ]
        );
    }

    #[test]
    fn crlf_dan_komentar_diabaikan() {
        let mut decoder = SseDecoder::new();
        let out = decoder.push(": ping\r\ndata: {\"b\":2}\r\n\r\n");
        assert_eq!(out, vec![SsePayload::Data("{\"b\":2}".into())]);
    }

    #[test]
    fn done_menutup_stream() {
        let mut decoder = SseDecoder::new();
        let out = decoder.push("data: [DONE]\n\n");
        assert_eq!(out, vec![SsePayload::Done]);
    }

    #[test]
    fn buffer_menyimpan_sisa_tanpa_newline() {
        let mut decoder = SseDecoder::new();
        decoder.push("data: belum");
        assert_eq!(decoder.pending(), "data: belum");
    }
}
