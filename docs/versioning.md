# Audit penomoran versi

## Yang sudah ada

- Versi workspace: `Cargo.toml` `[workspace.package] version = "0.1.0"` (satu versi
  untuk semua crate via `version.workspace = true`).
- `release-plz.toml`: satu `version_group = "workspace"` untuk 16 crate, hanya
  `tui-app` yang diberi tag/release; `git_tag_name = "v{{ version }}"`;
  `features_always_increment_minor = true` (0.x: feat → MINOR); `git_only = true`.
- Workflow: `release-plz.yml` membuat tag + memanggil `release.yml`;
  `release.yml` memverifikasi tag == versi `Cargo.toml`; semua artefak bernama
  `maicli-<platform>-vX.Y.Z`.
- Dokumentasi: `docs/setup-github.md` menjelaskan alur versi + tag.

## Status per 2026-11-23

1. **Repo URL** seragam `ModelUXAI/maicli` di `release-plz.toml`,
   `docs/setup-github.md`, `npm/modeluxai/package.json`, `SECURITY.md`,
   `sdk/go/go.mod`, dan issue config. Repo sudah ada di GitHub.
2. **Jumlah biner = 6** di dokumen, README, komentar `release.yml`, dan matriks
   `release.yml` (linux x64/arm64, macOS arm64/x64, Windows x64/arm64) — konsisten.
3. **Tag baseline `v0.1.0` sudah dibuat lokal** (tag anotasi pada commit rilis).
   Push tag hanya bisa setelah push `main` diizinkan pemilik repo.
4. **Versi npm/SDK masih 0.1.0** dan mengikuti rilis manual; `npm-publish.yml`
   menyinkronkan versi wrapper dari tag. Paket biner platform di-hardcode 4 varian
   (linux-x64, darwin-arm64, darwin-x64, win32-x64) — linux-arm64 dan
   win32-arm64 belum punya paket npm meski biner rilisnya ada.
5. **Belum ada heading `[0.1.0]` di `CHANGELOG.md`**; release-plz yang membuatnya
   saat PR rilis pertama.
6. **`semver_check = false`** — sengaja untuk 0.x: breaking change hanya menaikkan
   MINOR. Ini perlu diketahui kontributor.

## Rekomendasi

- Push `main` + tag `v0.1.0` setelah pemilik memberi akses tulis (collaborator,
  deploy key, atau PAT).
- Aktifkan release-plz hanya setelah tag baseline ada di remote.
- Biarkan release-plz menulis `[0.1.0]` pertama; jangan isi manual.

## Aturan tag (diperbarui 2026-11-24)

- **Tag bersifat immutable.** Dilarang force-update tag yang sudah ada.
  Setiap rilis membuat tag baru `vX.Y.Z` yang menunjuk commit rilis tersebut.
- **Versi naik tiap rilis.** Perubahan yang dirilis memakai semver:
  `feat` → MINOR (`0.1.0` → `0.2.0`), `fix` → PATCH, breaking → MAJOR.
- **Status saat ini:** rilis GUI + bundling CI dirilis sebagai **v0.2.0**
  (commit `536c47f` dan setelahnya); `v0.1.0` adalah baseline CLI dan
  tidak lagi dipindahkan.
- `release-plz` hanya akan membuat PR rilis otomatis bila commit memakai
  Conventional Commits (`feat:`, `fix:`). Commit berformat `[vX.Y.Z] …`
  tidak dikenali sebagai perubahan versi, sehingga versi harus di-bump manual
  sampai konvensi commit dipakai konsisten.
- Semua marker versi mengikuti satu angka: `Cargo.toml`, `gui/Cargo.toml`,
  `frontend/package.json`, `gui/tauri.conf.json`, `npm/modeluxai/package.json`,
  dan `sdk/typescript/package.json`.
