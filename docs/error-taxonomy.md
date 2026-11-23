# Taksonomi error

Ringkasan singkat; sumber utama `crates/tui-wire/src/error.rs` dan
`crates/tui-core`.

| Lapisan | Tipe | Catatan |
|---|---|---|
| Wire/tool | `MxError` + `ErrorCode` | kode stabil (`E_CONFIG`, `E_GATEWAY_AUTH`, `E_TOOL_FAILED`, …), pesan untuk pengguna berbahasa Indonesia |
| Tool | `ToolOutput::err` | kegagalan yang bisa dijelaskan ke model; bukan `MxError` |
| Config | `tui_config::LoadError`/`String` | parse TOML menyebut file+baris+kunci; validasi semantik mengembalikan pesan |
| Approval | `MxError` `E_APPROVAL_DENIED`/timeout | keputusan `Deny` bukan error infrastruktur |
| Session | `MxError` `E_SESSION_CORRUPT`/`E_SESSION_LOCKED` | file JSONL ketat (`deny_unknown_fields`), versi dicek |
| Provider | `GatewayError` | retry 429/5xx di client; auth error tidak di-retry |
| SDK | `MxError` | builder tanpa provider/approver → `E_CONFIG` |
| App | `anyhow`/`Result` di `main` | hanya binary; crate library dilarang `unwrap/expect/panic` |

Prinsip: error yang dilihat model = `ToolOutput::err`; error yang dilihat
operator = `MxError` dengan kode; jangan menelan error (log + pesan).
