# NOT-COMPLETE.md — yang belum diimplementasikan

Diperbarui: **2026-11-18** (setelah 12 putaran P2/P3). Sumber: `TUI/docs/*`
(mcp-v1, rpc-v1/v2, subagent-v1, sdk-rust-v1, platform-dan-integrasi,
fase-0/*) + audit fitur terhadap **omp 18.22.0** (`oh-my-pi-main/`).
Status saat audit: **463 test + 1 doc-test hijau** (`check.ps1`), 59 berkas /
25.338 baris / 13 crate; `docs/paritas.md` = 154 baris port (0 BLOCKED).

> Catatan konteks: paritas dengan omp **dilepas** (keputusan iqbal, opsi B).
> Bagian omp di bawah = peta potensi, bukan kewajiban v1.

---

## 1. Sisa dari TUI/docs (belum diimplementasikan)

### 1.1 MCP (`docs/mcp-v1.md`)

| Item | Status | Catatan |
|---|---|---|
| ~~Notifikasi MCP → timeline TUI~~ | ✅ 2026-11-18 (putaran 9) | **deadlock diselesaikan dua lapis**: forwarder **idle-timeout** (berhenti sendiri setelah 3×2 dtk tanpa notifikasi) + **abort** eksplisit sebelum printer; wiring hidup di teks (run_once+REPL) + TUI; bukti E2E MCP + repro manual 2,9 dtk (dahulu hang selamanya) |
| **OAuth authorization-code flow** (browser + loopback redirect + refresh token) | ⬜ | Jalur Bearer via `headers`/`env:` sudah ada; flow browser penuh belum (butuh interaksi browser) |
| Reconnect **stream** (GET SSE yang putus di tengah sesi) | ⬜ | Retry request 502–504 sudah; stream long-lived belum |
| Notifikasi long-lived di transport **HTTP/SSE** | ⬜ | Kanal langganan kosong untuk backend Http/Sse |
| `ping`/`roots/list` → jawaban kaya | 🟡 | Jawaban minimal ada |

### 1.2 RPC (`docs/rpc-v1.md`, `docs/rpc-v2.md`)

| Item | Status | Catatan |
|---|---|---|
| ~~`remove_queued_message`, `promote_queued_message`~~ | ✅ 2026-11-18 (putaran 2) | request v2 + arm busy (notice/error) + idle (error antrean kosong); 2 test. **`abort_and_restore_queue` SELESAI 2026-11-18**: abort turn + frame `queue_restored` (isi antrean dikembalikan klien, antrean dikosongkan) + cancel host tools/URI; test roundtrip |
| ~~`session_info_update`~~ | ✅ 2026-11-19 (putaran 2) | frame `session_info_update {session_id,model,title}` dikirim setelah `set_title` (baru) dan `model`/`open_session`; `SessionStore::session_id()` pub. Sisa: `login`, `command_output`, `config_update` |
| `extension_ui_request`/`response` + dialog kustom (`set_ask_dialog`) | 🟡 | `ui_extension` (one-way) + `question` ada; dialog dua arah belum |
| ~~Spool replay~~ ✅ 2026-11-19: `replay_spool` → `spool_replayed {jumlah}` — frame tertahan (kanal penuh) dimainkan ulang; teruji |
| Klien RPC resmi (Go/Python/TS) | ⬜ | omp punya `sdk/{go,python,rust}/omp-rpc`; kita hanya dokumen protokol |

### 1.3 Subagent / A2A (`docs/subagent-v1.md`, `platform-dan-integrasi.md` §8)

| Item | Status | Catatan |
|---|---|---|
| ~~Multi-turn anak~~ | ✅ 2026-11-19 | `SubagentCommand::Prompt` + loop multi-turn (satu sesi anak); Steer saat idle digabung ke prompt berikutnya; status: Running/Idle/Parked/Killed (Finished/Aborted dihapus — hasil terakhir tetap terbaca); 2 test |
| Steer **saat turn berjalan** masuk konteks | 🟡 | Steer saat berjalan → buffer → digabung ke prompt turn berikutnya (teruji); masuk **sebelum panggilan model berikutnya dalam turn yang sama** = belum (butuh run_turn API steer async) |
| ~~Event stream anak → induk~~ ✅ 2026-11-19 (tahap C v1): `handle.events()` — seluruh event sesi anak diteruskan (retensi tunggakan 200); test retensi terbukti. Frame RPC `subagent_progress` = belum (butuh desain frame) |
| ~~Agent Hub UI~~ ✅ **2026-11-19 (v1)**: **Alt+A** panel penuh — daftar anak + event live + **[s]teer/[p]rompt/[a]bort/[K]ill**; **`agent://sub-N` & `history://sub-N`** di composer → detail + 20 event di timeline; kolektor event otomatis. Sisa v2: transkrip pesan lengkap anak (bukan hanya event) ✅ **2026-11-20 selesai** — panel v2 penuh: transkrip + toggle + broadcast steer |ly ringkas |
| ~~Subagent D / ACP~~ 🟢 **2026-11-20 (v1)**: mode `acp` — agen = **server ACP** (JSON-RPC line-delimited stdio): host eksternal **dan lokal** mengendalikan (initialize/session_new/prompt dengan stream `session/update`/cancel, multi-sesi, konteks utuh antar prompt; E2E host nyata). Sisa v2: mx sebagai **klien ACP** (spawn agen eksternal sebagai subagent) |ansport (collab WS / ACP / custom) |
| Registry agen, discovery task (`task-agent-discovery`) | ⬜ | omp punya; kita belum |

### 1.4 SDK (`docs/sdk-rust-v1.md`)

| Item | Status | Catatan |
|---|---|---|
| ~~Sesi paralel (`AgentRegistry`)~~ | ✅ 2026-11-19 | `tui_sdk::AgentRegistry`: daftarkan builder per id, `bangun(id)` (builder Clone, bisa dibangun ulang), `ids/hapus/sesi`; dua sesi berjalan paralel (tokio::join) — 1 test + doc-test |
| **Janji semver** SDK | ⬜ | Keputusan produk (iqbal) |
| SDK TypeScript / Go / Python | ⬜ | omp punya tiga; kita Rust saja |

### 1.5 API server (`docs/platform-dan-integrasi.md` §4) — `mx serve` v1 sudah ada

| Item | Status | Catatan |
|---|---|---|
| ~~`POST /api/abort`, `GET /api/session`~~ | ✅ 2026-11-18 (putaran 4) | sesi **stateful** (transkrip+usage di state; Session dibangun ulang per request — tidak lintas-thread); prompt saat sibuk → **409**; abort idle → 409. Sisa: SSE push `/api/events` |
| ~~Autentikasi token file 0600~~ | ✅ 2026-11-18 (putaran 7) | `MX_SERVE_TOKEN` env menang; bila kosong → token **dibuat otomatis** di `<data>/serve-token` (0600/ACL) — server **tidak pernah** tanpa auth; E2E: tanpa token → 401, dengan token → 200 |
| Stateful: satu sesi panjang untuk banyak request | ⬜ | Saat ini satu sesi per prompt (stateless) |

### 1.6 WebSocket / collab (`platform-dan-integrasi.md` §6)

| Item | Status | Catatan |
|---|---|---|
| ~~WS relay~~ 🟢 **2026-11-20 (v1)**: mode `relay` — room WS kunci (env/file 0600), control/guest + promote, siaran event+jawab, sesi stateful; E2E 3 klien nyata. Sisa v2: multi-room + guest TUI read-only mode |olling `GET /api/events`; relay realtime butuh dependensi WS (mis. `tokio-tungstenite`) — **keputusan iqbal** |
| Transport provider WebSocket (mis. `PI_CODEX_WEBSOCKET`) | ⬜ | Tidak ada |

### 1.7 Platform (`platform-dan-integrasi.md` §2)

| Item | Status | Catatan |
|---|---|---|
| Verifikasi Linux/macOS nyata (chmod 600, kill-tree, keyring Keychain/Secret Service) | ⬜ | Kode POSIX sudah ditulis (`#[cfg(unix)]`); **butuh hardware/CI** |
| ~~check.sh di Linux~~ ✅ **2026-11-20 — SEMUA GERBANG HIJAU** di server iqbal (hermes-root x86_64 native; 501 test + 9 E2E; log /root/check-linux.log; source /root/tui-dev) |
| Packaging Linux/macOS + codesign/notarization | ⬜ | omp punya natives prebuilt 4 platform |
| Keyring macOS (Keychain) aktif | ⬜ | Backend Windows aktif |

### 1.8 Gate v1 (syarat rilis, milik iqbal)

| Item | Status |
|---|---|
| **UAT U-01..U-12** (`docs/uat-v1.md`) | ⬜ kolom hasil kosong |
| Verifikasi visual V-01/V-03 + angka C-09 (latensi, scroll) | ⬜ butuh mata manusia |
| `git init` + tag (O-8) | ⬜ folder bukan repo git |
| Amandemen formal `RENCANA.md` C-12 + `AGENTS.md` §0.4 (usulan teks di WORK A-51) | ⬜ menunggu iqbal |
| Keputusan `/resume` (pertahankan atau ubah jadi pembuka sesi) | ⬜ WORK pertanyaan terbuka #4 |
| **Rotasi kunci API** yang terekspos di chat | ⬜ **penting — lakukan** |

---

## 2. Perbandingan dengan oh-my-pi (fitur yang belum ada di TUI)

Audit `oh-my-pi-main/docs/*` (47 dokumen) + `packages/`. Urutan kasar:
besar → kecil.

### 2.1 Runtime agen lanjutan

| Fitur omp | Kita | Ukuran |
|---|---|---|
| **Extensions** (`docs/extensions.md`): loader paket ekstensi, custom tools/slash-commands dari JS/TS | ⬜ tidak ada | XL |
| ~~Hooks~~ | ✅ **internal** 2026-11-18 (putaran 10) | `AgentSession::set_after_tool(AfterToolHook)` — observasi (nama, is_error, cuplikan) di **semua jalur** (sukses/gagal/**ditolak approval**/paralel); 2 test. Hook **eksternal** dari config/extensions = masih ⬜ (butuh keputusan desain — eksekusi shell dari config berisiko) |
| ~~Skills~~ | ✅ 2026-11-18 (v1) | `tui_tools::skills`: folder `skills/<nama>/SKILL.md` + `MX_SKILLS_DIR` (dedup), header `description:`, daftar disuntik ke system prompt **semua mode** (TUI/teks/RPC/serve), tool `skill` (tier Read) membaca isi; 3 test |
| **Marketplace / plugin installer** (`docs/marketplace.md`, `plugin-manager-installer-plumbing.md`) | ⬜ tidak ada | XL |
| **Themes / keybindings** (`docs/theme.md`, `keybindings.md`) — kustomisasi | 🟡 aksen/dim via config; binding tetap | S |
| **System prompt customization** (`system-prompt-customization.md`) | ⬜ | S–M |
| **Magic keywords** (`magic-keywords.md`) | ⬜ | S |
| **Memory** (`memory.md`, `mnemosyne-memory-backend.md`) | ⬜ tidak ada | L–XL |
| **Autoresearch / prewalk / advisor-watchdog** (mode riset otomatis) | ⬜ tidak ada | XL |
| **Computer use / notebook / python-repl / resolve-tool runtime** (tool lanjutan) | ⬜ 8 tool v1 saja | L per tool |
| **TTSR injection, ai-schema-normalize, rulebook-matching** (pipeline internal omp) | ⬜ | L |
| **Blob artifact architecture** (attachment/binary artifact) | ⬜ | L |
| **fs-scan-cache** (cache pemindaian berkas untuk tool) | ⬜ | M |
| **LSP config** (`lsp-config.md`) | ⬜ | M |
| **Local models** (`local-models.md` — Ollama/LM Studio runtime) | 🟡 provider terdaftar; katalog model lokal belum | M |

### 2.2 Sesi & operasi

| Fitur omp | Kita | Ukuran |
|---|---|---|
| ~~Session tree~~ | ✅ 2026-11-18 (v1) | `tui_session::{pohon, SimpulPohon}` (parent dari header; kedalaman, proteksi siklus 32) + perintah **`/tree`** di teks & TUI (indentasi kedalaman); 2 test (root→fork kedalaman 0/1; siklus berhenti). Sisa: navigasi interaktif pohon |
| **Export/share** (`session-operations-export-share-fork-resume.md`) | ⬜ | M |
| **Session switching UI** yang kaya (recent list omp) | 🟡 `/sessions` + F7; ringan | S |
| **Compaction** penuh (ringkasan+prune+shake) | ✅ sudah | — |
| **Non-compaction retry policy** | 🟡 retry 429/5xx discovery; policy turn belum lengkap | S |

### 2.3 Distribusi & infra

| Fitur omp | Kita | Ukuran |
|---|---|---|
| **Natives Rust prebuilt** (4 platform, addon loader, PTY, shell) | ⬜ murni Rust, Windows teruji | XL |
| **Auth broker/gateway `serve`** penuh (OAuth broker antar-tool) | 🟡 `mx serve` API lokal ada; broker auth belum | XL |
| **Install-id / telemetri** | ⬜ | S |
| **Secrets manager** (`secrets.md`) | 🟡 env+keyring; manager penuh belum | M |
| **Collab web + browser relay** (`collab.md`, paket `collab-web`) | ⬜ | XL |
| **Gemini manifest extensions / ERRATA GPT-5 harmony / handoff-generation** (quirk per provider) | ⬜ | M per quirk |
| **Provider endpoint constraints + census budget** (budget gambar/endpoint per provider dari KDL) | 🟡 kompilator tahap 1 (`tui-katalog`); semantik penuh (classes/taxonomy/runtime/cascade — 2.000–3.000 baris) belum | L |

### 2.4 Yang **sudah** diuji setara (ringkas, bukti di `docs/paritas.md`)

Agent loop (steering/follow-up/pause/deadline/compaction), approval matrix + grants,
tool v1 + MCP 3 transport + bridge, RPC v1+v2 (chunk, host tools/URIs, ui
extension, sesi/antrean, spool), subagent A–C, SDK Rust, API server lokal,
collab polling, katalog KDL tahap 1, ACL/POSIX, sesi JSONL + fork + lock.

---

## 3. Rekomendasi urutan (bila dilanjutkan)

1. **Tutup v1 dulu** (milikmu): UAT + visual + git + rotasi kunci.
2. Keputusan cepat: SDK semver, `/resume`, ACP transport.
3. Nilai tertinggi lalu: **wiring notifikasi MCP → TUI** (S), **abort/session endpoint
   di serve** (S), **hooks internal** (M), **skills** (M), **session tree** (M).
4. Besar dengan keputusan: extensions/marketplace (XL), WS relay (XL),
   natives lintas platform + CI (XL), KDL semantik penuh (L), SDK
   Go/Python/TS (L).

---

*File ini dibuat atas permintaan iqbal (2026-11-18). Perbarui setiap kali
status berubah; jangan menumpuk status basi.*
| **Agent Orchestrator** | ✅ **2026-11-20 (v1)**: tool \orchestrate\ — memunculkan semua subagent (maks 8) paralel/berurutan, hasil ringkasan per task, semua anak live di Agent Hub (event/transkrip/broadcast); TUI + RPC |
