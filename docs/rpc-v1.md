# Protokol RPC v1 (`maicli --mode rpc`)

Transport: satu objek JSON per baris. Klien menulis ke **stdin**, server menulis ke
**stdout**. stderr dipakai log/kesalahan proses, bukan protokol. Versi diumumkan di
frame `ready` sebagai `maicli-rpc/1`.

Mode ini adalah pengecualian terdokumentasi (stdout untuk protokol);
log tetap ke file lewat `tracing`.

## Alur

```
klien                                 server
  │  {"type":"ping","id":"1"}   →      │  {"type":"pong","id":"1"}
  │  {"type":"prompt","id":"2",…} →    │  {"type":"event","event":"agent_start"}
  │                                     │  {"type":"event","event":"text_delta","text":"…"}
  │  {"type":"abort"}            →      │  … turn berhenti, stop_reason=aborted
  │                                     │  {"type":"result","id":"2","stop_reason":"stop","turns":1}
  │  {"type":"shutdown"}         →      │  (server menutup)
```

Server mengirim `ready` lebih dulu, memuat `protocol` dan `model` awal.

## Permintaan

| `type` | Field | Arti |
|---|---|---|
| `prompt` | `id`, `message` | Jalankan satu turn. `result`/`error` memakai `id` yang sama |
| `abort` | `id?` | Batalkan turn berjalan. `ack` bila `id` diisi |
| `model` | `id`, `model` | Ganti model sesi, lalu `ack`. Bila daftar discovery tersedia, id divalidasi seperti `/model` di TUI/teks: tak dikenal → `E_MODEL_UNKNOWN`, model tidak berubah. Daftar kosong (discovery belum berhasil) meneruskan id apa adanya. |
| `ping` | `id` | `pong` |
| `usage` | `id` | `usage` berisi pemakaian token kumulatif |
| `answer` | `id?`, `text` | menjawab frame `question`; `text` kosong = dilewati |
| `approval` | `id`, `decision` | menjawab frame `approval_request`; `decision` = `once`, `session`, `always`, atau `deny` |
| `shutdown` | — | Tutup server dengan rapi |

Field tambahan diabaikan (klien baru vs server lama); `type` tak dikenal ditolak
dengan `{"type":"error","code":"E_PROTOCOL"}` tanpa menghentikan server.

Selama turn berjalan: `abort`, `ping`, `shutdown`, **`approval`**, dan **`answer`** tetap
dilayani — tanpa itu approval/pertanyaan tidak bisa dijawab saat agen berjalan.
`prompt` dan `model` dijawab `{"type":"error","code":"E_AGENT_BUSY"}`. `model` juga ditolak `E_MODEL_UNKNOWN` bila id tidak ada di daftar discovery yang sudah diketahui.

## Frame server

| `type` | Field |
|---|---|
| `ready` | `protocol`, `model` |
| `pong` | `id` |
| `ack` | `id` |
| `question` | `question` — dikirim saat tool `ask` menunggu jawaban; balas dengan `answer` |
| `approval_request` | `id`, `tool`, `tier` (`read`/`write`/`exec`), `subject` — balas dengan `approval` |
| `usage` | `id`, `input_tokens`, `output_tokens`, `cached_tokens` |
| `event` | `event` (objek, lihat bawah) |
| `result` | `id`, `stop_reason` (`stop`/`tool_use`/`length`/`error`/`aborted`), `turns` |
| `prompt_result` | `id`, `agent_invoked`, `status` (`ok`/`aborted`/`error`), `stop_reason?`, `turns?`, `error?` | status akhir satu prompt; dikirim setelah `result` (v1.1, lihat `docs/rpc-v2.md` §3a) |
| `session_settled` | — | sesi diam, siap prompt berikutnya; dikirim setelah `prompt_result` |
| `error` | `id?`, `code`, `message` |

## Event

| `event` | Field |
|---|---|
| `agent_start` | — |
| `turn_start` | `index` |
| `message_start` | — |
| `text_delta` | `text` |
| `text_end` | — |
| `tool_start` | `tool_call_id`, `tool_name`, `preview` |
| `tool_end` | `tool_call_id`, `tool_name`, `is_error`, `preview` |
| `turn_end` | `index` |
| `agent_end` | `turns` |
| `notice` | `text` |

Event yang tidak punya padanan stabil (mis. `message_end` dengan objek pesan penuh,
`thinking_delta`) sengaja tidak dikirim; klien yang butuh isi lengkap memakai
`result` + file sesi.

## Approval di mode RPC

Kebijakan tetap dari config (`approval.mode`, `approval.tools`, `approval.restricted_folders`).
Permintaan yang menurut kebijakan perlu manusia dikirim sebagai frame `approval_request` dengan
`id`; klien membalas `{"type":"approval","id":"ap0","decision":"once"}`. Keputusan `deny`
menolak tool; permintaan yang tidak dijawab dalam `limits.approval_timeout_secs` (default 600
detik) otomatis ditolak, dan `abort`/`shutdown` memutus penantian dengan cara yang sama.
Dengan default `always-ask`, klien yang tidak menjawab akan memblokir tool tulis/exec sampai
timeout — memang itu tujuannya.

## Batas frame

Satu baris (frame) dibatasi **1 MiB** (`MAX_FRAME_BYTES`). Frame yang lebih besar dibuang tanpa
disimpan ke memori, server membalas `E_PROTOCOL` "frame terlalu besar: N byte, maksimum 1048576",
lalu melanjutkan membaca frame berikutnya. Galat JSON hanya menyertakan pratinjau 200 karakter
supaya frame rusak tidak memperbesar pesan galat.

## Belum ada di v1

- Negosiasi kapabilitas dan framing terfragmentasi: **sejak 2026-11-18 tersedia
  lewat protokol v2** (`docs/rpc-v2.md`): klien mengirim `negotiate_protocol`,
  server memecah frame besar menjadi `rpc_chunk` (batas fisik tetap 1 MiB,
  reassembly sampai 64 MiB). v1 tetap default dan kompatibel; `ready` kini juga
  memuat `supported_protocol_versions` + `max_reassembled_frame_bytes`
  (field tambahan, klien lama mengabaikannya).
- Batas waktu menjawab `approval_request` oleh klien **sudah ada** (mengikuti `limits.approval_timeout_secs`, deny otomatis); yang belum: negosiasi batas waktu per-request oleh klien.
- Fitur protokol lain yang belum ada (host tools/URIs, `prompt_result` terpisah,
  extension UI, subagent, sesi/queue) tercatat di `docs/rpc-v2.md` §6 dan
  `docs/platform-dan-integrasi.md` §5.
