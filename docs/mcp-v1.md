# MCP v1 (transport stdio) — `tui-mcp`

Status: **DONE (crate stdio+HTTP + bridge + wiring)** — dikerjakan 2026-11-18 sebagai item P1
`docs/platform-dan-integrasi.md` §7. Server MCP dikonfigurasi lewat `[mcp]` di
`config.toml` (lihat `docs/config-v1.md`) dan terhubung otomatis saat boot.

## Scope v1

| Bagian | Status |
|---|---|
| Transport **stdio** (spawn server, JSONL) | ✅ |
| `initialize` → `notifications/initialized` | ✅ |
| `tools/list` → `McpToolInfo { name, description, inputSchema }` | ✅ |
| `tools/call` → gabungan blok `text`, `isError` | ✅ |
| Request server `ping` / `roots/list` | ✅ (jawaban minimal) |
| **Notifikasi server → langganan** (`McpConnection::subscribe()`: progress/log) | ✅ (test `notifikasi_progress_diteruskan_ke_langganan`) |
| Request server lain | ditolak `-32601` (bukan menggantung) |
| Bridge ke `ToolRegistry` (`mcp.<server>.<tool>`, tier `exec`) | ✅ |
| Timeout per request (default 30 dtk, dapat diubah) | ✅ |
| Env kredensial tidak diwariskan ke server | ✅ (nama ber-`KEY`/`TOKEN`/`SECRET`/`PASSWORD` dibuang) |
| stderr server → ringkasan error (20 baris terakhir) | ✅ |
| Transport **Streamable HTTP** (`HttpTransport`, `Mcp-Session-Id`, respons JSON atau SSE) | ✅ (`connect_http`) |

## Pemakaian

```rust
use std::time::Duration;
use tui_mcp::{ServerSpec, connect_stdio};

let spec = ServerSpec::new("fs", "python")
    .arg("-m").arg("mcp_server_fs")
    .timeout(Duration::from_secs(20));

let koneksi = connect_stdio(spec).await?;
println!("{} tool dari {}", koneksi.tools().len(), koneksi.server());

// Langsung:
let hasil = koneksi.call_tool("read_file", serde_json::json!({"path": "a.txt"})).await?;

// Atau daftarkan ke registry agent:
let mut registry = tui_tools::ToolRegistry::v1();
koneksi.register_into(&mut registry)?;
```

Tool MCP yang didaftarkan memakai nama `mcp.<server>.<tool>`, mempertahankan
`inputSchema` server sebagai skema parameter model, dan bertier **exec** sehingga
**selalu melewati gate approval** (tidak ada jalur pintas). Embedder yang yakin
server itu read-only bisa memakai `into_tools_with_tier(ToolTier::Read)`.

## Keamanan

1. **Approval default exec** — tool MCP adalah kode pihak ketiga; default-nya
   butuh izin manusia setiap kali kebijakan mode memintanya.
2. **Env kredensial dibuang** — server tidak mewarisi `MODELUXAI_API_KEY` dsb.;
   env eksplisit tetap diteruskan lewat `ServerSpec::env`.
3. **stderr tidak dicetak ke TUI** — ringkasannya hanya masuk pesan error.
4. **Batas baris** 8 MiB per jawaban; pelanggaran menjadi `E_PROTOCOL`.
5. **Tidak ada approval bypass** — adapter memanggil client, dan pemanggilan tool
   selalu terjadi di dalam `run_tools` agent yang sudah melewati gate.

## Bukti

`cargo test -p tui-mcp` — 4 test (mock server MCP PowerShell, `cfg(windows)`):

1. `server_tidak_ada_dilaporkan` — spawn gagal → `E_TOOL_FAILED` yang menyebut
   perintahnya (lintas platform);
2. `handshake_list_dan_call_tool` — handshake, `tools/list` (echo + gagal),
   `tools/call` sukses & `isError: true` dipetakan benar;
3. `adapter_tool_berprefix_dan_bertier_exec` — nama `mcp.mock.echo`, tier `Exec`,
   `Concurrency::Exclusive`, eksekusi mengembalikan teks server;
4. `register_into_menolak_nama_bentrok` — nama bentrok ditolak `E_CONFIG`.

Mock server adalah skrip PowerShell sungguhan yang berbicara JSONL lewat
stdin/stdout, bukan stub Rust — jadi framing, handshake, dan korelasi id benar-benar
diuji. Test di-gate `cfg(windows)` karena skrip mock memakai PowerShell.

## Wiring aplikasi (selesai)

- **Config**: bagian `[mcp]` (`timeout_secs`, `servers.<nama>.{command,args,env,
  timeout_secs,enabled}`), tervalidasi ketat (`deny_unknown_fields`, `command`
  wajib) — lihat `docs/config-v1.md`.
- **Boot**: `mcp_boot::connect_all(&config)` dijalankan sekali di `main` sebelum
  mode apa pun; hasilnya (`mcp_tools`, `mcp_notes`) masuk `Boot`.
- **Registry**: `Boot::tool_registry()` = 8 tool v1 + tool MCP; dipakai semua
  mode (TUI/teks/RPC), termasuk `/new` dan `/fork`.
- **Tampilan**: TUI/teks mencetak `[mcp] <catatan>` per server (jumlah tool,
  gagal, nonaktif); RPC mengirim `notice`; label timeline tool MCP = `mcp`
  (`tool_meta`).
- **Kegagalan server tidak menjatuhkan aplikasi** — dicatat, sesi tetap jalan.

## Yang belum ada (jujur)

1. **Legacy HTTP+SSE** (protokol 2024-11-05) — hanya Streamable HTTP yang ada.
2. **OAuth/token refresh**, reconnect/backpressure otomatis.
3. **Notifikasi server kini bisa dilanggani** (`subscribe()`; test progress 50/100 diterima) — penerusan otomatis ke timeline TUI menyusul (butuh wiring di mode).
4. ~~E2E tool MCP lewat turn agent penuh~~ — **selesai 2026-11-18**:
   `e2e_mock_gateway::mode_teks_tool_mcp_lewat_turn_penuh` — mock gateway
   memerintahkan tool call `mcp.e2e.wap`, server MCP PowerShell sungguhan
   dijalankan lewat config proyek `[mcp.servers.e2e]`, tool dieksekusi (yolo),
   hasil "frasa-dari-mcp-e2e" kembali di jawaban model.

## Transport Streamable HTTP

`connect_http(url, nama, timeout)`: handshake `initialize` (header
`Mcp-Session-Id` disimpan bila server mengirim), `tools/list`, `tools/call`.
Respons server bisa JSON langsung atau **SSE** (`data:` per baris; `[DONE]`
dilewati; balasan dicocokkan berdasarkan `id`). Bukti:
`tests::http_handshake_list_dan_call` memakai mock server HTTP PowerShell
(`HttpListener`) yang benar-benar melayani `initialize`/`tools/list`/
`tools/call` + header sesi. Parsing jawaban (`parse_tools`/`parse_call`) dipakai
bersama stdio dan HTTP.

Langkah berikutnya (P2): legacy SSE, OAuth, reconnect; subagent.

## SSE legacy notifikasi (2026-11-19)

- Notifikasi (JSON-RPC tanpa id) di stream GET legacy kini diteruskan ke `subscribe()`;

- Notif yang datang sebelum langganan disimpan (retensi maks 100) dan diserahkan saat
  `subscribe()` dipanggil — tidak ada notif yang hilang di antara handshake dan langganan.