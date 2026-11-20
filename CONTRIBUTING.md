# Berkontribusi — ModeluXAI TUI (maicli)

Terima kasih mau berkontribusi! Proyek ini open-source dengan **gerbang mutu yang
sama untuk semua orang** (termasuk maintainer) — setiap PR melewati CI sebelum bisa
di-merge.

## Alur kontribusi (PR)

1. **Fork + branch** dari `main` — nama branch: `feat/…`, `fix/…`, atau `docs/…`;
2. Satu PR = **satu perubahan logis** (jangan campur refactor + fitur);
3. Jalankan gerbang lokal **sebelum push**:
   - Windows: `.\check.ps1`
   - Linux/macOS: `bash check.sh`
   (isi: `cargo fmt --check` · `cargo clippy -D warnings` · `cargo test` · `cargo deny check`)
4. Push + buka PR ke `main`;
5. CI wajib hijau (Linux + Windows; macOS ditempatkan allow-failure);
6. Review maintainer; merge hanya via **squash** dengan pesan imperatif
   (`tambah parser SSE tahan chunk terpotong`).

## Aturan kode (ringkas — detail di `AGENTS.md`)

- **Tidak ada `unwrap`/`expect`/`panic`/`unreachable` di kode library** — ditegakkan
  clippy (`-D warnings`). `tui-app` boleh `expect` hanya di startup dengan pesan yang
  menjelaskan apa yang gagal + tindakan pengguna.
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
  bersih. Dependensi yang menelepon jaringan sendiri dilarang.
- `unsafe` dilarang (lint `forbid`) — jalur proses-group POSIX pun dibangun tanpa FFI.
- Tidak ada `TODO/FIXME/HACK` di kode yang di-merge; hal tertunda dicatat di
  `WORK.md`.

## Versioning & release

- Semver: satu sumber di `[workspace.package] version` (`Cargo.toml` root);
- `CHANGELOG.md` Keep-a-Changelog — **setiap PR menambah entri** di bawah
  `[Unreleased]` (Added/Fixed/Changed/Removed);
- Rilis = tag `vX.Y.Z` → workflow `release.yml` membangun Linux/Windows/macOS,
  menguji ulang, dan membuat GitHub Release dengan catatan dari CHANGELOG.

## Melaporkan masalah

- Bug: langkah reproduksi + platform + log (tanpa kredensial);
- Keamanan: **jangan** buka issue publik — lihat `SECURITY.md`.
