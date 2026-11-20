//! Smoke test **live** gateway (C-02 kriteria 5).
//!
//! Test ini `#[ignore]` dan hanya jalan atas perintah eksplisit setelah kunci
//! tersedia:
//!
//! ```text
//! setx MODELUXAI_API_KEY "..."
//! cargo test -p tui-app --test live_gateway -- --ignored --nocapture
//! ```
//!
//! Opsional: `MODELUXAI_BASE_URL` untuk gateway selain default. Model **tidak
//! ditebak**: diambil dari `GET /v1/models` lebih dulu (AGENTS §3).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use tui_provider::{ChatRequest, OpenAiCompatClient, Provider};
use tui_wire::{Message, ProviderEvent, StopReason};

fn kunci() -> String {
    match std::env::var("MODELUXAI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => panic!(
            "MODELUXAI_API_KEY belum diisi; jalankan: \
             setx MODELUXAI_API_KEY \"<kunci>\" lalu buka shell baru"
        ),
    }
}

fn base_url() -> String {
    std::env::var("MODELUXAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.modeluxai.com/v1".to_string())
}

#[tokio::test]
#[ignore = "smoke live gateway: butuh MODELUXAI_API_KEY dan izin jaringan eksplisit"]
async fn smoke_live_discovery_dan_satu_turn() {
    let client = OpenAiCompatClient::new(base_url(), Some(kunci())).expect("klien gateway");
    let models = tokio::time::timeout(Duration::from_secs(30), client.list_models())
        .await
        .expect("discovery tidak boleh menggantung")
        .expect("discovery harus berhasil");
    assert!(
        !models.is_empty(),
        "gateway harus mengembalikan minimal satu model"
    );
    let model = models[0].id.clone();
    println!("model dari discovery: {model}");

    let request = ChatRequest::new(model, vec![Message::user("Balas satu kata: ok")]);
    let mut rx = tokio::time::timeout(Duration::from_secs(30), client.stream_chat(request))
        .await
        .expect("stream tidak boleh menggantung")
        .expect("stream harus terbuka");

    let mut teks = String::new();
    let mut selesai = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    while !selesai {
        let sisa = deadline.saturating_duration_since(tokio::time::Instant::now());
        let event = tokio::time::timeout(sisa, rx.recv())
            .await
            .expect("turn live tidak boleh melewati batas waktu")
            .expect("kanal event tidak boleh tertutup sebelum selesai");
        match event {
            ProviderEvent::TextDelta { text } => teks.push_str(&text),
            ProviderEvent::Done { message } => {
                assert!(
                    message.stop_reason != StopReason::Error,
                    "turn live berakhir dengan error: {:?}",
                    message.error
                );
                selesai = true;
            }
            ProviderEvent::Error { code, message } => {
                panic!("gateway mengembalikan error {code}: {message}");
            }
            _ => {}
        }
    }
    assert!(!teks.trim().is_empty(), "jawaban live tidak boleh kosong");
    println!("jawaban live: {teks}");
}
