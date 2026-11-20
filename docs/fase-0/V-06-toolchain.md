# V-06 / F-10 — toolchain dan dependensi

Status: DONE.

## Lingkungan terukur (2026-11-17)

| Komponen | Versi |
|---|---|
| OS | Microsoft Windows 10 IoT Enterprise LTSC 10.0.19044 (x64) |
| rustc | 1.99.1 (58cf8f9c3 2026-10-13), target `x86_64-pc-windows-msvc` |
| cargo | 1.99.1 (5f94df478 2026-08-27) |
| rustup | 1.29.3 (edbc831ce 2026-11-02) |
| komponen | `rustfmt`, `clippy` terpasang |
| git | 2.56.0.windows.2 |
| PowerShell | 5.1.19041.7844 |
| cargo-deny | 0.21.0 (dipasang dari crates.io, 8m11s) |

## Keputusan

| Keputusan | Nilai | Alasan |
|---|---|---|
| Edition | 2024 | rustc 1.99 mendukung; menutup idiom lama |
| `rust-version` (MSRV) | 1.85 | batas minimum edition 2024 |
| Runtime async | tokio (multi-thread) | satu runtime, dipakai juga oleh `reqwest` |
| TLS | rustls (bukan OpenSSL) | build Windows tanpa dependensi native |
| Lint | `unsafe_code=forbid`, `dead_code=deny`, clippy `unwrap_used/expect_used/panic/todo/unimplemented=deny` | `AGENTS.md` §4 |
| Profil rilis | `lto=fat`, `codegen-units=1`, `strip=true`, `panic=abort` | ukuran binary |
| Gerbang | `check.ps1` (fmt, clippy, test, deny, aturan dependensi) | `AGENTS.md` §7 |

## Catatan penting untuk keputusan berikutnya

- **Crate `pi-*` omp butuh Rust nightly**: `crates/pi-natives/src/lib.rs:23` memakai
  `#![feature(alloc_error_hook)]`, dan `rust-toolchain.toml` omp memakai `nightly`.
  Selama `TUI/` tidak memakai crate itu, stable sudah cukup. Memakai ulang `pi-shell`/`pi-edit`
  dkk. berarti mengubah keputusan MSRV ini → butuh keputusan iqbal (dicatat di WORK.md).
- Build crate `pi-*` juga butuh MSVC C compiler; `pi-ast` dengan fitur `wasm` butuh CMake.
- `find-msvc-tools` dipin `=0.1.12` di Cargo.toml omp — workaround build Windows; catat bila nanti
  memakai crate `pi-*`.

## Perintah gerbang (dijalankan, hijau)

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check            → advisories ok, bans ok, licenses ok, sources ok
.\check.ps1                 → SEMUA GERBANG HIJAU
```
