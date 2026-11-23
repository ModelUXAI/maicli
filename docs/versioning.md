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

## Yang belum konsisten

1. **Repo URL** sudah diseragamkan ke `ModelUXAI/maicli` di `release-plz.toml`,
   `docs/setup-github.md`, `npm/modeluxai/package.json`, `SECURITY.md`, dan issue
   config. Pastikan repo itu yang benar-benar dibuat.
2. **Jumlah biner di dokumen masih 4**, padahal matrix `release.yml` sekarang 6
   target (linux x64/arm64, macOS arm64/x64, Windows x64/arm64).
3. **Belum ada tag lokal `v0.1.0`** (git tag gagal karena safe.directory); baseline
   release-plz butuh tag awal. Perlu dicek/dibuat oleh pemilik.
4. **Versi npm/SDK masih 0.1.0** dan mengikuti rilis manual; perlu diputuskan
   apakah `npm-publish.yml` menyinkronkan versi dari tag (sudah dilakukan di
   langkah "sinkronkan versi wrapper"), tetapi paket platform di hardcode 4.
5. **Belum ada file `VERSION`/changelog rilis pertama**; CHANGELOG hanya
   `[Unreleased]`. Release-plz akan membuat heading `[0.1.0]` saat PR rilis.
6. **`semver_check = false`** — sengaja untuk 0.x, tetapi perlu dicatat di dokumen
   agar kontributor tahu breaking change 0.x hanya menaikkan MINOR.

## Rekomendasi

- Buat repo `ModelUXAI/maicli`, lalu buat tag baseline `v0.1.0` sebelum
  mengaktifkan release-plz.
- Dokumen sudah diperbarui ke 6 target biner.
- Biarkan release-plz yang menulis `[0.1.0]` pertama; jangan isi manual.
