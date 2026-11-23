# Subagent lokal v1 (`tui-subagent`)

Status: **DONE (tahap A + B)** — item P2 dari dokumen roadmap internal (tidak dipublikasikan).
**Tahap B (2026-11-18): `SubagentManager` + `SubagentHandle`** — kontrol anak live:
`Steer` (buffer → prompt turn berikutnya), `Abort` (token), `Park`/`Resume` (pause handle),
`Kill` (matikan task); status 6 keadaan (`berjalan/idle/parkir/selesai/dibatalkan/dihentikan`);
**lineage**: `SessionStore::create_with_parent` menulis header `parent_session` = id induk;
hasil ditulis **sebelum** status final (tanpa race). Registry anak otomatis **tanpa tool
`subagent`** (tidak bersarang). 9 test crate. **Tahap C (2026-11-18): panel TUI F3** — overlay daftar subagent (id, status,
turn/tool, potongan hasil); tool `subagent` terdaftar di TUI dengan
`with_manager`, jadi anak yang di-spawn model langsung terlihat di panel.
F1 menyebut F3. Tahap D (antar-instance) menyusul.

## Bentuk API

```rust
use std::sync::Arc;
use tui_subagent::{SubagentSpec, SubagentTool, run_subagent};

let spec = SubagentSpec {
    provider,              // Arc<dyn Provider> — biasanya sama dengan induk
    approver,              // Arc<dyn Approver> — gate tool anak
    tools,                 // ToolRegistry anak (tanpa tool `subagent`)
    model: "model-id".into(),
    deadline: std::time::Duration::from_secs(600),
    max_tool_calls: 32,    // proteksi loop
};

// Langsung:
let hasil = run_subagent(&spec, "riset berkas X").await;   // SubagentResult
// atau sebagai tool induk:
let tool = SubagentTool::new(spec.clone());
```

## Semantik

- Anak = `AgentSession` penuh: tool, approval, compaction, deadline sendiri.
- Satu prompt; hasil = teks assistant terakhir + ringkasan (`N turn, M tool call`).
- `SubagentTool` bertier **exec** + `Concurrency::Exclusive` — selalu melewati
  gate approval induk; **tidak ada jalur pintas**.
- Gagal anak (deadline, galat turn) → hasil error ke induk; **turn induk tetap
  lanjut**; teks kosong ditandai `(tanpa teks)`.
- Tidak bersarang: registry anak tidak boleh memuat tool `subagent` (pemanggil
  yang menambahkannya melanggar kontrak; crate tidak memfasilitasi nesting).
- Tanpa file sesi di tahap ini: hasil hidup di memori induk; persistensi +
  lineage (`parent_session`) = tahap B.

## Bukti

`cargo test -p tui-subagent` — 5 test (provider skrip `ProviderEvent::Done`):

1. `subagent_satu_turn_mengembalikan_teks` — hasil teks + 1 turn + 0 tool;
2. `subagent_menjalankan_tool_dan_melanjutkan` — tool call (todo) dieksekusi,
   lalu jawaban akhir;
3. `tool_subagent_eksekusi_menghasilkan_ringkasan_dan_teks` — jalur tool induk;
4. `prompt_kosong_adalah_error_hasil`;
5. `count_dan_last_text_membaca_transkrip` — helper transkrip.

## Tahap B — API manager

```rust
use tui_subagent::manager::{SubagentManager, SubagentCommand, SubagentState};

let manager = SubagentManager::new();
let handle = manager.spawn(spec, "prompt anak".into(), "induk-xyz", Some(sessions_dir));
// handle.status() → SubagentState (berjalan/idle/parkir/selesai/dibatalkan/dihentikan)
// handle.steer("fokus ke berkas X") — dipakai sebagai prompt turn berikutnya
// handle.abort() / handle.park() / handle.resume() / handle.kill()
// handle.result() → Option<SubagentResult>
// handle.session_path() → Option<PathBuf> (file sesi anak, parent_session terisi)
```

Bukti B (4 test): `spawn_selesai_dan_hasil_terbaca`, `kill_mengubah_status_dan_hasil_error`
(provider gantung), `lineage_file_anak_memuat_parent_session` (header `parent_session`
terbaca), `abort_turn_berjalan_menghasilkan_status_aborted`.

## Keputusan desain yang diberlakukan

1. Crate **terpisah** (`tui-subagent`), bukan modul `tui-tools`: `tui-tools` →
   `tui-core` → `tui-tools` adalah siklus (terbukti compiler saat dicoba).
2. Tool di level aplikasi: memasang `SubagentTool` ke registry adalah keputusan
   host/SDK, bukan default (`ToolRegistry::v1()` tetap 8 tool).
3. `max_tool_calls` dicatat di hasil; penegakan penuh (interupsi antar tool) =
   tahap B (karena v1 agent mengeksekusi tool per turn dan loop dipagari
   deadline + max 2 compaction/turn).

## Event anak → induk (tahap C v1, 2026-11-19)

- `SubagentHandle::events()` — langganan event sesi anak (AgentStart/End,
  ToolExecutionStart/End, Notice) sebagai kanal event dua tingkat;
- Notifikasi pra-langganan diserahkan juga (retensi tunggakan 200 — tidak hilang
  di antara spawn dan langganan);
- Implementasi: task penerus membaca penerima internal sesi anak dan mengirim ke slot
  langganan; banyak langganan diizinkan (kanal baru per panggilan).

## Agent Hub UI (Alt+A, 2026-11-19)

- **Alt+A**: panel penuh — kiri daftar anak (id + status), kanan event live + kendali;
- ↑↓ atau Ctrl+J/K memilih anak; **[s] teer**, **[p] prompt** (ketik → Enter), **[a] bort,
  **[K] ill** (Shift+K); Esc/Alt+A menutup;
- Event dikumpulkan otomatis oleh task kolektor (buffer 300 baris per anak);
- **`agent://sub-N` / `history://sub-N`** di composer: status + 20 event terakhir di timeline.

## Agent Hub v2: transkrip + broadcast (2026-11-20)

- **[t]** toggle kolom kanan: event ↔ **transkrip** (teks asisten per turn + baris
  `→ panggil <tool>`, dari event `MessageEnd`; cap 200 baris, 400 karakter per baris);
- **[b] broadcast steer**: ketik teks → Enter → dikirim ke **semua** anak yang berjalan
  (status bar "steer terkirim ke N").

## Agent Orchestrator (2026-11-20)

- Tool `orchestrate`: `{tasks:[{id,prompt}], parallel?:bool}` — memunculkan semua subagent
  (maks `MAKS_TASK_ORKESTRASI` = 8) lewat manager bersama;
- **paralel** (default): semua spawn → tunggu semua; **berurutan**: satu per satu;
- Hasil: ringkasan per task + total (ok/gagal); semua anak live di Agent Hub (Alt+A),
  bisa di-steer/broadcast/prompt lanjutan; TUI dan RPC terdaftar.

## ACP transport (subagent D, 2026-11-20)

- `maicli --mode acp`: agen sebagai **server ACP** di stdin/stdout (JSON-RPC 2.0
  line-delimited) — host eksternal (Zed dsb.) **dan lokal** (pipe) mengendalikan;
- Metode: `initialize`, `session/new`, `session/prompt` (blok teks; selama turn mengalir
  notifikasi `session/update`), `session/cancel` (cancel token — tidak menunggu lock);
- Multi-sesi: HashMap slot (ownership pindah ke task turn, dikembalikan setelah selesai —
  konteks utuh antar prompt);
- E2E: proses `maicli` anak dikendalikan host nyata — initialize/new/prompt/update/
  end_turn/cancel/tak dikenal — semua hijau.