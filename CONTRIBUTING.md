# Berkontribusi — ModeluXAI TUI (maicli)

Terima kasih mau berkontribusi! Proyek ini open-source dengan **gerbang mutu yang
sama untuk semua orang** (termasuk maintainer) — setiap PR melewati CI sebelum bisa
di-merge.

## Alur kontribusi (PR)

1. **Fork + branch** dari `main`. Nama branch: `feat/…`, `fix/…`, `docs/…`,
   `refactor/…`, `perf/…`, `test/…`, `ci/…`, atau `chore/…`.
2. Satu PR = **satu perubahan logis** (jangan campur refactor + fitur).
3. Jalankan gerbang lokal **sebelum push**:
   - Windows: `.\check.ps1`
   - Linux/macOS: `bash check.sh`
   (isi: `cargo fmt --check` · `cargo clippy -D warnings` · `cargo test` ·
   `cargo deny check` · aturan dependensi)
4. Commit dengan **Conventional Commits** (dipakai release-plz untuk versioning):
   | Prefix | Efek versi |
   |---|---|
   | `feat:` di `tui-app` | MINOR (aplikasi 0.x memakai `features_always_increment_minor`) |
   | `fix:` / perubahan hanya di crate internal | PATCH |
   | `feat!:` atau `BREAKING CHANGE:` | MAJOR (untuk `0.x`: MINOR) |
   | `docs:`, `refactor:`, `perf:`, `test:`, `build:`, `ci:`, `chore:` | tanpa bump |
   Contoh: `fix(relay): tolak room asing sebelum join`.
5. Buka PR ke `main`; **judul PR juga harus Conventional** (dipakai squash merge
   dan divalidasi check `pr-title`).
6. Tunggu required checks hijau:
   `fmt`, `clippy (ubuntu-latest)`, `clippy (windows-latest)`,
   `test (ubuntu-latest)`, `test (windows-latest)`, `build-release`, `deny`,
   `msrv`, `actionlint`, `pr-title`, `dependency-review`.
   macOS ikut diuji tetapi masih *allow-failure* sampai verifikasi macOS pertama.
7. Review maintainer; merge hanya via **squash** (linear history).

## Versioning & rilis

- **Jangan** mengubah versi `Cargo.toml` atau `CHANGELOG.md` manual di PR biasa.
  `release-plz` membuka PR rilis otomatis berisi bump versi + changelog dari
  Conventional Commits; maintainer me-merge PR itu.
- Setelah PR rilis di-merge, release-plz membuat tag `vX.Y.Z` + GitHub Release;
  workflow `release.yml` membangun 6 biner (Linux x64/arm64, macOS arm64/x64,
  Windows x64/arm64), checksum SHA256, dan melampirkannya ke Release.
- Detail alur, branch protection, dan troubleshooting: `docs/setup-github.md`.

## Aturan kode

- **Tidak ada `unwrap`/`expect`/`panic`/`unreachable` di kode library** — ditegakkan
  clippy (`-D warnings`). `tui-app` boleh `expect` hanya di startup dengan pesan
  yang menjelaskan apa yang gagal + tindakan pengguna.
- **Error bernama** (`tui_wire::ErrorCode`), pesan pengguna berbahasa Indonesia dan
  menyebut tindakan yang bisa dilakukan.
- **Tidak pernah menyentuh kredensial**: API key hanya dari env var / keyring; tidak
  pernah ditulis ke file, log, session, atau `Debug`. Jangan tambahkan env var dengan
  nama mengandung `KEY/TOKEN/SECRET/PASSWORD` ke proses anak.
- **Approval gate wajib**: semua eksekusi tool lewat gate; tidak ada jalur pintas
  "tool aman".
- Test perilaku (bukan test "fungsi ada"); deterministik; setiap bug → test regresi
  dulu, baru perbaikan.
- Dependensi baru: **alasan satu baris di `DEPENDENCIES.md`** + `cargo deny check`
  bersih. Dependensi yang menelepon jaringan sendiri dilarang tanpa persetujuan.
- `unsafe` dilarang (lint `forbid`).
- Tidak ada `TODO/FIXME/HACK` di kode yang di-merge; hal tertunda dicatat di
  issue/PR, bukan komentar kode.
- Jangan menurunkan MSRV (`rust-version = 1.89`) tanpa diskusi; job `msrv` akan
  menangkap dependency yang meminta Rust lebih baru.

## Melaporkan masalah

- Bug: gunakan template issue + langkah reproduksi + platform + log (tanpa kredensial);
- Fitur: template feature request;
- Keamanan: **jangan** buka issue publik — lihat `SECURITY.md`.
