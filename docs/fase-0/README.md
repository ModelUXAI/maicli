# docs/fase-0 — bukti verifikasi

Folder ini memuat bukti Fase 0 (`RENCANA.md` §5, V-01..V-06) dan Fase 0 program penuh
(`RENCANA-FULL-RUST.md` §7, F-01..F-12).

Aturan: setiap klaim menyebut path + baris sumber, dan setiap baris punya kolom **sumber**
yang membedakan "dibaca ulang di sesi ini" dari "dikutip dari laporan studi (S-1)".

| Dokumen | Isi | Status |
|---|---|---|
| `V-01-inventaris-subset.md` | perilaku inti yang di-port, dengan path:baris | PARTIAL |
| `V-02-gateway.md` | probe `/v1/models` + streaming live | **DONE** (2026-11-18: discovery 25 model, turn streaming, tool call, keyring nyata) |
| `V-03-tui-windows.md` | spike TUI di Windows | PARTIAL |
| `V-04-sesi.md` | verifikasi skema sesi v1 | DONE |
| `V-05-harness.md` | harness differential | **DONE-internal** — runner `mx --parity-*`, perekam `--event-log`, 11 golden + uji mutasi; golden omp dilepas oleh keputusan (F-01) |
| `V-06-toolchain.md` | keputusan edition/MSRV/dependensi | DONE |
| `F-01-snapshot.md` | snapshot omp | **DITUTUP** (keputusan iqbal 2026-11-18: paritas omp dilepas) |
| `F-02-inventaris.md` | hitungan definitif tool/slash/provider + jawaban Unknown | DONE |
| `F-04-crate-reuse.md` | audit crate `pi-*` | DONE (audit) — usang: omp 18.22.0 memakai natives; referensi sejarah |
| `F-05-wire.md` | skema RPC + generator | DONE (audit) — implementasi ditutup oleh keputusan opsi B (kita punya wire sendiri) |
| `F-06-katalog.md` | KDL + kompilator | DONE (audit) + **tahap 1 kompilator selesai** (`tui-katalog`: providers+auth ter-parse dari katalog omp nyata, 98+108) |

Catatan: implementasi M1 dimulai atas perintah eksplisit iqbal sebelum Gate Fase 0; lihat
`WORK.md` bagian "Catatan otoritas".
