# F-06 — katalog KDL

Status: DONE (audit baca-source). Verifikasi crate `kdl` selesai (2026-11-18): `kdl` 6.7.1 menerima KDL v2 penuh — probe test hijau (boolean `#true`, properti berstrip `base-url=`, komentar `//`, blok nested). Jebakan #1/#2 tertutup. Port kompilator semantik (2.000-3.000 baris) tetap kandidat P2. **Tahap 1 kompilator (2026-11-18)**: crate baru **`tui-katalog`** — `susun(rules/)` mem-parse strata `providers` + `auth` dengan `kdl` 6.7.1; **oracle terbukti**: katalog omp nyata → **98 provider** (dari 105 berkas — 7 berkas `on-api` lain stratum) + **108 auth**, field deepseek benar (`DEEPSEEK_API_KEY`, `deepseek-v4-pro`, effort). Semantik penuh (classes/taxonomy/runtime/cascade) = lanjutan.

## Volume

| Kelompok | Berkas | Baris |
|---|---|---|
| `providers/` | 105 | 9.844 |
| `auth/` | 109 | 1.316 |
| `classes/` | 19 | 2.134 |
| `taxonomy/` | 23 | 1.209 |
| `runtime/` | 1 | 534 |
| **total** | **257** | **15.037** (561.657 byte) |

Kompilator: `packages/catalog/scripts/compat-compiler/` — **8 berkas / 2.829 baris**
(`kdl-reader` 125, `compile-taxonomy` 588, `compile-cascade` 197, `compile-providers` 463,
`compile-auth` 852, `compile-behavior` 420, `compile-axes` 97, `index` 87).
Keluaran: `compat/rules.json` 526.739 byte.

## Sifat masukan (penting untuk port)

| Konstruksi | Jumlah |
|---|---|
| Blok `{ … }` | ±2.652 baris |
| Node dengan ≥2 argumen posisional | 1.621 |
| Boolean KDL **v2** (`#true`/`#false`) | **1.190** |
| Properti berstrip (`base-url=`) | 59 |
| Komentar `//` | 2.815 (18,7 %) |
| Slashdash `/-`, anotasi tipe `(t)`, string raw/multiline | **0** |
| Nama node berbeda | 437 |
| Kedalaman blok maksimum | 7 |

## Verdict

**Medium.** Parsing murah (`kdl-reader.ts` = 125 dari 2.829 baris, 4,4 %) — bisa diganti crate
`kdl` Rust; yang mahal adalah **semantik**: 2.704 baris validasi atas 437 nama node di 5 strata.
Perkiraan port: **2.000–3.000 baris Rust** + harness paritas terhadap `rules.json` yang sudah ada
(oracle-nya jelas: keluaran JSON yang sama).

## Jebakan yang dicatat

1. **KDL v2**: 1.190 `#true`/`#false` akan ditolak parser KDL 1.0.
2. ~~Ketersediaan crate `kdl` yang mendukung v2 belum diverifikasi~~ — terverifikasi 2026-11-18: `kdl` 6.7.1 (crates.io) meng-parse KDL v2 dengan benar (probe: boolean, properti, komentar, nesting).
3. Alternatif murah: konversi KDL → statics sekali di build (bukan port kompilator penuh) dan
   simpan `rules.json` sebagai aset — tapi kehilangan satu sumber kebenaran (KDL).
