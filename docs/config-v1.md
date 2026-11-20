# Skema config v1 (`config.toml`)

Dokumen ini adalah rujukan skema config yang dipakai `mx`. Semua nilai dibaca
dari TOML; **`config.yml` milik omp tidak dibaca** (RENCANA C-11).

## Lokasi file

| Lapisan | Path | Prioritas |
|---|---|---|
| default | nilai bawaan di kode | terendah |
| global | `%APPDATA%\ModeluXAI\TUI\config.toml` | menimpa default |
| proyek | `<cwd>/.mx/config.toml` | menimpa global |
| runtime | flag CLI: `--set bagian.kunci=nilai` (boleh diulang) dan `--model` | tertinggi |

Tidak ada file-watching: **perubahan config butuh restart** aplikasi agar
berlaku (keputusan v1, dicatat sesuai kriteria C-11(5)).

## Bagian dan kunci

Semua bagian memakai `deny_unknown_fields`: kunci yang salah ketik ditolak
dengan pesan yang menyebut file, baris, dan nama kunci.

### `[gateway]`

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `provider` | string | `modeluxai` | provider aktif (id registry); daftar lengkap via `/provider` di TUI atau `tui_wire::providers::all()` |
| `base_url` | string | `https://api.modeluxai.com/v1` | ujung OpenAI-compatible; otomatis mengikuti `provider` bila tidak diisi sendiri |
| `api_key_env` | string | `MODELUXAI_API_KEY` | nama env var / user keyring |

Kunci API tidak pernah disimpan di file ini. Urutan pencarian: **environment
variable** lalu **keyring OS** (Windows Credential Manager, service
`mx-modeluxai`, user = nama env var). `mx config key-import` menyalin nilai dari
env var ke keyring tanpa mencetaknya.

### `[models]`

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `default` | string | `deepseek-v4.1-flash` | model sesi baru |
| `compact` | string | `deepseek-v4.1-flash` | model untuk ringkasan konteks |

Saat melanjutkan sesi, urutan model: `--model` > `ModelChange` terakhir di sesi
> `models.default`.

### `[approval]`

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `mode` | string | `always-ask` | `always-ask` / `always-ask-cached` / `write` / `yolo` |
| `restricted_folders` | array path | kosong | folder yang selalu meminta persetujuan (mengalahkan grant) |
| `tools` | map nama→string | kosong | kebijakan per tool: `allow` / `deny` / `prompt` |
| `grant_ttl_hours` | integer | `0` | masa berlaku grant `always` dalam jam; `0` = tidak kedaluwarsa. Grant basi diabaikan dan dipangkas saat start. |

Grant `Always` disimpan di `%APPDATA%\ModeluXAI\TUI\grants.json` dan bisa
dihapus dengan `/forget-approvals`.

### `[limits]`

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `turn_deadline_secs` | integer | 900 | deadline satu turn |
| `approval_timeout_secs` | integer | 600 | batas menunggu keputusan approval |
| `bash_timeout_secs` | integer | 120 | timeout default tool `bash` |
| `bash_max_timeout_secs` | integer | 600 | batas atas timeout `bash` |
| `tool_output_cap_bytes` | integer | 65536 | cap output tool sebelum ditandai terpotong |
| `context_window_tokens` | integer | 128000 | ukuran window konteks untuk pemicu compaction (reserve default 16384) |

Nilai nol/aneh di-clamp oleh `Budget` supaya tidak membuat perhitungan konteks
negatif; lihat `crates/tui-context/src/budget.rs`.

### `[ui]`

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `accent` | string `#rrggbb`/`#rgb` | `#d7a05b` | warna status/approval |
| `dim` | string `#rrggbb`/`#rgb` | `#8a8f98` | warna teks sekunder |

Warna yang tidak valid ditolak saat memuat config (bukan diam-diam diabaikan).

### `[mcp]`

Server MCP dijalankan saat boot (bukan per turn); perubahan config butuh restart.

```toml
[mcp]
timeout_secs = 30

[mcp.servers.fs]
command = "python"
args = ["-m", "mcp_server_fs"]
timeout_secs = 20          # opsional; menimpa mcp.timeout_secs
enabled = true             # default true

[mcp.servers.fs.env]
ROOT = "C:\\kerja"
```

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `timeout_secs` | integer | 30 | timeout default satu request MCP |
| `servers.<nama>.command` | string | wajib | perintah yang dijalankan |
| `servers.<nama>.args` | array string | kosong | argumen perintah |
| `servers.<nama>.env` | map string→string | kosong | environment tambahan (kredensial gateway tidak diwariskan) |
| `servers.<nama>.timeout_secs` | integer | ikut `mcp.timeout_secs` | timeout server ini |
| `servers.<nama>.enabled` | boolean | `true` | `false` = tidak dijalankan |

Perilaku:

- Tool server terdaftar sebagai `mcp.<server>.<tool>`, tier **exec** (selalu lewat
  gate approval), `Concurrency::Exclusive`.
- Server yang gagal dijalankan **tidak** menggagalkan aplikasi: sesi berjalan
  dengan tool yang tersisa dan pengguna melihat catatan `[mcp] server '<nama>'
  tidak terhubung: …` (TUI/teks) atau event `notice` (RPC).
- `command` kosong ditolak saat memuat config; field asing ditolak
  (`deny_unknown_fields`).
- `mx config show` menampilkan `mcp.timeout_secs` dan daftar nama server (tanpa
  nilai env).

## Perintah terkait

- `mx config show` — config **efektif** + asal tiap nilai (`[default]`,
  `[file global]`, `[file proyek]`), menyebut **sumber** kunci API
  (`env`/`keyring`/`tidak ada`) tanpa pernah menampilkan nilainya.
- `mx config path` — path file global dan proyek.
- `mx config set <key> <value>` — tulis satu nilai ke file global secara atomik
  (key lain dipertahankan); nilai divalidasi sebelum file disentuh.
- `mx config key-import` — simpan kunci dari env var ke keyring.
- `mx --set bagian.kunci=nilai` — override sekali jalan (tidak menulis file); nilai divalidasi,
  dan `config show` menandainya sebagai `<runtime --set>`.

## Lokasi lain yang dipakai

| Isi | Path |
|---|---|
| sesi | `%APPDATA%\ModeluXAI\TUI\sessions\*.jsonl` |
| lock sesi | `<file sesi>.jsonl.lock` |
| grant approval | `%APPDATA%\ModeluXAI\TUI\grants.json` (`expires_at_ms` opsional per grant; file lama tanpa field ini tetap berlaku) |
| log | `%APPDATA%\ModeluXAI\TUI\logs\` |
| cache daftar model | `%APPDATA%\ModeluXAI\TUI\models-cache.json` (TTL 24 jam) |

Berkas config, grant, dan sesi dibatasi ACL-nya ke pengguna saat ini
(`icacls /inheritance:r /grant:r <user>:F`; warisan ACL dimatikan) — dibuktikan
test `acl_restricted`, bukan diasumsikan (RENCANA §10 butir 9).

`MX_DATA_DIR` (env) menimpa seluruh direktori data (config/sesi/log/cache) — dipakai instalasi portabel dan test E2E terisolasi; tidak mengubah isi berkas.\n\nSemua path dihitung dari `directories` (bukan path hardcode) dan relatif ke
direktori kerja sesi untuk bagian proyek.
