# V-05 — harness differential (status: DONE-internal)

Status: **DONE-internal** (keputusan iqbal 2026-11-18: paritas omp dilepas — F-01 ditutup).
Alat rekam, normalisasi, pembanding, 11 golden internal + uji mutasi lengkap;
**rekaman gateway live kini ada** (V-02 DONE 2026-11-18: `live-v02.jsonl` 27 event,
`live-tool.jsonl` tool call `read` nyata, di direktori data uji).

Bergantungan lama:
- ~~F-01~~ — ditutup: paritas omp dilepas;
- ~~V-02~~ — DONE (kunci + keyring nyata).

## Yang sudah ada

| Komponen | Bentuk | Bukti |
|---|---|---|
| Perekam transkrip | `mx --event-log <FILE>` (mode TUI/teks/RPC) menulis `AgentEvent` per baris JSON; di-flush sebelum proses keluar | `event_log::tests::*` |
| Normalisasi | `tui_wire::parity::normalize` — membuang id acak, timestamp, event UI murni; mempertahankan urutan, teks delta, tool, stop reason | `parity::tests::*` |
| Pembanding | `mx --parity-expected <ACUAN.jsonl> --parity-actual <HASIL.jsonl>` → laporan `skenario + indeks pertama berbeda + hilang/kelebihan`, exit 1 bila beda | `parity_report::tests::*`, smoke CLI |
| Baseline internal | **11 fixture** `tests/golden/*.txt`: `teks-murni`, `tool-diizinkan`, `tool-ditolak`, `tool-tidak-dikenal`, `rantai-dua-tool`, `dua-tool`, `follow-up`, `tiga-turn` (teks → tool → follow-up), `abort`, `length`, `error-provider` | `agent::tests::golden_*` (12 test, termasuk uji mutasi) |
| Uji harness wajib | mutasi satu event membuat harness MERAH dan menyebut indeks | `golden_mendeteksi_mutasi_satu_perilaku` |

## Rancangan (tetap berlaku)

```
skenario (prompt + config + rekaman gateway)
   ├─→ omp --mode rpc  (referensi) ─┐
   └─→ mx  --mode rpc  (kandidat)  ─┴─→ normalisasi → bandingkan urutan event + payload
```

1. **Perekam (VCR)**: merekam request/response gateway termasuk frame SSE, lalu diputar ulang
   identik ke kedua biner — tanpa itu transkrip berbeda karena model nondeterministik.
   Saat ini rekaman gateway **belum ada** (butuh V-02); mock lokal hanya untuk test protokol.
2. **Normalisasi**: lihat tabel di atas; aturan sudah dikunci test.
3. **Korpus**: 9 skenario **baseline implementasi sendiri** sudah ada. Korpus acuan omp
   (target §11.3: ≥15) belum bisa dibuat tanpa F-01 + V-02.
4. **Uji harness**: sudah ada (mutasi event → MERAH).

## Yang tidak lagi diperlukan (keputusan F-01)

- Transkrip acuan omp ≥15 skenario — dilepas bersama target paritas omp.

- Transkrip acuan dari `omp --mode rpc` untuk korpus ≥15 skenario (F-01).
- Rekaman request/response gateway agar kedua biner menerima input identik (V-02).
- Menjalankan `mx --parity-*` terhadap transkrip omp dan mencatat hasil di `docs/paritas.md`
  (status `paritas` masih 0).

## Cara menjalankan (setelah bahan tersedia)

```text
# rekam sisi mx
mx --mode text --prompt "<skenario>" --event-log hasil.jsonl
# bandingkan dengan acuan
mx --parity-expected acuan.jsonl --parity-actual hasil.jsonl
```
