# Changelog

Format mengikuti Keep-a-Changelog. Entri baru selalu di bawah `## [Unreleased]`.

## [Unreleased]

### Added
- **GUI desktop maicli (2026-11-24)**: frontend Vite + React + TypeScript
  (`frontend/`) dengan tema gelap, layar sambutan, daftar sesi, chip model/cwd,
  panel aktivitas tool, dan footer hint meniru contoh omp. Shell Tauri 2
  (`gui/`) menjalankan `maicli --mode rpc` sebagai child process; frame JSONL
  diteruskan dua arah lewat command `rpc_send` + event `rpc-line`.
- **Rilis GUI (2026-11-24)**: job `gui` di `release.yml` membangun `.exe` (NSIS/
  MSI) Windows, `.deb`/AppImage Linux, dan `.dmg` macOS (lewat runner CI).
  Amandemen RENCANA `A-2026-11-24` mencatat perubahan L-1/O-7/O-8.

