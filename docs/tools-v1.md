# Tool v1 — skema dan contoh

Delapan tool yang teregistrasi di `ToolRegistry::v1()`. Semua panggilan melewati
gate approval C-05 (tier menentukan perilaku); `concurrency = shared` berarti tool
boleh berjalan bersamaan dengan tool shared lain dalam satu batch, sedangkan
`exclusive` berjalan sendiri (belum ada tool exclusive di v1).

Setiap hasil dibatasi `limits.tool_output_cap_bytes` (default 65.536 byte) dan
ditandai `[dipotong …]` bila melebihi. Path relatif dihitung dari direktori kerja
sesi; path yang keluar direktori kerja ditolak atau meminta approval eksplisit.

## `read` — tier `read`

Skema:

```json
{
  "type": "object",
  "properties": {
    "path":   { "type": "string", "description": "Path relatif terhadap direktori kerja" },
    "offset": { "type": "integer", "description": "Baris awal (1-based), default 1" },
    "limit":  { "type": "integer", "description": "Jumlah baris maksimum, default semua" }
  },
  "required": ["path"]
}
```

Contoh: `{"path":"crates/tui-core/src/agent.rs","offset":120,"limit":40}`

Kegagalan: berkas tidak ada, path traversal (`..` maupun `..\` gaya Windows), path absolut di luar direktori kerja, isi biner (ditolak dengan
pesan bernama), `offset` di luar isi (dikembalikan sebagai keterangan, bukan error
fatal).

## `write` — tier `write`

```json
{
  "type": "object",
  "properties": {
    "path":    { "type": "string" },
    "content": { "type": "string" }
  },
  "required": ["path", "content"]
}
```

Contoh: `{"path":"catatan/kerja.txt","content":"halo\n"}`

Membuat direktori induk bila belum ada; menulis penuh (bukan append).

## `edit` — tier `write`

```json
{
  "type": "object",
  "properties": {
    "path":        { "type": "string" },
    "old_string":  { "type": "string", "description": "Teks yang dicari, harus unik kecuali replace_all" },
    "new_string":  { "type": "string" },
    "replace_all": { "type": "boolean", "description": "Ganti semua kemunculan" }
  },
  "required": ["path", "old_string", "new_string"]
}
```

Contoh: `{"path":"a.txt","old_string":"lama","new_string":"baru"}`

Kegagalan: `old_string` kosong, teks tidak ditemukan, teks ambigu (>1 kemunculan
tanpa `replace_all`).

## `glob` — tier `read`

```json
{
  "type": "object",
  "properties": {
    "pattern": { "type": "string", "description": "Contoh: **/*.rs" }
  },
  "required": ["pattern"]
}
```

Contoh: `{"pattern":"crates/**/src/*.rs"}`

## `grep` — tier `read`

```json
{
  "type": "object",
  "properties": {
    "pattern":     { "type": "string" },
    "path":        { "type": "string", "description": "Subdirektori, default direktori kerja" },
    "max_results": { "type": "integer", "description": "Batas hasil, default 200, maksimum 1000" }
  },
  "required": ["pattern"]
}
```

Contoh: `{"pattern":"fn main|unwrap_used","path":"crates"}`

## `bash` — tier `exec`

```json
{
  "type": "object",
  "properties": {
    "command":    { "type": "string" },
    "timeout_ms": { "type": "integer", "description": "Opsional, milidetik (default limits.bash_timeout_secs × 1000), dibatasi limits.bash_max_timeout_secs" }
  },
  "required": ["command"]
}
```

Contoh: `{"command":"cargo test -p tui-wire","timeout_ms":120000}`

Shell yang dipakai: **Windows `cmd.exe /C`**, platform lain `/bin/sh -c`. Bila perlu sintaks PowerShell, panggil eksplisit `powershell -NoProfile -Command "<perintah>"`.\n\nSelalu lewat approval; timeout dibatasi `limits.bash_timeout_secs` (default 120)
dan maksimum `limits.bash_max_timeout_secs` (default 600); output dipotong.
Perintah yang gagal mengembalikan kode keluar + output parsial bertanda, bukan
panic.

## `todo` — tier `read`

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Daftar baru; kosongkan untuk melihat daftar saat ini"
    }
  }
}
```

Contoh: `{"items":["port SSE","uji approval"]}`

## `ask` — tier `read`

```json
{
  "type": "object",
  "properties": { "question": { "type": "string" } },
  "required": ["question"]
}
```

Contoh: `{"question":"Lanjut ke sesi berikutnya?"}`

Hanya berfungsi di sesi interaktif; di luar mode interaktif permintaan ditolak
dengan pesan bernama (tidak menggantung).

## Tool tambahan (v1.1, 2026-11-23)

Tool berikut terdaftar di `ToolRegistry::v1` atau didaftarkan kondisional oleh
`Boot::tool_registry()` saat config terkait aktif. Semua tetap melewati
approval gate sesuai tier.

| Tool | Tier | Aktif bila | Ringkas |
|---|---|---|---|
| `fetch` | read | selalu | URL http(s) → teks/markdown, cap 64 KiB |
| `ast_grep` | read | selalu | pola `$$nama`/`$$$nama`, mask string/komentar |
| `ast_edit` | write | selalu | proposal → `apply:true` |
| `hashline_edit` | write | selalu | ganti baris via anchor + hash |
| `security_scan` | read | selalu | 8 pola kredensial, nilai diredaksi |
| `web_search` | read | selalu | DuckDuckGo HTML / Brave API |
| `github` | read | selalu | PR/issue REST; `read pr://…`/`issue://…` |
| `lsp` | read | `[lsp] enabled` | diagnostics/references/definition/rename/hover |
| `debug` | exec | `[debug] enabled` | DAP: breakpoints/stack/evaluate/continue |
| `eval` | exec | selalu | kernel Python/Bun persisten per `kernel_id` |
| `media` | exec | `[media]` diisi | TTS/gambar via perintah eksternal |
| `browser` | exec | selalu | CDP HTTP tabs/new/close + WS evaluate |
| `wait` | read | subagent aktif | tunggu subagent selesai |
| `stage_write` | write | selalu | staging + `/accept` atomik |
| `skill` | read | skills ada | baca isi `SKILL.md` |
| `ssh` | exec | `[ssh] hosts` | exec remote BatchMode |
| `memory` | write | memory aktif | add/search/view/forget/stats/retain/recall/reflect/learn/edit |
| `acp_subagent` | exec | `[acp_client] enabled` | agen ACP eksternal |
| `computer` | exec | selalu | screenshot/input/windows/ax/clipboard/run |
| `report_tool_issue` | read | selalu | catat isu tool (JSONL, tampil via `/tool-issues`) |

Catatan: `glob`/`grep` memakai cache TTL 5 dtk (`ToolContext.scan_cache`) untuk hasil pemindaian; perubahan berkas baru terlihat setelah cache kedaluwarsa atau tool dipanggil dengan konteks baru.

Catatan: `task-worktree`, `find`, dan tool extension didaftarkan lewat
`Boot::tool_registry()`/mode; lihat dokumen roadmap internal (tidak dipublikasikan).

## Override

- `approval.tools.<nama>` di `docs/config-v1.md` menimpa perilaku per tool.
- `read`/`glob`/`grep`/`todo`/`ask` bertier `read`; `write`/`edit` bertier `write`;
  `bash` bertier `exec`.
## Metadata tampilan (`AgentTool`)

Selain skema wire di atas, setiap tool punya metadata UI (`label`/`loadMode`/`render`)
yang **tidak** dikirim ke gateway:

| Tool | Label | Render |
|---|---|---|
| `read` | baca | teks biasa (`→`) |
| `write` | tulis | teks biasa |
| `edit` | edit | diff (`±`) |
| `glob` | glob | teks biasa |
| `grep` | grep | teks biasa |
| `bash` | perintah | keluaran perintah (`❯`) |
| `todo` | todo | teks biasa |
| `ask` | tanya | teks biasa |

`loadMode` untuk seluruh tool v1 adalah `eager` (semua skema dikirim tiap panggilan); `deferred`
disiapkan sebagai kontrak untuk tool yang skemanya dimuat saat dipakai.