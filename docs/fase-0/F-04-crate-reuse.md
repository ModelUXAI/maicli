# F-04 — audit 12 crate `pi-*` (reuse)

Status: DONE (audit baca-source). **Catatan usang (2026-11-18):** omp 18.22.0 sudah
merestrukturisasi crate `pi-*` menjadi **natives** (`packages/natives`, docs/natives-architecture.md);
audit di bawah mencerminkan struktur lama. **Tidak lagi relevan untuk build** — keputusan kita
menulis ulang murni Rust (tanpa dependensi omp) membuat butir "bukti build" tidak diperlukan;
dokumen dipertahankan sebagai referensi sejarah audit.

## Ringkasan

| Crate | Baris | Lisensi | Permukaan napi | Verdict Windows |
|---|---|---|---|---|
| `pi-ast` | 7.613 | MIT | tidak (1 hit = string test) | bisa dipakai; fitur `wasm` butuh CMake |
| `pi-builtins` | 175.092 | MIT | tidak | bisa dipakai (`mkdir -m`, xattr tidak ada di Windows) |
| `pi-diff` | 1.112 | MIT | tidak | bisa dipakai (nol kode platform) |
| `pi-edit` | 25.246 | MIT | tidak | bisa dipakai |
| `pi-iso` | 4.611 | MIT | tidak | bisa dipakai (ProjFS asli) |
| `pi-natives` | 127.490 | MIT | **ya — satu-satunya** (`cdylib`, `napi`) | bisa dipakai, **wajib nightly** |
| `pi-predict` | 7.713 | MIT | tidak | bisa dipakai (candle hanya macOS) |
| `pi-shell` | ±60,9 ribu (di luar mesin bash/zsh) | MIT | tidak | bisa dipakai |
| `pi-vcs` | 14.307 | MIT | tidak | bisa dipakai (syscall unix sudah di-gate) |
| `pi-vfs` | 6.885 | MIT | tidak | bisa dipakai |
| `pi-voice` | 3.759 | MIT | tidak (napi-free by design) | bisa dipakai (WASAPI penuh) |
| `pi-walker` | 15.654 | MIT | tidak | bisa dipakai |

Bukti kunci:

- `crates/pi-natives/Cargo.toml:11` → `crate-type = ["cdylib"]`; `:32-33` dependensi `napi`, `napi-derive`.
- `crates/pi-natives/src/lib.rs:23` → `#![feature(alloc_error_hook)]` ⇒ **nightly**.
- `crates/pi-voice/src/lib.rs:4`, `crates/pi-predict/src/lib.rs:4` → menyatakan napi-free.
- Repo omp sendiri sudah build Windows: `.github/workflows/ci.yml:887` (`windows-2025`, `omp-windows-x64.exe`), `:942` smoke test.

## Konsekuensi untuk `TUI/`

1. Reuse `pi-*` = **mengubah keputusan toolchain V-06** (stable → nightly) → butuh keputusan iqbal.
2. Tanpa reuse, tool `bash`/`edit`/`grep` harus ditulis sendiri di atas `std::process` + crate
   generik (`regex`, `walkdir`, `globset`) — seperti yang sekarang ada di `tui-tools`.
3. `pi-natives` tidak bisa dipakai sebagai library biasa tanpa melepas `napi` (patch besar), jadi
   pemakaian nyatanya adalah **crate non-napi** (11 crate).

## Rekomendasi

Mulai reuse dari kandidat termurah dan paling terisolasi: `pi-diff` (1.112 baris, nol kode platform)
dan `pi-vfs`, lalu `pi-walker`. `pi-shell`/`pi-builtins` baru setelah ABI nightly diterima.
