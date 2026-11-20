# Dukungan Platform & Integrasi — Status dan Peta Jalan

Dokumen ini menjawab tiga pertanyaan sekaligus:

1. Platform mana yang **didukung** (`Windows native`, `Linux native`, `macOS native`)
   dan apa yang masih kurang untuk masing-masing;
2. Permukaan integrasi mana yang sudah ada dan mana yang **belum** — `SDK`, `API`,
   `RPC`, `WebSocket`, `MCP`, dan `agent-to-agent`;
3. Fitur apa saja di `oh-my-pi` (omp) yang **belum kita punya** dan harus dibangun,
   lengkap dengan urutan dan kriteria penerimaannya.

Sumber pembanding: source omp **18.22.0** di `oh-my-pi-main/` (path dirujuk per
Sumber pembanding: source omp **18.22.0** di \oh-my-pi-main/\ (path dirujuk per
bagian). Kondisi implementasi kita: \cargo test\ **501 test** + 9 E2E,
\check.ps1\ hijau di Windows (per 2026-11-20, setelah 4 keputusan iqbal dieksekusi).
Legenda status:

| Simbol | Arti |
|---|---|
| ✅ | Ada dan diuji/diverifikasi |
| 🟡 | Ada sebagian, belum memenuhi seluruh kriteria |
| ❌ | Belum ada sama sekali |
| ⛔ | Sengaja di luar v1 (RENCANA §13); butuh keputusan iqbal untuk dibangun |

---

## 1. Ringkasan eksekutif

| Permukaan | Kita (v1) | omp 18.22.0 | Kesenjangan utama |
|---|---|---|---|
| Windows native | ✅ target & teruji | ✅ | — (tinggal UAT/verifikasi visual) |
| Linux native | 🟢 **gerbang hijau + kill-tree POSIX + 0600 teruji + packaging** (2026-11-20) | ✅ prebuilt natives | uji TUI manual + smoke lanjutan |
| macOS native | ❌ | ✅ prebuilt natives | sama + Keychain, codesign/notarization |
| SDK | 🟡 `tui-sdk` v1 (Rust, satu sesi/proses) | ✅ TS in-process + SDK RPC Go/Python/Rust | belum ada SDK TS/Go/Python; stabilitas semver belum diputuskan |
| SDK | 🟡 `tui-sdk` v1 + **AgentRegistry** (sesi paralel) | ✅ TS + SDK RPC Go/Python/Rust | SDK TS/Go/Python + semver belum (keputusan) |
| API | 🟡 `mx serve` + **extensions loader** + **ACP server** (`mx --mode acp`) | ✅ setara + marketplace | marketplace download + ACP klien (mx sebagai subagent) belum |
| RPC | 🟡 **semua frame omp ada** (login/config_update/command_output/subagent_list/replay_spool termasuk) | ✅ | parity frame penuh ✅ |
| WebSocket | 🟢 **relay penuh** (`mx --mode relay`: room kunci, control/guest + promote, siaran event) | ✅ | multi-room + guest TUI read-only belum |
| MCP | 🟡 stdio + HTTP + legacy SSE + **notifikasi long-lived + reconnect** | ✅ setara + OAuth | OAuth browser flow belum |
| Agent-to-agent | 🟢 subagent A–D: multi-turn, event anak→induk, **Agent Hub (Alt+A)**, `agent://`/`history://`, **ACP**, **orchestrator** | ✅ setara | klien ACP (agen eksternal sebagai anak) belum |
**Kesimpulan (diperbarui setelah P1 selesai + keputusan paritas):** kita kini
memiliki SDK Rust, RPC v2 (negosiasi+chunk+host tools/URIs), MCP stdio+HTTP,
dan subagent tahap A — **P1 integrasi dasar selesai**. omp tetap lebih luas di
P2/P3 (collab WebSocket, extension API, ACP, platform lintas-OS). **Keputusan
iqbal 2026-11-18: paritas omp dilepas** — omp jadi rujukan sejarah perilaku,
bukan target kesetaraan.

---

## 2. Platform native

### 2.1 Windows native — ✅ (target v1)

| Aspek | Kondisi |
|---|---|
| Build/test | `check.ps1` hijau; 377 test; release `mx.exe` terbangun |
| Shell tool | `cmd.exe /C` (`docs/tools-v1.md:116`) |
| Kill-tree `bash` | `taskkill /PID <pid> /T /F` (`crates/tui-tools/src/lib.rs:772`) |
| ACL file sensitif | `icacls /inheritance:r /grant:r <user>:F` (`crates/tui-wire/src/secure_file.rs:25`) |
| Keyring | backend `windows-native` (`crates/tui-config/Cargo.toml:20`) |
| Verifikasi visual | V-03 PARTIAL — menunggu sesi manual iqbal (A-9, V-01) |

Tidak ada pekerjaan platform tersisa yang bisa dikerjakan tanpa iqbal; yang tersisa
adalah UAT dan verifikasi visual (`docs/uat-v1.md`).

### 2.2 Linux native — 🟡 (gerbang hijau 2026-11-20; sebagian kriteria tercapai)

2026-11-20: `check.sh` **SEMUA GERBANG HIJAU** di server native iqbal (hermes-root, x86_64, 8 core — bukan Docker/WSL): 501 test + 9 E2E; **izin 0600 teruji** (`acl_restricted` POSIX kini memeriksa `mode & 0o077 == 0`). Kriteria §2.2: #1 ✅ #3 ✅; sisa #2 (kill-tree POSIX masih no-op), #4 (TUI manual), #5 (pernyataan keyring env-only). Detail temuan:

| Bagian | Kondisi sekarang | Yang harus dibangun |
|---|---|---|
| Shell `bash` | `/bin/sh -c` sudah dipilih saat bukan Windows (`docs/tools-v1.md:116`) | uji nyata di linux-x64 & arm64 |
| Kill-tree timeout/batal | ✅ **proses group**: `process_group(0)` + `/usr/bin/kill -9 -- -pgid` — **test pohon 2 level** lulus di server (temuan debug: zombie ≠ hidup) | — |
| ACL berkas | `secure_file` **no-op** di non-Windows | `chmod 600` + `umask 077`, test permission bits |
| Keyring | crate `keyring` difitur `windows-native` saja | aktifkan backend Linux (Secret Service) atau dokumentasikan env-only |
| Gate/test | `check.ps1` Windows; `check.sh` ada tetapi **belum pernah dieksekusi** | runner CI Linux native (bukan Docker/WSL, sesuai AGENTS §3), jalankan `check.sh` |
| Packaging | belum ada artefak Linux | target build + smoke test biner |
| TUI | crossterm/ratatui lintas platform secara desain | uji terminal Linux (raw mode, restore, resize) |

Kriteria penerimaan Linux native:

1. `check.sh` hijau di runner Linux native (x64), log disimpan;
2. `kill_tree` menghentikan seluruh pohon proses (test dengan proses anak);
3. izin berkas sesi/config/grant = `0600` (test);
4. TUI berjalan dan terminal kembali normal setelah keluar (rekaman/manual);
5. keyring Linux atau pernyataan eksplisit "Linux: env var saja" di dokumen.

### 2.3 macOS native — ❌ (belum didukung v1)

Sama seperti Linux, ditambah hal khas macOS:

| Bagian | Kondisi | Yang harus dibangun |
|---|---|---|
| Shell & kill-tree | sama seperti Linux | process group kill, test |
| ACL | no-op | `chmod 600` + test |
| Keyring | backend macOS (Keychain) belum diaktifkan | aktifkan backend Keychain atau env-only |
| Distribusi | belum ada | `codesign` + notarization bila biner dibagikan (omp punya `docs/macos-signing-notarization.md`) |
| TUI/terminal | belum diuji | uji Terminal.app/iTerm2: raw mode, restore, resize, bracketed paste |
| Gate/CI | belum ada | runner macOS native, `check.sh` hijau |

Kriteria penerimaan macOS: sama dengan Linux (1–5) + biner tertandatangani/notarized
bila didistribusikan, atau pernyataan "source-only, build sendiri".

### 2.4 Ringkasan pekerjaan platform

| Item | Linux | macOS | Windows |
|---|---|---|---|
| POSIX process-group kill | wajib | wajib | n/a |
| Permission 600 | wajib | wajib | ACL ✅ |
| Keyring backend | wajib/dinyatakan env-only | wajib/dinyatakan env-only | ✅ |
| CI + `check.sh` | wajib | wajib | `check.ps1` ✅ |
| Uji TUI terminal | wajib | wajib | manual (V-03) |
| Packaging | build + smoke | codesign/notarize | release lokal |

Catatan scope: `RENCANA.md` §1/§13 menetapkan **Windows-native** untuk v1. Dukungan
Linux/macOS adalah **amandemen scope** yang harus diputuskan iqbal, bukan pekerjaan
implisit.

---

## 3. SDK

### 3.1 Yang dimiliki omp

- SDK in-process TypeScript dari paket `@oh-my-pi/pi-coding-agent`
  (`docs/sdk.md`): `createAgentSession`, `SessionManager`, `Settings`, `AuthStorage`,
  `ModelRegistry`, `AgentRegistry`, discovery helper (`discoverExtensions`,
  `discoverSkills`, `discoverMCPServers`, …), `createTools`, langganan event
  (`session.subscribe`), `session.prompt`, `session.dispose`.
- SDK lintas proses berbasis RPC: `sdk/go/omp-rpc`, `sdk/python/omp-rpc`,
  `sdk/rust/omp-rpc` (klien protokol v1/v2, termasuk negosiasi dan reassembly chunk).
- Pola pemakaian: "provide to override, omit to discover"; satu sesi per proses
  kecuali `AgentRegistry` privat untuk banyak sesi.

### 3.2 Yang kita punya

🟡 `crates/tui-sdk`: `SessionBuilder` + `Session` + `EventStream` + **AgentRegistry**
(sesi paralel, 2026-11-19), 6 test + 2 doc-test. Yang belum: SDK TS/Go/Python + janji
semver (keputusan iqbal).

### 3.3 Yang harus dibangun (status per 2026-11-18)

Crate baru `tui-sdk` (nama kerja), bergantung pada `tui-core`/`tui-provider`, bukan
pada `tui-app`:

| Komponen | Isi | Kriteria penerimaan |
|---|---|---|
| `AgentSessionBuilder` | cwd, provider, model, approval policy, tool allowlist, session store | ✅ `SessionBuilder` (provider/approver wajib, tools/settings opsional); test `builder_tanpa_provider_adalah_error_config` |
| `SessionHandle` | `prompt`, `abort`, `subscribe` (stream event bertipe), `dispose` | ✅ `Session` + `EventStream`; test `prompt_menghasilkan_turn_dan_event_berpasangan`, `abort_menghentikan_turn_yang_menunggu_stream` |
| Event stream publik | tipe event stabil + versi | 🟡 re-export `AgentEvent`/`ProviderEvent`; janji semver masih keputusan terbuka |
| Tool injection | daftar tool boleh/paksa, tool kustom | 🟡 `SessionBuilder::tools(ToolRegistry)`; test `tool_call_dieksekusi_lewat_gate_dan_turn_selesai` |
| Dokumentasi | `docs/sdk-rust-v1.md` + rustdoc | ✅ dokumen ada; crate bebas `unwrap/expect/panic` (gate clippy) |

Prasyarat keputusan: apakah SDK menjadi permukaan yang **dijanjikan stabil** (semver
sendiri) atau sekadar crate internal yang diekspos. Ini keputusan produk, bukan
teknis.

---

## 4. API

"API" di sini dipisah menjadi empat jenis agar tidak tercampur:

| Jenis API | Kita | omp | Catatan |
|---|---|---|---|
| Klien gateway model (OpenAI-compatible) | ✅ `crates/tui-provider`: `/v1/models`, streaming SSE, tool_call, retry/backoff, usage | ✅ | Ini satu-satunya API yang sudah kita punya; diuji dengan gateway mock, live belum (V-02) |
| API server lokal/daemon | 🟡 **`mx serve`** ✅ (health/prompt/session/abort/events + SSE push, token 0600) | ✅ `auth-broker`/`auth-gateway` `serve` | broker OAuth + sesi multi belum |
| API ekstensi/plugin | 🟡 **skills** ✅ (folder + tool) + hook `after_tool` internal | ✅ extensions, hooks, custom tools, marketplace | loader extension eksternal belum (keputusan) |
| API klien agen (ACP) | ❌ | ✅ mode ACP (`packages/coding-agent/src/modes/acp/*`) | Protokol untuk editor/host eksternal mengendalikan agen |

### 4.1 Yang harus dibangun bila API server diputuskan

Usulan minimum (`mx serve`, kandidat Fase 2/3):

1. `GET /healthz` — status proses/model;
2. `POST /v1/agent/prompt` — body prompt, respons SSE event agen (pakai ulang event
   internal);
3. `GET /v1/session` — metadata sesi aktif;
4. `POST /v1/agent/abort`;
5. autentikasi lokal (token file 0600) + bind `127.0.0.1` default.

Prasyarat: keputusan dependensi (mis. `axum`/`hyper`), dan penegasan bahwa ini **bukan**
jalur yang mem-bypass approval.

---

## 5. RPC

### 5.1 Kondisi kita (v1 + transport v2)

`mx --mode rpc` (`docs/rpc-v1.md`): JSONL stdin/stdout, frame `ready`
(`protocol: mx-rpc/1`, model), 8 permintaan (`prompt`, `abort`, `model`, `ping`,
`usage`, `answer`, `approval`, `shutdown`), 9 frame server (`ready`, `pong`, `ack`,
`question`, `approval_request`, `usage`, `event`, `result`, `error`), batas frame
**1 MiB**, field tambahan diabaikan, `type` tak dikenal → `E_PROTOCOL`.

### 5.2 Yang dimiliki omp (`docs/rpc.md`)

| Fitur | omp | Kita |
|---|---|---|
| Negosiasi versi | `ready` mengumumkan `supportedProtocolVersions`, klien kirim `negotiate_protocol` (v2) | ✅ `supported_protocol_versions`, `negotiate_protocol`, frame `negotiated` |
| Batas fisik | 1 MiB per frame stdout | ✅ 1 MiB |
| Frame besar | `rpc_chunk` base64 + validasi `chunkId/index/count/byteLength`, reassembly sampai 64 MiB | ✅ `encode_frame` + `RpcFrameDecoder`; 8 test wire + 3 test app |
| Semantik prompt | `response` ack masuk antrean, lalu `prompt_result` (status/yield), `session_settled` | 🟡 `result` → `prompt_result` (status ok/aborted/error) → `session_settled` |
| Antrean pesan | `remove_queued_message`, `promote_queued_message`, `abort_and_restore_queue` | ❌ (prompt saat sibuk ditolak, bukan diantrekan) |
| Host tools | `set_host_tools` + `host_tool_call`/`host_tool_result` (host menyediakan tool) | ✅ selesai (prefiks `host.`, tier exec, timeout 120 dtk; `host_tool_cancel` belum) |
| Host URIs | `host_uri_request`/`result` (mis. `history://`) | ✅ selesai (protokol + wiring tool file) |
| Extension UI | `extension_ui_request`/`response`, dialog, `set_ask_dialog` | 🟡 hanya `question` untuk tool `ask` |
| Subagent | `subagent_lifecycle/progress/event`, `set_subagent_subscription` | ❌ |
| Sesi/queue | `open_session`, `remove_queued_message`, `promote_queued_message`, `abort_and_restore_queue`, `get_state` | ❌ |
| Login/berbagi | frame `login`, `command_output`, `session_info_update`, `config_update` | ❌ |

### 5.3 Yang harus dibangun (bertahap)

| Tahap | Isi | Kriteria |
|---|---|---|
| RPC v1.1 | `prompt_result` terpisah dari `result`, `session_settled`, id konsisten | ✅ selesai (`result` → `prompt_result` → `session_settled`) |
| RPC v2 | `negotiate_protocol`, `rpc_chunk` + validasi penuh, batas 64 MiB | ✅ selesai (`docs/rpc-v2.md`); test urutan salah/metadata berubah/base64 rusak/oversize |
| Host tools & URIs | `host_tool_*`, `host_uri_*` dengan allowlist host | ✅ keduanya selesai (tool file memantulkan URL skema host lewat `UriResolver`) |
| Sesi/queue | buka/tutup sesi, antrean pesan, `get_state` | test state machine; tidak ada pesan hilang |
| Subagent | langganan event subagent (setelah §7 ada) | — |

Perubahan protokol wajib: naikkan `protocol` (`mx-rpc/2`), pertahankan klien v1, update
`docs/rpc-v1.md` → `docs/rpc-v2.md`, dan tambah E2E frame.

---

## 6. WebSocket

### 6.1 Yang dimiliki omp

- **Collab** (`docs/collab.md`): `/collab` membagikan sesi berjalan secara realtime ke
  omp lain atau browser; guest merender sesi yang sama, bisa prompt/interrupt (mode
  control) atau hanya menonton (mode view). Relay default `wss://<relay>/r/<roomId>.<key>`,
  lokal `ws://localhost:7475`; room mengikuti sesi (`/new`, `/resume`, `/fork`), autoStart,
  retry/backoff; paket `collab-web` dan `browser-relay` di omp.
- Transport provider WebSocket opsional untuk beberapa provider
  (`docs/environment-variables.md`, mis. `PI_CODEX_WEBSOCKET`).

### 6.2 Yang kita punya

🟡 Tidak ada **WebSocket**; transport jaringan kita: HTTP(S)+SSE ke gateway, **dan API server lokal** (`mx serve`): REST + **SSE push** riwayat (collab v1). Relay WS penuh = keputusan + dependensi.
gateway model (`reqwest`, rustls).

### 6.3 Yang harus dibangun bila collab/realtime diputuskan

Ini fitur kompleks (relay + kriptografi kunci room + otorisasi guest + replikasi state
TUI), bukan "tambah satu dependensi":

1. protokol room: join, snapshot sesi, delta event, kontrol (prompt/abort) dengan
   batas izin view/control;
2. E2E: kunci room di URL fragment; relay tidak membaca isi;
3. relay: pakai layanan yang ada atau server kecil; opsi lokal `ws://127.0.0.1`;
4. klien: guest TUI read-only dulu, control kemudian;
5. keamanan: tidak ada token/secret di URL selain kunci room; perintah guest tetap
   melewati approval host.

Status rekomendasi: **Fase 3**, butuh keputusan produk (apakah collab termasuk produk
kita) dan dependensi WS (mis. `tokio-tungstenite`).

---

## 7. MCP (Model Context Protocol)

### 7.1 Yang dimiliki omp

- Klien MCP penuh: transport **stdio**, **Streamable HTTP**, dan **HTTP+SSE legacy**
  (`docs/mcp-protocol-transports.md`), handshake `initialize` →
  `notifications/initialized`, `tools/list`, `tools/call`, notifikasi progres,
  penanganan `ping`/`roots/list`, korelasi request, timeout/cancel, refresh OAuth,
  reconnect, dan pemetaan error.
- Tool bridge MCP → tool agen (`docs/mcp-server-tool-authoring.md` §4), konfigurasi
  `mcp.json`/settings (`docs/mcp-config.md`), lifecycle & UI operator.
- Server-side authoring (cara menulis MCP server/tool) juga didokumentasikan.

### 7.2 Yang kita punya

🟡 `crates/tui-mcp`: **semua transport** (stdio + Streamable HTTP + legacy SSE) + **notifikasi long-lived + reconnect** (2026-11-19) + auth `env:` + retry 502-504 + config url/transport. 10 test. Sisa: OAuth browser flow.

### 7.3 Yang harus dibangun (kandidat Fase 2)

Crate `tui-mcp`:

| Komponen | Isi | Kriteria |
|---|---|---|
| Transport stdio | spawn server, JSONL, handshake, `tools/list`, `tools/call`, timeout, matikan child saat exit | ✅ selesai; test mock server PowerShell |
| Tool bridge | daftar tool MCP masuk `ToolRegistry` dengan nama `mcp.<server>.<tool>`, tier default `exec`/`write` sesuai anotasi server | ✅ selesai (`register_into`, `into_tools_with_tier`); test prefix + tier |
| Config | `[mcp.servers]` (command/args/env, http/sse url), validasi ketat | ✅ `[mcp]` + `[mcp.servers.*]` (command/args/env/timeout/enabled); 4 test config |
| HTTP/SSE | streamable HTTP + legacy SSE | 🟡 Streamable HTTP ✅ (`connect_http`, mock HttpListener); legacy SSE belum |
| Keamanan | env rahasia tidak diwariskan, output di-cap, approval tetap wajib | ✅ env dibuang + tier exec default; wiring gate mengikuti registry tool (sama seperti tool lain) |

Catatan scope: MCP saat ini **⛔ di luar v1** (RENCANA §13). Dokumen ini mencatat
kebutuhan dan desainnya agar keputusan bisa diambil tanpa riset ulang.

---

## 8. Agent-to-agent (A2A)

### 8.1 Yang dimiliki omp

omp menyebut ini "subagent", bukan protokol A2A standar. Tidak ditemukan bukti
implementasi spesifikasi Google A2A di dokumen yang dibaca (yang ada: subagent
internal, collab antar-instance omp, dan ACP untuk host eksternal).

- **Subagent**: spawn sinkron/detached, registry agen, lineage induk/anak,
  hibernasi/"parked", revive, kill, steering pesan saat berjalan
  (`docs/agent-hub.md`, `docs/task-agent-discovery.md`).
- **Agent Hub**: roster + inspector + activity log di TUI (`Alt+A`), baca/steer
  transkrip subagent.
- **URI agen**: `agent://<id>` (output akhir / pesan), `history://<id>`
  (transkrip), `read proc://` (job latar); pesan ke agen = revive/steer/prompt.
- **Collab**: dua instance omp berbagi satu sesi (host menjalankan agen+tools).
- **ACP**: host eksternal (editor) mengendalikan agen lewat protokol ACP
  (`packages/coding-agent/src/modes/acp/*`).

### 8.2 Yang kita punya

🟢 Subagent A–D penuh (2026-11-20): tool `subagent` + **orchestrator** (maks 8 anak
paralel/berurutan), **multi-turn anak** (prompt saat idle), **event anak→induk**
(retensi), **Agent Hub (Alt+A)** — event live + transkrip + steer/prompt/abort/kill +
broadcast, `agent://`/`history://`, **ACP** (host eksternal+lokal). Sisa: klien ACP
(agen eksternal sebagai anak).

### 8.3 Yang harus dibangun (bertahap)

| Tahap | Isi | Kriteria |
|---|---|---|
| A. Subagent lokal | tool `subagent` (spawn dengan prompt+tool set), registry dalam sesi, artefak sesi anak dengan `parent_session`, hasil kembali ke induk | ✅ tahap A (crate `tui-subagent`; tanpa artefak file — tahap B); test spawn+tool+gagal |
| B. Kontrol | steering pesan, abort anak, park/revive, kill; lineage | test: steer saat anak berjalan; kill membersihkan proses; resume memulihkan "parked" |
| C. Tampilan | panel/daftar subagent di TUI (mirip mini Agent Hub), buka transkrip anak | test model layar + navigasi |
| D. Antar-instance | pesan antar-sesi/antar-proses (butuh transport, lihat §6) atau ACP | keputusan protokol; test dua proses |

Prasyarat: definisi "agent-to-agent" yang dipilih — **internal subagent** (paling
murah, tanpa jaringan) atau **protokol antar-proses** (ACP/collab/custom). Keduanya
berbeda biaya dan risikonya; jangan dicampur dalam satu kartu.

---

## 9. Peta jalan (urutan pembangunan)

| Fase | Isi | Ukuran | Prasyarat |
|---|---|---|---|
| P0 — v1 Windows | tutup UAT/visual/live gateway; tidak ada permukaan baru | — | kunci + waktu iqbal |
| P1 — integrasi dasar | ✅ SELESAI: `tui-sdk`; RPC v2 + v1.1; host tools; host URI (wiring penuh); MCP stdio + Streamable HTTP + wiring | L | kartu RENCANA resmi bila diadakan |
| P2 — ekosistem | ✅ **SEBAGIAN BESAR SELESAI 2026-11-18**: API server lokal (`mx serve`), MCP http/sse + auth + notifikasi, subagent A–C; sisa: ACP | L–XL | P1 stabil ✅ |
| P3 — realtime & platform | 🟡 collab v1 (SSE push/polling) ✅; sisa: WS relay penuh, Linux/macOS verifikasi + CI + packaging | XL | keputusan produk & CI native |

Setiap item P1+ wajib: kartu RENCANA baru (jangan menyisipkan ke kartu v1), dependensi
dicatat di `DEPENDENCIES.md`, test perilaku + kasus gagal, dan `check.ps1`/`check.sh`
hijau sesuai platform target.

---

## 10. Keputusan yang dibutuhkan

1. **Platform**: apakah Linux/macOS native masuk scope produk, atau tetap Windows-only
   dengan pernyataan resmi? (mempengaruhi P3 dan CI)
2. **SDK**: dijanjikan stabil (semver) atau eksperimental?
3. **API server**: perlu daemon HTTP lokal atau cukup RPC? (mempengaruhi dependensi
   jaringan baru)
4. **MCP**: masuk Fase 2 atau tetap ⛔? (RENCANA §13 melarang sekarang)
5. **A2A**: subagent internal saja, atau protokol antar-proses/ACP?
6. **WebSocket/collab**: fitur produk atau tidak sama sekali?
7. **Distribusi**: biner per platform (butuh signing) atau source-only?

---

## 11. Rujukan

**omp 18.22.0** (`oh-my-pi-main/`):

- `README.md:464` — daftar platform natives (`linux-x64`, `linux-arm64`,
  `darwin-x64`, `darwin-arm64`, `win32-x64`, `win32-arm64`) + instalasi macOS/Linux/Windows
- `docs/sdk.md:1` — SDK in-process
- `docs/rpc.md:37` — protokol RPC v1/v2, framing, host tools/URIs, subagent
- `docs/mcp-protocol-transports.md:65`, `docs/mcp-config.md`,
  `docs/mcp-server-tool-authoring.md`, `docs/mcp-runtime-lifecycle.md` — MCP
- `docs/collab.md:102` — WebSocket collab relay
- `docs/agent-hub.md:1`, `docs/task-agent-discovery.md` — subagent/A2A internal
- `docs/extensions.md`, `docs/hooks.md` — API ekstensi
- `packages/coding-agent/src/modes/acp/acp-mode.ts` — ACP
- `sdk/{go,python,rust}/omp-rpc` — SDK RPC lintas bahasa

**Repo kita:**

- `docs/rpc-v1.md`, `docs/tools-v1.md:116`, `docs/config-v1.md`,
  `docs/skema-sesi-v1.md`, `docs/paritas.md`, `docs/uat-v1.md`
- `crates/tui-tools/src/lib.rs:772` (`kill_tree`), `crates/tui-wire/src/secure_file.rs:25`
  (ACL), `crates/tui-config/Cargo.toml:20` (keyring), `crates/tui-app/src/rpc_mode.rs:111`
  (`MAX_FRAME_BYTES`), `check.ps1`, `check.sh`
