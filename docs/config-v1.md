# Skema config v1 (`config.toml`)

Dokumen ini adalah rujukan skema config yang dipakai `maicli`. Semua nilai dibaca
dari TOML; **`config.yml` milik omp tidak dibaca**.

## Lokasi file

| Lapisan | Path | Prioritas |
|---|---|---|
| default | nilai bawaan di kode | terendah |
| global | `%APPDATA%\ModeluXAI\TUI\config.toml` | menimpa default |
| proyek | `<cwd>/.maicli/config.toml` | menimpa global |
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
`maicli-modeluxai`, user = nama env var). `maicli config key-import` menyalin nilai dari
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
- `maicli config show` menampilkan `mcp.timeout_secs` dan daftar nama server (tanpa
  nilai env).

### `[memory]`

Memori persisten lintas sesi; backend dipilih pengguna. Tool `memory` (tier
Write) dan command `/memory`/`/mm` memakai bagian ini. Injeksi ke system prompt
dibatasi `inject_chars` dan hanya bila `enabled`.

```toml
[memory]
enabled = true
backend = "jsonl"        # jsonl | markdown | sqlite
path = "memory"          # relatif terhadap cwd proyek
embedding = "hash"       # hash (lokal, tanpa model) | none
max_entries = 500
inject_chars = 2000
inject_mental_models = true
auto_refresh_models = false   # refresh semua mental model berkala
auto_refresh_secs = 300       # minimal 30 saat auto_refresh aktif
```

| Kunci | Tipe | Default | Arti |
|---|---|---|---|
| `enabled` | boolean | `true` | aktifkan injeksi + command memori |
| `backend` | string | `jsonl` | `jsonl` (`notes.jsonl`), `markdown` (`MEMORY.md`), `sqlite` (`memory.db`) |
| `path` | string | `memory` | folder memori; path relatif dihitung dari cwd |
| `embedding` | string | `hash` | embedding lokal hash n-gram 384-d (deterministik, tanpa jaringan) atau `none` (keyword) |
| `max_entries` | integer | 500 | entry tertua dipangkas saat melebihi |
| `inject_chars` | integer | 2000 | batas karakter blok memori di system prompt |
| `inject_mental_models` | boolean | `true` | sertakan ringkasan mental model |

### `[marketplace]`

```toml
[marketplace]
enabled = true
index_url = "https://contoh/index.json"   # untuk /marketplace search
allow_insecure = false                    # izinkan http/file/direktori lokal
```

Extension dipasang ke `<config_root>/extensions/<nama>` lewat `git clone
--depth 1`, divalidasi dengan manifest ketat yang sama dengan loader, lalu
dipakai setelah `/plugins reload` atau restart. `allow_insecure` hanya untuk
pengembangan (repo lokal).

### `[ssh]`

Host bernama untuk tool `ssh` (tier Exec). `target` wajib `user@host` (tanpa
spasi), tanpa password interaktif (`BatchMode=yes`).

```toml
[ssh.hosts.prod]
target = "deploy@192.0.2.10"
port = 22
identity = ""        # kosong = ssh-agent/default
timeout_secs = 60
```

### `[advisor]`, `[presets.*]`, `[collab]`, `[acp_client]`

```toml
[advisor]
enabled = false
provider = ""        # kosong = provider sesi (gateway kedua bila diisi)
model = ""           # kosong = model sesi
api_key_env = ""     # kosong = env provider sesi

[presets.kerja]
provider = "modeluxai"
model = "deepseek-v4.1-flash"
effort = "  "        # kosong = default

[ui.keybindings]
palette = "ctrl+p"   # default ctrl+k
help = "f1"
cancel = "ctrl+c"
clear_input = "ctrl+u"
tool_details = "ctrl+e"
hub = "alt+a"

[collab]
port = 7475          # relay WS in-TUI (/collab, /live)
remote_url = ""      # wss://… relay remote; dipakai /collab join <room> <kunci>

[acp_client]
enabled = false      # daftarkan tool `acp_subagent`
command = ""         # perintah agen ACP eksternal (kosong = tidak aktif)
args = ["--stdio"]
timeout_secs = 120
```

### `[media]`

Penyedia TTS/generasi gambar eksternal. Prompt dikirim di stdin, path berkas
keluaran menjadi argumen terakhir; env kredensial dibuang, timeout 30 dtk.

```toml
[media]
tts_command = ["python3", "tts.py"]
image_command = ["python3", "image.py"]
```

### `[hooks]`

Hook eksternal dijalankan lewat `/hook-test <event>`. Konteks JSON dikirim ke
stdin; timeout 5 dtk; output dipotong 2 KiB. Perintah berasal dari config
pengguna, bukan dari model.

```toml
[hooks]
before_model = ["python3 /path/ke/hook.py"]
after_tool = []
before_tool = []
on_stop = []
[hooks.magic_keywords]
urgent = "Jawab ringkas dan prioritaskan langkah aman."
# [hooks]
# ttsr_rules = ["rm -rf", "DROP TABLE"]
```

### `[oauth.broker.<nama>]` (mode `serve`)

Broker OAuth 2.1 authorization-code + PKCE untuk API server lokal. Token hanya
disimpan di memori proses `serve`, tidak ditulis ke disk. Endpoint:
`GET /api/oauth/start?provider=<nama>`, `GET /api/oauth/callback?code=…&state=…`,
`GET /api/oauth/status?provider=<nama>`. Semua endpoint tetap memakai token serve.

```toml
[oauth.broker.github]
client_id = "..."
client_secret_env = "GITHUB_CLIENT_SECRET"  # kosong = client publik
authorize_url = "https://github.com/login/oauth/authorize"
token_url = "https://github.com/login/oauth/access_token"
redirect_uri = "http://127.0.0.1:7475/api/oauth/callback"
scopes = ["repo", "read:user"]
```

### `[mcp.servers.<nama>.oauth]`

OAuth 2.1 authorization-code + PKCE untuk server MCP berbasis `url`. Token
disimpan di `<config_root>/mcp-oauth/<nama>.json` (0600) dan di-refresh otomatis;
tanpa token server dilewati sampai `/mcp reauth <nama>` berhasil.

```toml
[mcp.servers.jira]
url = "https://jira.contoh/mcp"
transport = "http"

[mcp.servers.jira.oauth]
client_id = "maicli"
authorization_url = ""   # kosong = discovery /.well-known/oauth-authorization-server
token_url = ""
scopes = ["mcp"]
redirect_port = 0        # 0 = port loopback acak
```

## Perintah terkait
- `maicli config show` — config **efektif** + asal tiap nilai (`[default]`,
  `[file global]`, `[file proyek]`), menyebut **sumber** kunci API
  (`env`/`keyring`/`tidak ada`) tanpa pernah menampilkan nilainya.
- `maicli config path` — path file global dan proyek.
- `maicli config set <key> <value>` — tulis satu nilai ke file global secara atomik
  (key lain dipertahankan); nilai divalidasi sebelum file disentuh.
- `maicli config key-import` — simpan kunci dari env var ke keyring.
- `maicli --set bagian.kunci=nilai` — override sekali jalan (tidak menulis file); nilai divalidasi,
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
test `acl_restricted`, bukan diasumsikan.

`MAICLI_DATA_DIR` (env) menimpa seluruh direktori data (config/sesi/log/cache) — dipakai instalasi portabel dan test E2E terisolasi; tidak mengubah isi berkas.\n\nSemua path dihitung dari `directories` (bukan path hardcode) dan relatif ke
direktori kerja sesi untuk bagian proyek.
