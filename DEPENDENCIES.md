# DEPENDENCIES.md

Setiap dependensi langsung, satu baris alasan, dan kartu penambahnnya.
Dependensi tanpa alasan = kartu tidak DONE (`AGENTS.md` §3).

## Runtime

| Crate | Versi | Alasan | Ditambah |
|---|---|---|---|
| `tokio` | 1.44 | runtime async + proses + time + `signal` (Ctrl+C mode teks) + `test-util` (jam dijeda untuk test konkurensi); satu-satunya runtime di pohon; juga `spawn_blocking` untuk menulis `grants.json` dari `tui-approval` | C-01, C-04, C-05 |
| `reqwest` | 0.12 (rustls) | klien HTTP gateway tanpa OpenSSL (build Windows bersih) | C-02 |
| `reqwest` (di `tui-mcp`) | 0.12 | transport Streamable HTTP MCP; satu-satunya klien HTTP di pohon (sudah dipakai `tui-provider`), rustls tanpa native TLS | P1/MCP |
| `serde` + `serde_json` | 1 | serialisasi pesan, sesi, wire gateway, file grant approval (`grants.json`), cache model (`models-cache.json`) | C-01, C-02, C-05 |
| `thiserror` | 2 | error bernama per crate (`AGENTS.md` §4) | C-01 |
| `anyhow` | 1 | hanya di `tui-app` (aturan §4) | C-01 |
| `async-trait` | 0.1 | trait `Provider`, `Tool`, `Approver`, `DecisionUi` | C-02 |
| `clap` | 4.5 | parsing CLI `mx` | C-01 |
| `toml` | 0.8 | config berlapis + pesan error berbaris | C-11 |
| `tracing` + `tracing-subscriber` + `tracing-appender` | 0.1/0.3/0.2 | log hanya ke file (`AGENTS.md` §5) | C-01 |
| `directories` | 6 | path config/sesi lintas platform tanpa hardcode | C-11 |
| `regex` | 1.11 | tool `grep` | C-04 |
| `globset` | 0.4 | tool `glob` | C-04 |
| `walkdir` | 2.5 | penelusuran direktori tool `glob`/`grep` | C-04 |
| `ratatui` + `crossterm` | 0.30 / 0.29 | TUI: layout, viewport, raw mode | C-09 |

## Pengembangan (bukan runtime)

| Crate | Alasan |
|---|---|
| `unicode-width` | 1 | lebar tampilan teks (CJK/emoji) untuk lipat baris TUI | C-09 |
| `keyring` | 3.6 | baca/tulis kunci API ke Windows Credential Manager (env var tetap prioritas) | C-11 |
| `cargo-deny` 0.21.0 (bukan crate) | gerbang lisensi/advisory/ban (`AGENTS.md` §5) |

## Catatan

- Tidak ada dependensi yang menelepon jaringan sendiri selain `reqwest` (dipakai eksplisit oleh
  `tui-provider`).
- Dua versi `base64`/`syn`/`windows-sys` di pohon berasal dari `reqwest` dan `ratatui` (warning
  `cargo deny`, bukan kegagalan).
- Crate `pi-*` dari omp **belum** dipakai; bila dipakai, butuh Rust nightly dan keputusan V-06 baru.
| `kdl = "6"` (tui-katalog) | parser KDL v2 untuk kompilator katalog omp (F-06) | 2026-11-18 |
- tui-tools: **serde** (2026-11-20) — deserialisasi manifest extension yang ketat (deny_unknown_fields).
- tui-app: **tokio-tungstenite** (2026-11-20) — server WebSocket untuk relay collab (room view/control; sesuai keputusan iqbal).
- tui-app: **futures-util** (2026-11-20) — StreamExt untuk stream pesan WebSocket (pasangan tokio-tungstenite).