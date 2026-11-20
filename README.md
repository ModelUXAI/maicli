<div align="center">

# ModeluXAI TUI — `mx`

**Agen AI terminal-first untuk pengembang.** Multi-provider, multi-platform,
dengan Agent Hub, orchestrator, dan seluruh pipa integrasi — dalam satu biner
Rust, tanpa runtime Node.

[![CI](https://img.shields.io/badge/CI-Linux%20%7C%20Windows-blue)](.github/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange)](rust-toolchain.toml)

</div>

---

## Apa ini

`mx` adalah agen pemrograman yang bekerja di terminal: kamu memberi tugas,
ia membaca/menulis kode, menjalankan perintah, dan melapor — setiap akses
berkas & eksekusi melewati **approval gate** yang bisa kamu kendalikan.

## Fitur

- **Lima mode**: `tui` (terminal penuh), `text` (REPL untuk SSH/pipe),
  `rpc` (otomasi; protokol v1+v2), `serve` (API HTTP lokal + SSE),
  `relay` (collab WebSocket), `acp` (protokol Agent Client — Zed dsb.)
- **Agent Hub** (`Alt+A`): kelola subagent secara interaktif — event live,
  transkrip, steer/prompt, abort/kill, broadcast ke semua anak
- **Orchestrator**: satu perintah mem-spawn hingga 8 subagent
  (paralel/berurutan) dan merangkum hasilnya
- **Skill**: folder `skills/<nama>/SKILL.md` otomatis masuk konteks
- **MCP**: server MCP (stdio / Streamable HTTP / legacy SSE) → jadi tool;
  notifikasi long-lived + reconnect otomatis
- **Katalog provider** (opsional): muat katalog omp (98 provider) — pilih via
  UI interaktif `/catalog`
- **Extension**: folder `extensions/<nama>/extension.json` → tool proses
  ter-sandbox (env dibersihkan, approval wajib)
- **Keamanan**: kunci API hanya env var / **keyring OS** (Windows Credential
  Manager, Linux Secret Service, macOS Keychain); file sensitif mode-600/ACL;
  proses anak tidak mewarisi kredensial; kill pohon proses di semua platform
- **Setup wizard**: pertama kali dijalankan tanpa kunci → wizard pilih
  provider → kunci → model — selesai

## Instalasi

**A. Unduh rilis** ([Releases](../../releases)):

```bash
# Linux x64
tar -xzf mx-v0.1.0-linux-x64.tar.gz && sudo mv mx /usr/local/bin/
```

**B. Build dari sumber** (butuh Rust stable):

```bash
cargo install --path crates/tui-app
```

## Mulai cepat

```bash
mx                  # pertama kali → wizard setup
# … ikuti langkahnya (pilih provider → kunci → model) …

mx                  # TUI penuh
mx --mode text      # REPL teks (untuk SSH)
mx --prompt "ringkas README ini" --mode text   # satu prompt → keluar
mx --mode serve     # API lokal: /api/health, /api/prompt, /api/events/stream
mx --mode relay     # bagikan sesi via WebSocket (control/guest)
mx --mode acp       # kendalikan dari host eksternal (Zed dsb.)
```

Perintah di dalam TUI: `/model`, `/provider add`, `/catalog`, `/compact`,
`/tree`, `/usage` — daftar lengkap di `F1`. Kelola subagent: `Alt+A`.

## Arsitektur (13 crate)

```
tui-wire      tipe protokol & klien gateway (OpenAI-compatible)
tui-context   anggaran konteks + compaction
tui-provider  streaming SSE, tool_call, retry
tui-session   sesi persisten + lineage (fork/tree)
tui-approval  kebijakan approval + grant + keyring
tui-config    config ketat (global/proyek/override)
tui-tools     tool inti + skills + extensions + katalog runner
tui-mcp       klien MCP (stdio/HTTP/SSE)
tui-subagent  manager + orchestrator (A–D)
tui-katalog   kompilator katalog KDL omp
tui-core      sesi agent (turn, tools, hooks)
tui-sdk       embedding Rust (SessionBuilder, AgentRegistry)
tui-app       biner `mx` (TUI/text/rpc/serve/relay/acp)
```

## Kontribusi

Lihat [CONTRIBUTING.md](CONTRIBUTING.md) — PR melewati gerbang yang sama
(fmt, clippy ketat, test, dependensi) di Linux **dan** Windows.

## Keamanan

Laporkan kerentanan secara privat — [SECURITY.md](SECURITY.md).

## Lisensi

[MIT](LICENSE) — © 2026 iqbal (ModeluXAI)
