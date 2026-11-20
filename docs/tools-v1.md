# Tool v1 — skema dan contoh (RENCANA C-04)

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

## Override

- `approval.tools.<nama>` di `docs/config-v1.md` menimpa perilaku per tool.
- `read`/`glob`/`grep`/`todo`/`ask` bertier `read`; `write`/`edit` bertier `write`;
  `bash` bertier `exec`.
## Metadata tampilan (`AgentTool`)

Selain skema wire di atas, setiap tool punya metadata UI (RENCANA C-04 `label`/`loadMode`/`render`)
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