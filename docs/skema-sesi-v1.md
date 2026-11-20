# Skema sesi JSONL v1

Berlaku untuk `crates/tui-session`. Ditulis 2026-11-17 (V-04). Perubahan bentuk = versi baru,
bukan edit diam-diam.

## Bentuk file

- Satu berkas per sesi: `<millis>_<id>.jsonl` di `<config>/ModeluxAI/TUI/sessions/`.
- Satu baris = satu objek JSON. Baris pertama **wajib** header `{"type":"session", …}`.
- Encoder menulis satu baris lalu `flush()` — baris yang sudah dianggap selesai tidak hilang
  saat proses mati mendadak.

```json
{"type":"session","version":1,"id":"18f2a…-0","title":"sesi mx","created_at":1763…,"cwd":"C:\\kerja"}
{"type":"session_init","id":"18f2a…-1","timestamp":1763…,"cwd":"C:\\kerja","app_version":"0.1.0"}
{"type":"message","id":"18f2a…-2","parent_id":null,"timestamp":1763…,"message":{"role":"user","content":"halo"}}
```

## Entry

| `type` | Field wajib | Masuk konteks model? |
|---|---|---|
| `session` | `version`,`id`,`title`,`created_at`,`cwd` (`parent_session` opsional) | tidak |
| `message` | `id`,`parent_id`,`timestamp`,`message` | **ya** |
| `model_usage` | `id`,`parent_id`,`timestamp`,`model`,`input_tokens`,`output_tokens` | tidak |
| `model_change` | `id`,`parent_id`,`timestamp`,`model` | tidak |
| `compaction` | `id`,`parent_id`,`timestamp`,`summary`,`replaced_messages` | tidak (ringkasan dipakai terpisah) |
| `title_change` | `id`,`timestamp`,`title` | tidak |
| `approval_decision` | `id`,`timestamp`,`tool`,`subject`,`tier`,`decision` | tidak |
| `session_init` | `id`,`timestamp`,`cwd`,`app_version` | tidak |

`parent_id` menyiapkan percabangan (fork): id entry induk, atau `null` untuk akar.

## Kebijakan kerusakan

| Kasus | Perilaku | Alasan |
|---|---|---|
| baris terakhir tidak lengkap | diabaikan + warning | crash saat menulis baris terakhir adalah kejadian normal |
| baris tengah rusak | `E_SESSION_CORRUPT` + nomor baris | kerusakan di tengah berarti file berubah di luar aplikasi |
| baris pertama bukan header | `E_SESSION_CORRUPT` | tanpa header, cwd/versi tidak diketahui |
| file kosong | `E_SESSION_CORRUPT` | sesi kosong bukan sesi |

## Fork

`fork(source, dir, title)` menulis file baru: header baru (`id` baru, `parent_session` = id sesi
induk), lalu menyalin semua entry lama apa adanya (id entry dipertahankan agar rujukan tetap sah).

## Audit approval

Setiap keputusan gate approval ditulis sebagai entry `approval_decision` dengan `tool`, `tier`
(`read`/`write`/`exec`), `subject`, dan `decision` (`Once`/`Session`/`Always`/`Deny`). Entry ini
tidak pernah masuk konteks model; gunanya untuk audit dan untuk menjawab "siapa mengizinkan apa".
`tier` bertanda `#[serde(default)]` supaya file lama yang belum memilikinya tetap terbaca.

## Lock

Setiap `SessionStore` memegang lock `<file>.lock` (dibuat dengan `create_new`, isi: timestamp ms +
pid). Proses kedua yang membuka file yang sama ditolak `E_SESSION_LOCKED`. Lock yang lebih tua
dari satu jam dianggap basi dan diambil alih otomatis, supaya proses yang mati mendadak tidak
mengunci sesi selamanya. Lock dilepas saat `SessionStore` di-drop.

## Resume

`resume_messages(entries)` mengembalikan **hanya** entry `message`, berurutan; entry metadata
(`session_init`, `model_usage`, `model_change`, `title_change`, `approval_decision`) tidak dikirim
ke model. Entry `compaction` menghapus semua pesan sebelumnya dan menggantinya dengan satu pesan
`[ringkasan konteks sebelumnya]` — aturan yang sama dipakai saat sesi hidup, jadi resume tidak
mengulang pekerjaan ringkasan yang sudah dibayar.

`list(dir)` mengembalikan ringkasan terbaru lebih dulu (urut `updated_at`, lalu id sebagai
pemecah seri); file yang tidak bisa diparse dilewati supaya satu file korup tidak menyembunyikan
sesi lain. `latest(dir)` = elemen pertama daftar itu, dipakai `mx --resume` tanpa id.

## Tulis atomik

`write_atomic` menulis `<path>.jsonl.tmp` lalu rename. Di Windows rename menolak menimpa, jadi file
lama dihapus lebih dulu — jendela non-atomik ini dicatat dan akan ditutup dengan backup harian
sebelum v1 dirilis.

## Perbedaan dengan omp v3 (dicatat, bukan ditebak)

| Aspek | omp v3 | v1 ini |
|---|---|---|
| versi | `CURRENT_SESSION_VERSION = 3` | `1` |
| slot judul 256 byte | ada (`session-entries.ts:17`) | tidak |
| tipe entry | 14+ (`custom`, `branch_summary`, `reset_boundary`, …) | 8 |
| pohon | `activeLeafId` | `parent_id` per entry |
| backend | File/Memory/SQL/Redis | File saja |

Impor sesi omp v3 = pekerjaan M4 (`RENCANA-FULL-RUST.md` §8), bukan bagian v1.
## Kompatibilitas versi

- Header menyimpan `version` (saat ini `1`).
- File dengan **versi lebih baru** dari yang didukung ditolak dengan `E_SESSION_CORRUPT` yang menyebut
  versi file dan versi implementasi — implementasi tidak boleh membaca skema yang belum dikenal dengan
  asumsi lama.
- Versi `0`/tanpa header ditolak.
- Belum ada format sesi lama di v1 sehingga **tidak ada migrasi yang diperlukan**; bila kelak versi
  dinaikkan, migrasi harus ditambahkan bersama kenaikan `SESSION_VERSION` dan diuji.