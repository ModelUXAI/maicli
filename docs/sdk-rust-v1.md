# SDK Rust v1 (`tui-sdk`)

Status: **DONE (v1 awal)** — dikerjakan 2026-11-18 sebagai item P1 dari
dokumen roadmap internal (tidak dipublikasikan). Permukaan ini dijanjikan stabil selama `v1`:
perubahan yang memutus API harus menaikkan versi major dan dicatat di CHANGELOG.

## Tujuan

Embedder (host, IDE, skrip otomasi) bisa menjalankan satu sesi agent `maicli` tanpa
mengubah `tui-core` dan tanpa TUI. Fokus v1: **satu sesi per proses**, event dua
tingkat, kontrol abort/pause/model, dan tool set yang bisa diganti.

Bukan bagian v1: MCP, subagent, collab/WebSocket, server HTTP, API ekstensi,
platform Linux/macOS — semuanya masih di luar scope (lihat
dokumen roadmap internal (tidak dipublikasikan)).

## Bentuk API

```rust
use std::sync::Arc;
use tui_sdk::SessionBuilder;

let (mut session, mut events) = SessionBuilder::new("C:\\kerja")
    .provider(provider)   // Arc<dyn tui_sdk::Provider>
    .approver(approver)   // Arc<dyn tui_sdk::Approver>
    .model("model-id")
    .interactive(false)   // tool `ask` tidak meminta UI
    .build()?;

let outcome = session.prompt("ringkas repo ini").await?;

while let Some(event) = events.next().await {
    // AgentStart, TurnStart, MessageUpdate, ToolExecutionStart, ... AgentEnd
    if matches!(event, tui_sdk::AgentEvent::AgentEnd { .. }) {
        break;
    }
}
```

### Builder

| Metode | Wajib | Default |
|---|---|---|
| `SessionBuilder::new(cwd)` | ya | — |
| `.provider(Arc<dyn Provider>)` | **ya** | — (error `E_CONFIG`) |
| `.approver(Arc<dyn Approver>)` | **ya** | — (error `E_CONFIG`) |
| `.tools(ToolRegistry)` | tidak | `ToolRegistry::v1()` (8 tool) |
| `.settings(TurnSettings)` | tidak | `TurnSettings::default()` |
| `.model(id)` | tidak | model dari `TurnSettings` |
| `.interactive(bool)` | tidak | `false` |

`build()` mengembalikan `(Session, EventStream)`. Kanal event dibuatkan SDK;
embedder tidak perlu menyentuh `tokio::mpsc` langsung.

### Session

| Metode | Fungsi |
|---|---|
| `prompt(text).await` | jalankan satu turn (termasuk tool/follow-up internal) |
| `abort()` | batalkan turn berjalan (stream + tool) |
| `cancel_token()` | token untuk pengawas eksternal |
| `model()` / `set_model(id)` | model turn berikutnya |
| `messages()` | transkrip saat ini |
| `usage()` / `estimated_tokens()` | token terpakai / estimasi konteks |
| `pause()` / `resume()` | jeda di batas iterasi |
| `push_messages()` / `seed_usage()` | memulihkan sesi tersimpan |
| `set_context_window(n)` | ambang compaction otomatis |

### EventStream

| Metode | Fungsi |
|---|---|
| `next().await` | event berikutnya; `None` = kanal tutup |
| `try_next()` | ambil event yang sudah antre |

Urutan event satu turn mengikuti ledger paritas internal (tidak dipublikasikan): `agent_start` →
(`turn_start` → event pesan/tool)* → `turn_end` → `agent_end`. Pasangan
`message_start`/`message_end` dijamin lengkap, termasuk jalur abort/error.

## Jaminan dan batasan

1. **Satu sesi per proses** di v1; sesi paralel belum dijanjikan.
2. **Tidak ada UI**: `interactive=false`; tool `ask` mengembalikan error hasil,
   bukan menggantung.
3. **Approval tetap wajib**: tidak ada jalur pintas; embedder harus memberi
   `Approver` (mis. `PolicyApprover` dari `tui-approval`).
4. **Pembatalan**: `abort()` menghentikan streaming, eksekusi tool, dan
   penantian approval (lihat test core).
5. **Tidak menyimpan sesi**: persistensi tetap tanggung jawab embedder
   (gunakan `tui-session` bila perlu file JSONL).

## Bukti

`cargo test -p tui-sdk` — 5 test perilaku:

1. `prompt_menghasilkan_turn_dan_event_berpasangan` — hasil turn + urutan event
   (`agent_start` … `agent_end`, ada `message_end`) + transkrip berisi assistant;
2. `tool_call_dieksekusi_lewat_gate_dan_turn_selesai` — tool dipanggil, approver
   dipanggil, hasil tool masuk transkrip, turn selesai normal;
3. `builder_tanpa_provider_adalah_error_config` — error `E_CONFIG` yang menyebut
   apa yang harus diisi;
4. `abort_menghentikan_turn_yang_menunggu_stream` — selesai `aborted` < 2 detik;
5. `tool_output_wire_bisa_dipakai_embedder` — re-export tipe tool untuk tool
   kustom.

Doc-test pada contoh crate ikut dijalankan (`cargo test -p tui-sdk --doc`).

## Contoh embedding penuh

Lihat `crates/tui-app/src/text_mode.rs` sebagai embedder internal yang paling
dekat: ia membangun `AgentSession` dengan provider/approver nyata dan memetakan
event ke output teks. SDK adalah jalur yang sama tanpa ketergantungan `tui-app`.
## AgentRegistry (sesi paralel, 2026-11-19)

```rust
let mut registry = tui_sdk::AgentRegistry::baru();
registry.daftarkan("utama", builder);
let (mut sesi, mut events) = registry.bangun("utama").unwrap()?;
// sesi.prompt("...").await?;
```

- `daftarkan(id, builder)` menimpa; `bangun(id)` mem-clone builder (bisa dibangun ulang);
- `ids()`, `sesi(id) -> bool`, `hapus(id)`;
- Teruji: dua sesi paralel (tokio::join) + hapus satu.