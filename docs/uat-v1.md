# Paket UAT v1 (RENCANA §11.3)

Dijalankan **manual oleh iqbal** di Windows, hasil dicatat di kolom “Hasil”.
Agent tidak mengisi kolom hasil. Semua langkah memakai gateway sungguhan;
tanpa `MODELUXAI_API_KEY` yang valid, U-01..U-09 dan U-12 tidak bisa dinilai.

## Persiapan

1. `cargo build --release` (atau `.\check.ps1` bila ingin sekalian gate).
2. Pastikan kunci tersedia: `setx MODELUXAI_API_KEY "<kunci>"` **atau**
   `mx config key-import` (kunci tidak boleh ditempel ke chat/log).
3. Terminal: Windows Terminal dan `cmd.exe` (U-09 dijalankan di keduanya).
4. Untuk bukti performa, jalankan dengan `MX_LOG=debug`; log ditulis ke
   `%APPDATA%\ModeluXAI\TUI\logs\`. Catat `delta_ms`/`frame_ms` bila relevan.

## Skenario

| ID | Tujuan | Langkah | Hasil yang diharapkan | Hasil (diisi iqbal) |
|---|---|---|---|---|
| U-01 | Streaming teks dasar | Jalankan `mx`, ketik prompt teks biasa, Enter | Jawaban tampil bertahap (streaming), turn selesai, status kembali “siap” | |
| U-02 | Approval `read` | Prompt yang memicu tool `read` pada berkas nyata | Blok approval inline muncul (tool, subjek, argumen verbatim); pilih “izinkan sekali” → hasil berkas tampil | |
| U-03 | Deny `bash` | Prompt yang memicu `bash`; tolak approval | Tool tidak berjalan, turn selesai aman, timeline mencatat penolakan | |
| U-04 | Ganti model | `/model <id dari daftar>`, lalu turn baru | Model aktif berubah di status; turn berikut memakai model baru; `/usage` mencatat model itu | |
| U-05 | Compaction | Sesi panjang (≥ beberapa turn besar), `/compact` | Ringkasan dibuat, konteks turun (token sebelum→sesudah ditampilkan), percakapan tetap koheren | |
| U-06 | Resume sesi | Tutup aplikasi, `mx --resume latest` (atau `mx --resume <id>`); di dalam aplikasi pakai `/sessions` lalu `/switch <id>`. Catatan: `/resume` di v1 berarti **lanjutkan sesi yang dijeda** (pasangan `/pause`), bukan membuka sesi tersimpan — lihat deviasi di `WORK.md` | Konteks dipulihkan; pesan lama tampil; bisa melanjutkan turn | |
| U-07 | Fork sesi | `/fork` lalu lanjutkan percakapan | Sesi salinan aktif; sesi asli tidak berubah; keduanya bisa dibuka terpisah | |
| U-08 | Abort streaming | Saat streaming, tekan `Ctrl+C` sekali | Turn berhenti (“turn dibatalkan”), aplikasi tetap hidup; `Ctrl+C` kedua saat idle meminta konfirmasi keluar | |
| U-09 | Resize terminal | Resize jendela saat streaming dan saat histori panjang | Render tetap rapi, tidak ada artefak; scroll tetap benar | |
| U-10 | Config rusak | Rusakkan satu kunci di `config.toml`, jalankan `mx` | Aplikasi berhenti dengan error menyebut **file + baris + kunci**, tidak jalan dengan config setengah | |
| U-11 | Kunci hilang | Jalankan tanpa env var/keyring | `E_KEY_MISSING` dengan petunjuk (env var/`key-import`), tidak meminta kunci lewat chat | |
| U-12 | Pemakaian nyata | 30 menit pemakaian untuk pekerjaan sendiri | Tidak crash; sesi tersimpan; rotasi/log wajar; tidak ada kebocoran kunci di file sesi/log | |

## Catatan pengisian

- Tulis **apa yang benar-benar terjadi**, termasuk yang gagal; jangan menulis
  “lulus” tanpa langkah yang dijalankan.
- Lampirkan potongan log (`MX_LOG=debug`) hanya bila ada masalah; **jangan**
  lampirkan isi kunci API.
- Hasil UAT ini menjadi syarat Gate Fase 1 (RENCANA §6 akhir) dan tidak boleh
  digantikan klaim agent.
