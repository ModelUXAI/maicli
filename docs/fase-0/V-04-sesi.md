# V-04 — skema sesi v1

Status: DONE.

- Spesifikasi: [`docs/skema-sesi-v1.md`](../skema-sesi-v1.md).
- Implementasi: `crates/tui-session` (`entries.rs`, `store.rs`).
- Test (8): round-trip tulis/baca, tail terpotong diabaikan + warning, baris tengah rusak →
  `E_SESSION_CORRUPT` dengan nomor baris, file tanpa header ditolak, fork menyetel
  `parent_session`, id unik, serialisasi header (`type:"session"`), hanya entry `message`
  yang masuk konteks model.

## Keputusan yang dikunci

| Keputusan | Nilai | Alasan |
|---|---|---|
| Versi format | `1` | v1 punya skema sendiri; impor omp v3 = M4 |
| Satu baris = satu entry | ya | append murah, crash hanya merusak baris terakhir |
| Flush per entry | ya | baris selesai tidak boleh hilang |
| Toleransi kerusakan | tail boleh, tengah tidak | membedakan crash vs intervensi luar |
| Fork | salin entry apa adanya + header baru | rujukan id lama tetap sah |
| Atomik | tmp + rename (dengan catatan Windows) | murah, cukup untuk v1 |

## Risiko yang dicatat

- Rename di Windows tidak menimpa: ada jendela non-atomik antara `remove_file` dan `rename`.
  Ditutup dengan backup harian sebelum rilis v1.
- ~~Belum ada penguncian lintas-proses~~ — sejak 2026-11-18 **sudah ada**: lock `<file>.lock` per store, proses kedua ditolak `E_SESSION_LOCKED`, lock basi (>=1 jam) diambil alih; bukti `store::tests::lock_kedua_ditolak_saat_sesi_masih_dipakai`, `lock_basi_diambil_alih`, `lock_dilepas_saat_store_di_drop`.

  Masuk kartu C-06 lanjutan.
