# F-01 — snapshot omp yang dikunci (DITUTUP oleh keputusan iqbal)

Status: **DITUTUP (2026-11-18, opsi B)** — iqbal memutuskan **target paritas
dengan omp dilepas**. Tidak ada lagi kebutuhan mengunci snapshot omp untuk
pembandingan; checkout `oh-my-pi-main/` (18.22.0, tanpa `.git`) tetap di repo
sebagai **rujukan baca** perilaku (komentar kode + docs), bukan dependensi.

## Fakta historis (tercatat sebelum keputusan)

| Fakta | Nilai |
|---|---|
| Versi paket di checkout | **18.22.0** (`packages/*/package.json`) |
| Versi yang dikunci RENCANA.md | 18.20.2 @ `7c49c11` |
| `.git` di checkout | tidak ada |

## Konsekuensi keputusan opsi B

1. **C-12** ditutup dengan bukti internal: 11 golden fixture + runner
   `mx --parity-*` + uji mutasi (`golden_mendeteksi_mutasi_satu_perilaku`);
   kriteria "golden omp lolos" tidak berlaku lagi.
2. `docs/paritas.md` diredefinisi menjadi **ledger bukti perilaku internal**
   (141 baris port + uji), bukan pembandingan omp.
3. Rujukan `path:baris` omp di dokumen tetap sah sebagai **jejak asal
   perilaku** (snapshot 18.22.0), tidak dijamin cocok dengan versi omp lain.
4. Amandemen formal `RENCANA.md`/`AGENTS.md` §0.4 dicatat sebagai usulan untuk
   iqbal di `WORK.md` (agent tidak mengedit keduanya).