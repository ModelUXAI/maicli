# Protokol RPC v2 (`mx --mode rpc`) — transport chunk

Status: **DONE (transport + prompt_result/settled)** — negosiasi versi, chunking frame besar, dan semantik akhir prompt. Fitur
protokol lanjutan yang belum ada (extension UI, subagent, sesi/queue) tercatat
sebagai peta jalan di `docs/platform-dan-integrasi.md` §5; host tools/URIs dan
`prompt_result` kini ada.

Dokumen ini melanjutkan `docs/rpc-v1.md`; v1 tetap **default** dan klien lama
tidak berubah perilakunya.

## 1. Negosiasi

Server mengumumkan dukungannya di frame `ready`:

```json
{
  "type": "ready",
  "protocol": "mx-rpc/1",
  "model": "…",
  "supported_protocol_versions": [1, 2],
  "max_reassembled_frame_bytes": 67108864
}
```

Klien yang ingin chunking mengirim:

```json
{"type":"negotiate_protocol","id":"p1","protocol_version":2}
```

Balasan sukses:

```json
{"type":"negotiated","id":"p1","protocol_version":2}
```

Balasan gagal (server tidak mendukung versi itu; koneksi tetap hidup, versi
tidak berubah):

```json
{"type":"error","id":"p1","code":"E_PROTOCOL","message":"versi protokol 9 tidak didukung; …"}
```

Aturan:

1. Sebelum negosiasi (atau bila klien tetap di v1), server berperilaku v1.
2. `negotiate_protocol` boleh dikirim kapan saja; versi yang tidak didukung tidak
   mengubah state.
3. Field lama di `ready` tetap sama; klien v1 yang mengabaikan field baru tetap
   kompatibel.

## 2. Batas transport

| Batas | Nilai | Catatan |
|---|---|---|
| Frame fisik (baris JSON) | **1 MiB** | termasuk newline; batas lama, tidak berubah |
| Frame logis v2 (setelah reassembly) | **64 MiB** | `max_reassembled_frame_bytes` |
| Frame masuk (klien → server) | 1 MiB | klien **wajib** memecah sendiri bila perlu; inbound tidak direassembly |

Frame keluar yang melebihi 1 MiB:

- **v1**: server membalas `{"type":"error","code":"E_PROTOCOL",…}` yang menyebut
  negosiasi v2 — tidak ada pemotongan data diam-diam;
- **v2**: server menulis urutan `rpc_chunk` (lihat bawah), tanpa memotong isi.

## 3. `rpc_chunk`

Satu frame logis dipecah menjadi `count` baris; setiap baris:

```json
{
  "type": "rpc_chunk",
  "chunk_id": "rpc-7",
  "index": 0,
  "count": 3,
  "byte_length": 1600042,
  "data": "<base64 potongan UTF-8 JSON>"
}
```

- `chunk_id` unik selama proses; `index` mulai 0 dan berurutan; semua potongan
  satu frame memakai `chunk_id` dan `byte_length` yang sama;
- `data` adalah potongan **byte UTF-8** dari JSON frame asli, di-base64;
- klien merekonstruksi: validasi metadata → gabung byte sesuai `index` →
  pastikan panjang = `byte_length` → decode UTF-8 ketat → parse JSON satu objek.

Kesalahan yang **wajib** ditolak klien (server tidak mengirimnya):

| Kasus | Perilaku |
|---|---|
| `index` pertama bukan 0 | error protokol |
| indeks tidak berurutan / chunk terputus | error protokol |
| `chunk_id`/`count`/`byte_length` berubah di tengah urutan | error protokol |
| frame biasa datang di tengah urutan chunk | error protokol |
| base64 tidak valid | error protokol |
| panjang gabungan ≠ `byte_length` | error protokol |
| UTF-8/JSON hasil gabungan tidak valid | error protokol |

Implementasi referensi Rust tersedia di `crates/tui-wire/src/rpc.rs`:
`RpcFrameDecoder::push_line`.

## 3a. `prompt_result` dan `session_settled` (v1.1)

Setelah satu `prompt` selesai, server kini mengirim **tiga frame berurutan**:

1. `result` (seperti v1) — `{type, id, stop_reason, turns}`;
2. `prompt_result` — `{type:"prompt_result", id, agent_invoked, status,
   stop_reason, turns, error}`:
   - `status`: `ok` / `aborted` / `error`;
   - `agent_invoked`: `true` (v1 kita selalu menjalankan turn; `false` disiapkan
     untuk prompt yang selesai lokal tanpa turn, mis. command);
   - `stop_reason`/`turns`: bila turn pernah berjalan;
   - `error`: pesan kesalahan bila `status="error"`.
3. `session_settled` — `{type:"session_settled"}`: sesi diam, siap prompt berikutnya.

Aturan: `prompt_result` selalu **setelah** `result` dengan `id` sama, dan
`session_settled` selalu terakhir — klien boleh mengandalkan urutan ini untuk
menandai akhir satu prompt. Klien lama yang hanya membaca `result` tidak terganggu.

## 3b. Host tools (v2)

Host bisa mendaftarkan tool miliknya sendiri tanpa MCP server:

```json
{"type":"set_host_tools","id":"h1","tools":[
  {"name":"echo_host","description":"balas argumen","schema":{"type":"object"}}
]}
```

- balasan sukses: `ack` + `notice` berisi daftar tool aktif;
- nama duplikat atau kosong → `E_PROTOCOL`, registry tidak berubah;
- mengirim ulang `set_host_tools` **mengganti** set sebelumnya (tool host yang
  tidak dikirim lagi dihapus; tool agent/MCP tidak tersentuh);
- `tools: []` menghapus semua tool host.

Saat model memanggil tool host, server meminta host mengeksekusinya:

```json
{"type":"host_tool_call","id":"host-1","toolCallId":"","toolName":"echo_host","arguments":{...}}
```

Host menjawab (saat idle **atau** di tengah turn):

```json
{"type":"host_tool_result","id":"host-1","is_error":false,"content":"…"}
```

Aturan: tool host bernama `host.<nama>` di registry, tier **exec** (selalu lewat
gate approval), `Concurrency::Exclusive`; hasil yang tidak cocok dengan
permintaan aktif → `E_PROTOCOL`; host yang tidak menjawab dalam 120 detik
menghasilkan hasil error ke model, turn tidak menggantung. `host_tool_cancel`
kini **aktif**: `abort`/`shutdown` (idle maupun di tengah turn) mengosongkan
penantian — penunggu menerima hasil error "dibatalkan: turn dihentikan", host
menerima frame `host_tool_cancel { target_id }`. Sama untuk `host_uri_cancel`
pada skema URI. Test `batalkan_host_tools_mengirim_cancel_dan_melepas_penunggu`,
`batalkan_host_uri_mengirim_cancel_dan_melepas_penunggu`.

## 3c. Host URI schemes (v2)

Host bisa memiliki skema URI sendiri ("berkas virtual"):

```json
{"type":"set_host_uri_schemes","id":"s1","schemes":[
  {"scheme":"db","description":"baris basis data","writable":true,"immutable":false}
]}
```

- nama skema dinormalisasi (trim + huruf kecil) dan harus cocok
  `[a-z][a-z0-9+.-]*`; nama tidak valid/duplikat → `E_PROTOCOL`;
- mengirim ulang **mengganti** set penuh; `schemes: []` menghapus semua;
- balasan sukses: `ack` + `notice` berisi daftar `skema://` aktif.

Pembacaan/penulisan URI skema host diteruskan ke host:

```json
{"type":"host_uri_request","id":"u1","operation":"read","url":"db://users/42"}
{"type":"host_uri_request","id":"u2","operation":"write","url":"db://users/42","content":"isi baru"}
```

Host menjawab (`content` = string sukses, atau objek `err` dengan pesan):

```json
{"type":"host_uri_result","id":"u1","content":"isi lama"}
{"type":"host_uri_result","id":"u2","content":{"err":"tabel terkunci"}}
```

Dilayani saat idle maupun di tengah turn; hasil tanpa permintaan aktif →
`E_PROTOCOL`; `host_uri_cancel` disiapkan (penghentian praktis lewat timeout
host tool/turn).

Wiring penuh sejak putaran berikutnya: `ToolContext.uri_bridge` (`tui_tools::UriResolver`) dipasang mode RPC dengan resolver yang meneruskan operasi lewat `host_uri_request`; tool `read`/`write` agent memeriksa `bridge.owns(url)` lebih dulu — URL skema host dipantulkan ke host, URL lain tetap jalur berkas. Timeout 120 detik. Test `tools::tests::read_dan_write_url_skema_host_dipantulkan_ke_bridge` (read, write, skema asing tetap error).

## 4. Prinsip yang dipegang

1. **Tidak ada pemotongan diam-diam.** Semua kegagalan menjadi frame/error
   bernama, bukan data hilang.
2. **Kompatibilitas v1.** Klien lama tetap bisa `prompt`/`abort`/`approval`.
3. **Batas ganda.** Frame fisik kecil (1 MiB) menjaga memori; satu frame logis
   besar tetap bisa dikirim dengan chunk sampai 64 MiB.
4. **Kontrak klien diuji.** Decoder menolak urutan salah, metadata berubah,
   base64 rusak, dan frame biasa di tengah chunk (lihat test di bawah).

## 5. Bukti

`cargo test -p tui-wire` (8 test baru) + `cargo test -p tui-app` (3 test baru):

- `negosiasi_protokol_dibaca` — parsing permintaan;
- `chunk_v2_bolak_balik_dengan_decoder` — frame 1,2 MB dipecah, tiap baris
  ≤ 1 MiB, decoder mengembalikan frame identik;
- `v1_menolak_frame_besar` — pesan error menyebut v2;
- `decoder_menolak_urutan_salah`, `decoder_menolak_frame_biasa_di_tengah_chunk`,
  `decoder_menolak_base64_rusak`, `decoder_menolak_metadata_chunk_berubah`;
- `ready_v2_bisa_dibaca_klien`;
- `negosiasi_v2_membuat_frame_besar_dipecah`, `negosiasi_versi_tak_dikenal_ditolak`,
  `frame_besar_pada_v1_menjadi_error_protokol` (sisi server `Out`).

## 6. Yang belum ada (dicatat, bukan diklaim)

| Fitur omp | Status kita |
|---|---|
| `prompt_result` terpisah + `session_settled` | ✅ selesai (setelah `result`, `id` sama; `session_settled` terakhir) |
| Host tools (`set_host_tools`, `host_tool_call`, `host_tool_result`) | ✅ selesai (tier exec, timeout 120 dtk); **`host_tool_cancel` aktif** (abort/shutdown membatalkan semua penunggu + frame cancel) |
| Host URIs (`set_host_uri_schemes`, `host_uri_request`/`result`) | ✅ selesai lengkap (protokol + wiring tool file via `UriResolver`) |
| Extension UI sub-protocol | belum (hanya `question` untuk tool `ask`) |
| Subagent frames | belum (subagent belum ada) |
| Sesi/queue commands (`get_state`, `open_session`, **antrean prompt** batas 8 saat turn berjalan; notice posisi; penuh → `E_AGENT_BUSY`) | ✅ selesai (frame `state`; buka sesi terbaru/`open_session` via `create_with_parent`-style open + lock dijaga) |
| Spool backpressure ke file | belum (kanal memori) |

## subagent_list (2026-11-19)

- Request `{"type":"subagent_list","id":…}` → frame `subagent_list {agents:[{id,status,turns,tool_calls,is_error}]}`;
- Read-only (polling); tool `subagent` kini aktif di mode RPC sehingga prompt induk bisa
  mem-spawn anak dan klien memantau statusnya.