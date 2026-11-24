<div align="center">

# ModeluXAI TUI — `maicli`

**Agen AI untuk terminal.** Multi-provider, jalan di Windows, Linux, dan
macOS. Satu biner Rust; tidak butuh runtime Node.

[![CI](https://img.shields.io/badge/CI-Linux%20%7C%20Windows-blue)](.github/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange)](rust-toolchain.toml)

</div>

`maicli` adalah agen pemrograman yang bekerja di terminal. Kamu memberi tugas, ia
membaca dan menulis kode, menjalankan perintah, lalu melapor. Setiap akses
berkas dan eksekusi melewati approval gate yang bisa kamu atur.

## Fitur

- Enam mode: `tui` (terminal penuh), `text` (REPL untuk SSH/pipe), `rpc`
  (otomasi, protokol v1+v2), `serve` (API HTTP lokal + SSE + broker OAuth),
  `relay` (collab WebSocket + klien web/link/QR), dan `acp` (Agent Client
  Protocol, server + klien agen eksternal)
- Agent Hub (`Alt+A`): kelola subagent dari panel; lihat event dan
  transkrip, kirim steer/prompt, abort/kill, atau broadcast ke semua anak
- Orchestrator: satu perintah mem-spawn hingga 8 subagent (paralel atau
  berurutan) dan merangkum hasilnya
- Skill: folder `skills/<nama>/SKILL.md` otomatis masuk konteks
- MCP: server MCP (stdio, Streamable HTTP, atau legacy SSE) muncul sebagai
  tool; notifikasi long-lived dengan reconnect otomatis
- Katalog provider (opsional): muat katalog omp (98 provider) dan pilih lewat
  UI `/catalog`
- Extension/plugin: folder `extensions/<nama>/extension.json` jadi tool proses
  ter-sandbox; env dibersihkan dan approval wajib; `/plugins list|reload`
- Tool tambahan: `lsp`, `debug` (DAP), `ast_grep`/`ast_edit`, `hashline_edit`,
  `fetch`, `web_search`, `github`/`pr://`, `find`, `security_scan`, `eval`
  (Python/Bun), `browser` (CDP), `media` (TTS/gambar), `wait`, `stage_write`
- Keamanan: kunci API hanya lewat env var atau keyring OS (Windows Credential
  Manager, Linux Secret Service, macOS Keychain); file sensitif mode-600/ACL;
  proses anak tidak mewarisi kredensial; kill pohon proses di semua platform
- Setup wizard: pertama kali dijalankan tanpa kunci, wizard memandu pilih
  provider, isi kunci, dan pilih model

## Instalasi

**Satu paket — CLI + TUI menyatu** (model distribusi omp, tanpa Electron/Tauri):

```bash
npm install -g @modeluxai/cli
maicli                       # satu perintah, langsung TUI terminal
```

Satu perintah `npm install -g` sudah mencakup biner platform yang dipilih otomatis
melalui `optionalDependencies` (Linux x64/arm64, macOS x64/arm64, Windows x64/arm64).
Tidak perlu installer GUI terpisah; tidak ada Chromium/webview.

A. Atau unduh biner dari halaman Releases GitHub:

```bash
# Linux x64 (pola: maicli-<platform>-vX.Y.Z.tar.gz)
sha256sum -c maicli-linux-x64-v0.4.3.tar.gz.sha256
tar -xzf maicli-linux-x64-v0.4.3.tar.gz
sudo mv maicli-linux-x64/maicli /usr/local/bin/
```

Tersedia: Linux x64/arm64, macOS arm64/x64, Windows x64/arm64 — setiap arsip berisi
`maicli`, `LICENSE`, `README.md`, `CHANGELOG.md`, dan file `.sha256`.

B. Atau build dari sumber (butuh Rust stable):

```bash
cargo install --path crates/tui-app --locked
# atau: cargo build --release -p tui-app --locked
```

## Mulai cepat

```bash
maicli                  # pertama kali: wizard setup
# ikuti langkahnya: pilih provider, isi kunci, pilih model

maicli                  # TUI penuh
maicli --mode text      # REPL teks (untuk SSH)
maicli --prompt "ringkas README ini" --mode text   # satu prompt, lalu keluar
maicli --mode serve     # API lokal: /api/health, /api/prompt, /api/events/stream
maicli --mode relay     # bagikan sesi via WebSocket (control/guest)
maicli --mode acp       # kendalikan dari host eksternal (Zed dsb.)
```

Perintah di dalam TUI: `/model`, `/provider add`, `/catalog`, `/compact`,
`/tree`, `/usage`, `/plan`, `/goal`, `/research`, `/watchdog`, `/staging`,
`/accept`, `/artifact`, `/context-note`, `/skill-new`, `/hooks` (via config).
Tekan `F1` untuk daftar lengkap. Kelola subagent dengan `Alt+A`.

SDK: RPC Go/Python/TypeScript di `sdk/` (lihat `docs/sdk-lintas-bahasa.md`);
crate Rust `tui-sdk` untuk embed in-process (lihat `docs/sdk-rust-v1.md`).

## Arsitektur (15 crate)

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
tui-memory    memori persisten (jsonl/markdown/sqlite + embedding lokal)
tui-computer  kontrol desktop (screenshot/input)
tui-core      sesi agent (turn, tools, hooks)
tui-sdk       embedding Rust (SessionBuilder, AgentRegistry)
tui-app       biner `maicli` (TUI/text/rpc/serve/relay/acp)
```

## Kontribusi

Lihat [CONTRIBUTING.md](CONTRIBUTING.md). PR wajib melewati required checks
(fmt, clippy Linux/Windows, test Linux/Windows, build release, deny, MSRV 1.89,
judul Conventional, dependency review) sebelum bisa di-merge.

## Rilis

Versioning otomatis dari Conventional Commits lewat **release-plz**: PR rilis →
tag `vX.Y.Z` → 6 biner + SHA256 di GitHub Release. Runbook lengkap dan aturan
branch protection: [docs/setup-github.md](docs/setup-github.md).

## Keamanan

Laporkan kerentanan secara privat lewat [SECURITY.md](SECURITY.md).

## Lisensi

[MIT](LICENSE), © 2026 iqbal (ModeluXAI)
