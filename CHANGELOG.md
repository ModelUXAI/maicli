# Changelog

Format mengikuti Keep-a-Changelog. Entri baru selalu di bawah `## [0.2.1] - 2026-11-24`.

## [Unreleased]

## [0.2.2](https://github.com/ModelUXAI/maicli/compare/v0.2.1...v0.2.2) - 2026-11-24

### Other

- [v0.1.0] finalkan ACL Windows: icacls tanpa PowerShell, verifikasi runner hijau
- [v0.1.0] perbaiki prasyarat rilis Windows: ACL runner, mock gateway, dan gate indeks
- [v0.1.0] siapkan repo publik ModelUXAI/maicli: CI, rilis, SDK, npm wrapper
- tambah 7 command baru dan /effort (antrean prompt, retry, clear, skills, hub, extended-context, force satu turn)
- ganti nama biner mx menjadi maicli (env var, folder data, docs, packaging)
- basis v1 lengkap - TUI agent Rust (13 crate, 504 test, 9 E2E)
- tambah kelompok 2 command: stats, changelog, setup flag, logout, mcp kelola, export, handoff, commit, todo subcommand
- [v0.1.0] perbaiki paket rilis arm64 dan test computer headless
- [v0.1.0] tangani clipboard tanpa display dan catat status CI
- [v0.1.0] perbaiki portabilitas Windows/macOS agar CI lintas platform hijau

### Changed
- **Redesain frontend GUI mengikuti 18 layar design reference (2026-11-24)**:
  `frontend/` ditulis ulang sebagai sistem visual berbasis token Tailwind 4
  (`@theme` dari `design-reference/*/code.html`: canvas `#0b141c`, surface
  `#060f16`–`#2d363e`, primary/secondary/tertiary/error, radius 2/4/8/12,
  spacing 4–24, JetBrains Mono lokal + Material Symbols lokal, tanpa gradient
  dan tanpa shadow lembut). Satu shell untuk semua layar (rail 256px, header
  48px, footer 40px) menggantikan grid tiga kolom tetap 300/1fr/320. Komponen
  baru: composer tiga band ASCII, HUD meter (bar + ASCII `█░`), kartu tool
  sub-shell, diff dua gutter nomor, modal approval Y/N/S, slash palette, modal
  model/settings/shortcuts, dan welcome dengan telemetry nyata.
- **Klien RPC bertipe (`src/rpc/{frames,transport,client}.ts`)**: korelasi
  request/balasan lewat `id`, reassembly `rpc_chunk` v2 (batas 64 MiB,
  `E_CHUNK_INVALID`/`E_CHUNK_TOO_LARGE`), negosiasi protokol v2 sekali saat
  `ready`, dan `command_output` tanpa id dikaitkan ke request `command` yang
  menunggu `ack`. `src/rpc.ts` lama dihapus; `list_sessions` (tidak ada di
  server) dihapus dari klien.
- **Balasan prompt simulasi dihapus**: `prompt` benar-benar dikirim lewat RPC.
  Daftar model diambil dari `command_output` perintah `/model`, daftar perintah
  slash dari registry Rust (`COMMANDS`), dan field yang tidak tersedia di
  protokol dirender `—` dengan alasannya, bukan angka rekaan.

### Added
- **Test frontend (49 test, Vitest + jsdom)**: parser frame dan chunking,
  reducer store (penggabungan `text_delta`, pasangan `tool_start`/`tool_end`,
  siklus approval, penurunan `busy`), composer (Enter/Shift+Enter/busy),
  matriks approval Y/N/S + Esc + fokus awal, dan parser diff yang toleran
  terhadap input cacat. `vite.config.ts` memuat plugin `@tailwindcss/vite` dan
  blok `test` (environment jsdom, setup jest-dom).
- **Mode preview DEV** (`?preview=1`, `?preview=welcome|approval|ask`): memutar
  frame hasil konversi transkrip golden `tests/golden/*.txt` lewat dynamic
  import, sehingga verifikasi visual berjalan tanpa API key dan tanpa masuk
  bundle produksi.

### Fixed
- **Rilis lengkap (2026-11-24)**: langkah `cargo test` dihapus dari workflow rilis karena CI sudah menggating commit yang sama; menjalankan ulang di saat rilis menyebabkan artefak linux-x64 hilang akibat test flaky.

### Added
- **GUI desktop maicli (2026-11-24)**: frontend Vite + React + TypeScript
  (`frontend/`) dengan tema gelap, layar sambutan, daftar sesi, chip model/cwd,
  panel aktivitas tool, dan footer hint meniru contoh omp. Shell Tauri 2
  (`gui/`) menjalankan `maicli --mode rpc` sebagai child process; frame JSONL
  diteruskan dua arah lewat command `rpc_send` + event `rpc-line`.
- **Bundling GUI CI sukses lintas OS (2026-11-24)**: root cause ganda diperbaiki — `tauri build` dipanggil langsung dari folder `gui/` (bukan lewat `npm --prefix` yang mengubah cwd dan memicu panic `app_paths.rs:133`), dan `icons/icon.ico` terdaftar untuk bundler Windows; runner macOS diganti `macos-latest`. Run `45330035078`: job `gui` hijau di Ubuntu/Windows/macOS, job `release` sukses. Release v0.1.0 kini berisi 17 aset otomatis: `.exe`, `.msi`, `.deb`, `.AppImage`, `.dmg` (+ arsip CLI + sha256), tanpa unggah manual.

- **Rilis GUI (2026-11-24)**: job `gui` di `release.yml` membangun `.exe` (NSIS/
  MSI) Windows, `.deb`/AppImage Linux, dan `.dmg` macOS (lewat runner CI).
  Amandemen RENCANA `A-2026-11-24` mencatat perubahan L-1/O-7/O-8.

### Fixed
- **Portabilitas & QA GUI**: `tauri-gui-xvfb-qa` — bukti screenshot nyata dari
  `tauri build --no-bundle` di Xvfb (`/tmp/maicli-gui.png`), bukan `cargo build`.
  Keluarga Tauri di-pin (2.11.6 / macros 2.6.3 / utils 2.9.3) sesuai skill
  `tauri-pin-2-11-family`.

### Changed
- **Konsistensi rilis + commit publik (2026-11-23)**: komentar header
  `release.yml` menyebut 6 target (bukan 4); `npm-publish.yml` memakai nama paket
  wrapper yang benar (`@modeluxai/cli`) di komentar dan `npm dist-tag`;
  `docs/versioning.md` memindahkan temuan ke status (repo `ModelUXAI/maicli`, tag
  `v0.1.0` lokal, varian npm yang belum terbit). Mode 113 berkas dinormalisasi
  (777 → 644/755) dan `__pycache__/`+`*.pyc` masuk `.gitignore`. Gate: 641 test +
  8 E2E, `/tmp/check-final-2026-11-23.log`.

- **Perbaiki sintaks heredoc npm-publish (2026-11-23)**: `.github/workflows/npm-publish.yml`
  — penutup heredoc `EOF` dikembalikan dan isi JSON diindentasi agar lolos
  `actionlint`/shellcheck (4 error SC1073/SC1072/SC1044/SC1009). Semua blok
  `run:` kini terverifikasi dengan `bash -n`.

- **npm wrapper memakai biner yang benar-benar dirilis (2026-11-23)**:
  `.github/workflows/npm-publish.yml`, `npm/modeluxai/bin/maicli.js`, dan
  `npm/modeluxai/package.json` kini memakai 4 platform yang benar-benar ada di
  GitHub Release (linux-x64, linux-arm64, win32-x64, win32-arm64). Sebelumnya
  hardcoded 4 platform termasuk darwin-arm64/darwin-x64 yang tidak terbit karena
  runner macOS dibatalkan akun GitHub.

- **Rilis tetap terbit bila job macOS dibatalkan (2026-11-23)**:
  `.github/workflows/release.yml` memberi `if: always() && needs.build.result != 'skipped'`
  pada job `release`, supaya artefak dari target yang berhasil tetap dipublikasikan
  saat runner macOS dibatalkan akun GitHub (job macOS sering cancel dengan 0 langkah).

- **Test computer tahan display usang (2026-11-23)**: `crates/tui-computer/src/lib.rs`
  menjalankan `ax_tree()` dan `clipboard_roundtrip` di thread dengan batas 3 dtk; `x11rb::connect` bisa
  menggantung tanpa batas pada container/CI dengan `DISPLAY` usang. Timeout
  dianggap sebagai X11 tidak tersedia (perilaku terdokumentasi).

- **Instal linker arm64 disederhanakan (2026-11-23)**: `.github/workflows/release.yml`
  tidak lagi menjalankan `dpkg --add-architecture arm64` + `apt-get update`
  (sering gagal/timeout di runner); cukup `apt-get install gcc-aarch64-linux-gnu`.

- **Rilis lintas-target tidak menjalankan test (2026-11-23)**:
  `.github/workflows/release.yml` membatasi langkah `cargo test --workspace` hanya
  ke target native `x86_64-unknown-linux-gnu`. Gerbang merah sudah dijamin CI
  pada commit yang sama; menjalankan test di target silang `aarch64-unknown-linux-gnu`
  gagal di harness/test emulator dan membatalkan job rilis lain.

- **ACL Windows final — verifikasi runner hijau (2026-11-23)**: `crates/tui-wire/src/secure_file.rs`
  memakai `icacls /inheritance:r /grant:r <user>:F` (tanpa PowerShell `Set-Acl`,
  yang modul Security-nya gagal dimuat di bawah beban CI). `acl_restricted()`
  membaca output `icacls` dan memeriksa jaminan nyata: ACE pengguna berjalan
  hadir, tanpa ACE turunan `(I)`, tanpa ACE pihak asing (ACE `SYSTEM`/
  `Administrators` bawahan Windows diizinkan). Diagnosa runner Windows dengan env
  CI persis: `exit=True` (semua test hijau). Percobaan `Set-Acl` dan `icacls *SID`
  dibatalkan karena keduanya gagal di runner.

- **Stabilisasi test approval timeout di CI (2026-11-23)**: jendela observasi
  `approval_timeout_menolak_dan_tool_tidak_jalan` diperlebar 5→20 dtk. Diagnosa
  runner Windows (`cargo test` serial) `exit=0`, tetapi saat CI berjalan dengan
  beban paralel test ini kadang kehabisan waktu sebelum approval sampai ke UI.
  Semua assertions tetap sama.

- **Mock SSE path-aware untuk CI Windows (2026-11-23)**: `crates/tui-app/src/tui_mode.rs`
  `spawn_sse_script` kini membedakan `GET /models` (JSON) dari `chat/completions`
  (skrip SSE) dan melayani hingga 32 koneksi. Sebelumnya discovery Windows
  menghabiskan skrip pertama sehingga tool call tidak pernah sampai ke UI
  (`approval_timeout_menolak_dan_tool_tidak_jalan` gagal). Setelah perbaikan ACL
  di putaran sebelumnya, ini satu-satunya kegagalan `test (windows-latest)`.

- **Perbaikan prasyarat rilis Windows (2026-11-23)**: `crates/tui-wire/src/secure_file.rs`
  mengganti `icacls /inheritance:r` (menyisakan 3 ACE SYSTEM/Administrators/user,
  `protected=False` di runner CI) dengan DACL baru via `Set-Acl`: tepat 1 ACE
  FullControl untuk `WindowsIdentity::GetCurrent().Name` + proteksi warisan aktif.
  `acl_restricted()` kini memeriksa SEMUA ACE dan membandingkan identitas proses
  lengkap, bukan hanya `USERNAME` + ACE pertama. `crates/tui-app/tests/e2e_mock_gateway.rs`
  menaikkan kapasitas mock server jadi 32 koneksi (dulu 8) dan buffer 16 KB untuk
  menahan beberapa koneksi reqwest paralel di Windows. `scripts/check-docs-index.sh`
  memakai `pwd -P` agar `git check-ignore` bekerja ketika cwd adalah symlink.
  Verifikasi: `check.sh` hijau 641 test + 8 E2E (`/tmp/check-win2-2026-11-23.log`),
  clippy cross-target Windows hijau (`/tmp/clippy-win-final.log`).
  **Belum diverifikasi di runner Windows nyata** — perlu push + CI.

- **Portabilitas CI lintas platform (2026-11-23)**: `crates/tui-computer/src/lib.rs`
  memberi fallback `daftar_jendela` di luar Linux; `crates/tui-tools/src/media.rs`
  memindah `use super::*` ke dalam test `cfg(unix)`; test `kill_tree_posix` di-gate
  `target_os = "linux"` (bukan `/proc` di macOS); 4 test repo git menetapkan
  `core.autocrlf=false`; test clipboard tidak menganggap clipboard kosong di
  lingkungan tanpa display sebagai kegagalan. Setelah push `7659db5`, CI `fmt`, `clippy (Linux/Windows)`,
  `test (ubuntu)`, `build-release`, `deny`, `msrv`, `actionlint` hijau.
  **Masih merah**: `test (windows-latest)` (ACL runner 3 ACE/protected=False + 3 E2E
  gateway unreachable) dan `test (macos-latest)` (dijalankan sebelum fix; diagnosa
  ulang di branch `debug/ci-logs` → `exit=0`). Bukti:
  `/tmp/check-final3-2026-11-23.log`, `/tmp/clippy-win4.log`.

- **Audit penomoran versi + URL repo (2026-11-23)**: `docs/versioning.md`
  merangkum satu versi workspace 0.1.0, release-plz, tag `vX.Y.Z`; URL repo
  diseragamkan ke `ModelUXAI/maicli`; dokumen rilis 4→6 biner; npm
  `@modeluxai/cli`. Gate: 641 test + 8 E2E.

- **Gate indeks dokumentasi lulus (2026-11-23)**: `check.sh` menjalankan
  `scripts/check-docs-index.sh`; indeks lengkap. Gate: 641 test + 8 E2E,
  `/tmp/check-platform-round43-2026-11-23.log`.

- **Gate tautan dokumentasi lulus (2026-11-23)**: `check.sh` menjalankan
  `scripts/check_doc_links.py`; hasil `tautan dokumentasi ok`. Gate: 641 test +
  8 E2E, `/tmp/check-platform-round42-2026-11-23.log`.

- **Rapikan CHANGELOG (2026-11-23)**: baris kosong antar-bullet di
  `[Unreleased]` dihapus; tidak ada perubahan isi. Tanpa perubahan kode.

- **Audit diff/whitespace (2026-11-23)**: `git diff --check` bersih, tidak ada
  konflik unmerged. Tanpa perubahan kode.
- **Smoke staging/isu (2026-11-23)**: `/staging`, `/accept`, `/tool-issue`,
  `/tool-issues`, `/xd` berfungsi pada data dir kosong. Tanpa perubahan kode.
- **Smoke command baru (2026-11-23)**: `/context-note`, `/plan`, `/artifact
  list`, `/tool-issues` berfungsi pada data dir kosong. Tanpa perubahan kode.
- **Smoke RPC/teks (2026-11-23)**: `--mode rpc` mengeluarkan frame `ready`;
  `--mode text` memuat sesi, `/help`, `/model` pada data dir kosong. Tanpa
  perubahan kode.
- **Audit link dokumentasi (2026-11-23)**: tautan relatif di semua `*.md`
  dicek; satu tautan keluar repo di README diganti teks. Tanpa perubahan kode.
- **Dokumentasi tool baru (2026-11-23)**: `docs/tools-v1.md` memuat tabel tool
  v1.1 + tier + syarat aktif. Tanpa perubahan kode.
- **Sinkronisasi README (2026-11-23 09:52)**: 6 mode, platform rilis arm64,
  tool/SDK baru, dan perintah TUI terbaru. Tanpa perubahan kode.
- **Audit akhir (2026-11-23)**: gate penuh dijalankan ulang — 639 test + 8 E2E,
  `/tmp/check-final-audit-2026-11-23.log`. Tidak ada fitur `❌`; blocker
  eksternal tercatat di `docs/blocked-external.md`.
- **Paket sumber pre-rilis (2026-11-23 09:21)**: arsip
  `/tmp/maicli-pre-release.tar.gz` (789 entry) + SHA256
  `72f327413c0fa7285a8cbe5ac2ea02867545598bdfc8d71442163f5616aa9fda`.
  Push/tag belum dilakukan.
- **Verifikasi akhir (2026-11-23)**: gate penuh dijalankan ulang —
  `check.sh` SEMUA GERBANG HIJAU, 635 test + 8 E2E,
  `/tmp/check-final-2026-11-23.log`. Sisa pekerjaan hanya item eksternal.
- **Rekonsiliasi dokumentasi (2026-11-23 08:18)**: §F40 di
  `docs/platform-dan-integrasi.md` menandai blok "Masih belum" lama sebagai
  snapshot historis dan merangkum status terkini. Tanpa perubahan kode.
- **Bukti pre-UAT (2026-11-23 08:02)**: `docs/uat-v1.md` mencatat bukti U-10
  (config rusak menyebut file+baris+kunci) dan gate 635 test. UAT manual
  U-01..U-12 tetap menjadi wewenang pemilik.

- **Natives arm64 (2026-11-23 08:00)**: matrix rilis menambah
  `aarch64-unknown-linux-gnu` dan `aarch64-pc-windows-msvc`, plus cross-linker
  Linux arm64. YAML tervalidasi; rilis nyata menunggu tag/push.

- **Notarization macOS (2026-11-23 07:17)**: `release.yml` menambahkan
  `notarytool submit --wait` + `stapler staple` opsional setelah codesign,
  aktif bila secret Apple tersedia. YAML tervalidasi; tanpa perubahan Rust.

- **Jalur codesign macOS (2026-11-23 07:14)**: `release.yml` menandatangani
  biner macOS bila secret `APPLE_CERT_P12` tersedia, lalu `codesign --verify`;
  notarization belum. YAML tervalidasi, tanpa perubahan kode Rust.

- **Audit plugin runtime (2026-11-23 06:49)**: status `Plugin manager/runtime`
  diperbarui dari 🟡 ke ✅ setelah diverifikasi: extension loader menjalankan
  tool proses eksternal dan `/plugins list|reload` terbukti memuat manifest
  nyata. Tidak ada perubahan kode.

- **Hook eksternal di `serve` (2026-11-23 06:46)**: pass-through setter pada
  `tui_sdk::Session` memungkinkan `[hooks]` dipasang otomatis pada semua mode,
  termasuk server HTTP lokal. Gate: `check.sh` hijau, 622 test + 8 E2E,
  `/tmp/check-platform-round20-2026-11-23.log`.

- **Automatic external hooks (2026-11-23 06:33 lanjutan)**: hook `before_tool`
  dan `after_tool` dari `[hooks]` kini dipasang otomatis pada sesi mode teks,
  TUI, dan RPC (serve belum). Gate: `check.sh` hijau, 622 test + 8 E2E,
  `/tmp/check-platform-round19-2026-11-23.log`.

- **`docs/platform-dan-integrasi.md` §10.2**: audit lengkap fitur omp yang belum
  ada di TUI (tool, workflow/UX, ekosistem/platform) + prioritas P0–P3;
  rujukan path `oh-my-pi/` diselaraskan, rekomendasi §10.1 yang basi diperbaiki.
- **Sistem rilis GitHub** (2026-11-21): CI dipecah menjadi required checks
  (`fmt`, `clippy` Linux/Windows, `test` Linux/Windows + macOS allow-failure,
  `build-release`, `deny`, `msrv` Rust 1.89, `pr-title`, `dependency-review`);
  versioning otomatis **release-plz** (PR rilis + tag `vX.Y.Z` + CHANGELOG);
  `release.yml` membangun 4 biner (Linux x64, macOS arm64/x64, Windows x64) +
  SHA256 dan dipanggil juga lewat `workflow_call` (tanpa PAT); ruleset
  `main-protection` + script `scripts/setup-github.sh`; template PR/issue +
  Dependabot.
- **`check.sh` menserialkan test** (`RUST_TEST_THREADS=1`; bisa ditimpa) agar
  E2E relay/subagent tidak flaky di mesin yang sibuk; CI/release memakai setelan
  yang sama.
- **`kdl` dipin ke `~6.5`** agar MSRV 1.89 benar-benar dapat dibangun
  (`kdl 6.6+` menuntut Rust 1.95); job `msrv` kini memverifikasinya.
- **Registry command naik dari 33 ke 62** (`CommandSpec` + `CommandResult::Feature`):
  command baru dieksekusi mode TUI/teks lewat `features.rs`; RPC menolak command
  fitur dengan pesan mode yang jelas. `MAICLI_DATA_DIR` kini juga memindahkan
  config global (sebelumnya hanya sesi/log) sehingga sesi portabel benar-benar
  terisolasi; `default_global_path` menghormati env itu.
- **Rekaman event** memakai `Recorder` yang bisa dinyalakan/dimatikan saat sesi
  berjalan; berkas hanya dibuka saat `/record on` (tidak membuat file kosong).
- **MSRV workspace 1.85 → 1.89** dan `rusqlite` 0.41 (SQLite 3.54) menutup
  RUSTSEC-2026-0459; `cargo deny` mengizinkan BSL-1.0 (transitif arboard).
- **Keputusan iqbal (2026-11-18): target paritas dengan omp dilepas.** `docs/paritas.md` menjadi ledger
  bukti perilaku internal (141 baris port + uji); F-01 ditutup; C-12 ditutup dengan 11 golden internal,
  uji mutasi, dan rekaman gateway live. omp tetap rujukan sejarah perilaku di komentar/dokumen.
- Keadaan di status line TUI kini diturunkan dari **`TurnStatus` §7.2** (streaming, menjalankan tool, menunggu
  persetujuan, dibatalkan, error) dan status terminal tidak langsung ditimpa `siap` saat `AgentEnd`.
- Registry slash command kini memakai `CommandSpec { name, usage, aliases, description, allow_args }`:
  `/help` dibangun dari registry, alias `?` → `/help` dan `exit` → `/quit`, dan command yang tidak
  menerima argumen menolak argumen dengan pesan jelas (dulu argumen diabaikan diam-diam).
- `Ctrl+C` di TUI tidak lagi langsung menutup: saat turn berjalan ia membatalkan turn lebih dulu,
  dan saat idle perlu dua tekanan berturut-turut (tombol lain membatalkan konfirmasi). `/quit` atau
  `/exit` yang diketik saat turn berjalan kini membatalkan turn dan meminta dijalankan lagi setelah
  idle, sesuai kriteria C-08/C-09.
- Normalisasi parity kini ikut membandingkan **stop reason** (`message_end:ok:stop|tooluse|length|…`),
  supaya skenario `length`/`toolUse` tidak lagi terlihat identik dengan jawaban biasa.
- `maicli config show` menyebut **sumber** kunci API (`env`, `keyring`, atau `tidak ada`) alih-alih
  selalu menulis "environment", tanpa pernah menampilkan nilainya.
- Seluruh penulisan entry sesi (pesan, compaction, approval, usage, judul, model) dipindah ke
  `spawn_blocking` lewat `session_open::append`, sehingga tidak ada lagi fs sinkron di jalur async —
  melengkapi perpindahan pemuatan awal di putaran sebelumnya (§4).
- Pemuatan awal dipindah ke `spawn_blocking`: muat config, resolusi/resume sesi, dan buka/buat
  store sesi di mode teks, TUI, dan RPC tidak lagi menjalankan fs sinkron di dalam future (§4).

### Added
- **Sistem GitHub open-source + nama publik (2026-11-23)**: `CODEOWNERS`,
  `CODE_OF_CONDUCT.md`, review CODEOWNERS di ruleset, PR template docs, dan
  `docs/nama-publik.md`. Nama final: repo `ModelUXAI/maicli`, paket npm
  `@modeluxai/cli`, biner tetap `maicli`. YAML/script valid; tanpa perubahan Rust.

- **SDK lintas bahasa dipublikasikan (2026-11-23)**: `sdk/go` (module
  `github.com/ModelUXAI/maicli/sdk/go`), `sdk/python`, dan `sdk/typescript`
  berisi client RPC dependency-free + test; dipakai lewat `docs/sdk-lintas-bahasa.md`.

- **Rilis npm `@modeluxai/cli` (2026-11-23)**: `npm/modeluxai/` berisi wrapper
  `bin/maicli.js` (unduh biner rilis + verifikasi sha256) dan
  `.github/workflows/npm-publish.yml` untuk publikasi bertag.

- **Paket rilis lokal Linux x64 (2026-11-23)**: binary release dibangun ulang,
  di-strip, diarsipkan + SHA256; checksum OK dan binary dari arsip berjalan.
  Path `/tmp/maicli-pkg-final/maicli-linux-x64-v0.1.0.tar.gz`.

- **Gate indeks dokumentasi (2026-11-23)**: `scripts/check-docs-index.sh`
  memastikan semua `docs/*.md` terindeks; `setup-github.md` dan
  `subagent-v1.md` ditambahkan ke indeks. Tanpa perubahan Rust.

- **Preflight owner (2026-11-23)**: `scripts/preflight-owner.sh` memeriksa
  prasyarat item eksternal tanpa mencetak kredensial. Tanpa perubahan Rust.

- **Gate tautan dokumentasi (2026-11-23)**: `scripts/check_doc_links.py`
  dijalankan oleh `check.sh` sebelum gate Rust. Tanpa perubahan Rust.

- **Indeks dokumentasi (2026-11-23)**: `docs/README.md` menautkan dokumen
  utama dan menjelaskan snapshot historis. Tanpa perubahan kode.
- **Dokumentasi taksonomi error (2026-11-23)**: `docs/error-taxonomy.md`
  merangkum `MxError`/`ToolOutput::err`/config/session/provider/SDK. Tanpa
  perubahan kode; gate 641 test tetap.
- **Cache `grep` (2026-11-23 10:11)**: hasil `grep` memakai TTL 5 dtk seperti
  `glob`; 1 test. Gate: 641 test + 8 E2E,
  `/tmp/check-platform-round41-2026-11-23.log`.

- **Tool `report_tool_issue` (2026-11-23 10:10)**: laporan isu dari dalam turn,
  JSONL + tampil lewat `/tool-issues`; 1 test. Gate: 640 test + 8 E2E,
  `/tmp/check-platform-round40b-2026-11-23.log`.

- **Owner handoff (2026-11-23)**: `docs/owner-handoff.md` memuat perintah persis
  untuk item eksternal (live gateway, UAT, push/tag, rotasi kunci, relay,
  macOS, keputusan SDK TS). Tanpa perubahan kode; gate 639 test tetap.

- **Daftar blocker eksternal (2026-11-23)**: `docs/blocked-external.md`
  merinci item yang butuh pemilik/lingkungan + bukti penerimaan. Tanpa
  perubahan kode; gate 639 test tetap.

- **`computer` AX tree (2026-11-23 09:40)**: action `ax` (X11) menampilkan
  jendela terlihat + geometri. Gate: 639 test + 8 E2E,
  `/tmp/check-platform-round39-2026-11-23.log`.

- **`computer.run` (2026-11-23 09:37)**: urutan 1..32 langkah JSON dalam satu
  panggilan tool, stop-on-error, run bersarang ditolak. Gate: 638 test + 8 E2E,
  `/tmp/check-platform-round38-2026-11-23.log`.

- **`computer` windows + clipboard (2026-11-23 09:21)**: action `windows`
  (X11) dan `clipboard_read`/`clipboard_write`; 1 test clipboard. Gate: 637 test
  + 8 E2E, `/tmp/check-platform-round37-2026-11-23.log`.

- **Memori kaya (2026-11-23 09:21)**: action `memory` `retain`/`recall`/
  `reflect`/`learn`/`edit` (sintesis sederhana + pelajaran). Gate: 636 test +
  8 E2E, `/tmp/check-platform-round36-2026-11-23.log`.

- **Runbook host relay publik (2026-11-23 08:15)**: `docs/relay-public-host.md`
  berisi unit systemd, vhost nginx TLS+WS, kunci 0600, verifikasi, dan rotasi
  kunci. Tidak ada perubahan Rust; host nyata tetap wewenang pemilik.

- **TTSR retry (2026-11-23 08:02)**: setelah pola larangan menghentikan stream,
  aturan disuntikkan dan turn diulang sekali; bila retry juga memicu pola, turn
  berhenti error. 2 test core. Gate: 635 test + 8 E2E,
  `/tmp/check-platform-round35-2026-11-23.log`.

- **TTSR stream guard (2026-11-23 07:54)**: `[hooks] ttsr_rules` menghentikan
  stream saat delta teks memicu pola larangan (test di core). Gate: 634 test +
  8 E2E, `/tmp/check-platform-round34-2026-11-23.log`.

- **Prewalk (2026-11-23 07:52)**: `/prewalk [langkah]` mengantrekan telusur
  rencana read-only sebelum eksekusi. Gate: 633 test + 8 E2E,
  `/tmp/check-platform-round33-2026-11-23.log`.

- **Watchdog/TTSR dasar (2026-11-23 07:47)**: `/watchdog <pola1,pola2>`
  menyuntik aturan larangan + checker `perlu_stop`. Gate: 632 test + 8 E2E,
  `/tmp/check-platform-round32b-2026-11-23.log`.

- **Autoresearch (2026-11-23 07:44)**: `/research <pertanyaan> [2..5]`
  mengantrekan putaran riset read-only. Gate: 631 test + 8 E2E,
  `/tmp/check-platform-round31b-2026-11-23.log`.

- **Guided goal + think/yield (2026-11-23 07:40)**: `/guided-goal` mengantrekan
  langkah pertama; `/think`/`/yield` menyuntik kontrol ke konteks. Gate:
  630 test + 8 E2E, `/tmp/check-platform-round30b-2026-11-23.log`.

- **Goals/loop (2026-11-23 07:37)**: `/goal <teks|clear>`, `/loop <n> <prompt>`
  (1..20), `/ratchet [prompt]` memakai antrean follow-up. Gate: 629 test + 8 E2E,
  `/tmp/check-platform-round29b-2026-11-23.log`.

- **Magic keywords (2026-11-23 07:27)**: `[hooks.magic_keywords]` menyuntikkan
  instruksi tambahan ke konteks bila kata kunci muncul di pesan pengguna
  (context_sync semua mode). Gate: 628 test + 8 E2E,
  `/tmp/check-platform-round28b-2026-11-23.log`.

- **Preview → accept (2026-11-23 07:24)**: tool `stage_write` menyimpan
  perubahan di `<data>/staging`; `/staging` daftar, `/accept <id>` menulis
  atomik ke target dalam cwd. Gate: 627 test + 8 E2E,
  `/tmp/check-platform-round27-2026-11-23.log`.

- **System prompt override (2026-11-23 07:20)**: `/system-prompt <teks|clear>`
  menyimpan `<data>/system-prompt.md` dan menyuntikkannya ke system prompt.
  Gate: 626 test + 8 E2E, `/tmp/check-platform-round26-2026-11-23.log`.

- **Public URL relay (2026-11-23 07:11)**: `/api/relay-info` dan QR memakai
  `collab.remote_url` bila relay dipasang di belakang host publik; fallback
  alamat lokal. Gate: 625 test + 8 E2E,
  `/tmp/check-platform-round25-2026-11-23.log`.

- **Rotasi kunci relay (2026-11-23 07:07)**: `POST /api/relay/revoke` dengan
  `x-relay-key` merotasi kunci room (kunci disimpan di `Mutex`); smoke kunci
  lama → baru, kunci salah → 403. Gate: 625 test + 8 E2E,
  `/tmp/check-platform-round24-2026-11-23.log`.

- **Link/QR relay lokal (2026-11-23 07:03)**: endpoint `/api/relay-info`
  (URL, room, QR SVG) dan tampilan link/QR di halaman web relay. Gate:
  `check.sh` hijau, 625 test + 8 E2E, `/tmp/check-platform-round23-2026-11-23.log`.

- **Klien web relay lokal (2026-11-23 06:59)**: port relay kini melayani halaman
  HTML+JS untuk join room/kunci via WebSocket; koneksi WS tetap berfungsi.
  Smoke `curl` + gate: 624 test + 8 E2E,
  `/tmp/check-platform-round22-2026-11-23.log`.

- **Tool `media` (2026-11-23 06:53)**: TTS/generasi gambar generik lewat
  `[media] tts_command|image_command` (prompt di stdin, hasil ke berkas, env
  kredensial dibuang, timeout 30 dtk). Gate: `check.sh` hijau, 623 test + 8 E2E,
  `/tmp/check-platform-round21b-2026-11-23.log`.

- **Blob artifacts + external hooks (2026-11-23 06:33)**: `/artifact add|list|get`
  (SHA-256 + metadata) dan `[hooks]` dengan `/hook-test <event>` (stdin JSON,
  timeout 5 dtk). Gate: `check.sh` hijau, 622 test + 8 E2E,
  `/tmp/check-platform-round18-2026-11-23.log`.

- **Hashline + resolusi konflik (2026-11-23 06:28)**: tool `hashline_edit`
  (anchor/hash baris + proposal/apply) dan `/conflict` (`conflict://N`,
  `ours|theirs|base`). Gate: `check.sh` hijau, 619 test + 8 E2E,
  `/tmp/check-platform-round17-2026-11-23.log`.

- **`xd://issue` + broker OAuth (2026-11-23 06:19/06:21)**: resolver URI
  internal untuk isu tool; broker OAuth 2.1 generik di `maicli serve`
  (`[oauth.broker.<nama>]`, authorization code + PKCE, token di memori).
  Gate: `check.sh` hijau, 615 test + 8 E2E,
  `/tmp/check-platform-round16b-2026-11-23.log`.

- **Bun kernel + isu tool (2026-11-23 06:13)**: `eval lang="js"` memakai
  `bun repl` (state lintas-call), `/tool-issue`, `/tool-issues`, dan
  `/tool-issue-resolve` dengan injeksi system prompt. Gate: `check.sh` hijau,
  612 test + 8 E2E, `/tmp/check-platform-round14-2026-11-23.log`.

- **PDF text + browser evaluate (2026-11-23 05:59)**: ekstraksi teks PDF per
  halaman (`lopdf`+`flate2`) pada `read`, dan `Runtime.evaluate` lewat CDP WS
  pada tool `browser`. Gate: `check.sh` hijau, 610 test + 8 E2E,
  `/tmp/check-platform-round13b-2026-11-23.log`.

- **`eval` + `browser` (2026-11-23 05:53)**: kernel Python persisten per
  `kernel_id` (state lintas-call, timeout) dan kendali tab browser lewat CDP
  HTTP lokal (`tabs`/`new`/`close`). Gate: `check.sh` hijau, 608 test + 8 E2E,
  `/tmp/check-platform-round12-2026-11-23.log`.

- **Keybinding kustom (2026-11-23 05:49)**: `[ui.keybindings]` untuk 6 aksi TUI
  (cancel/palette/help/clear_input/tool_details/hub) + parser tombol teruji.
  Gate: `check.sh` hijau, 604 test + 8 E2E, `/tmp/check-platform-round11-2026-11-23.log`.

- **`/task-worktree` (2026-11-23 05:44)**: task di worktree git terisolasi
  dengan hasil JSON berskema (`status`, `task_id`, `worktree`, `output`) dan
  pembersihan otomatis. Gate: `check.sh` hijau, 603 test + 8 E2E,
  `/tmp/check-platform-round10-2026-11-23.log`.

- **`wait` + `fs-scan-cache` (2026-11-23 05:37)**: tool penunggu subagent
  dengan timeout, dan cache TTL 5 dtk untuk hasil `glob`. Gate: `check.sh`
  hijau, 601 test + 8 E2E, `/tmp/check-platform-round9b-2026-11-23.log`.

- **Plan mode & utilitas kerja (2026-11-23 05:29)**: `/plan` (`append`/`clear`)
  + `/plan-review` dengan injeksi system prompt, `/diagnose` (status git +
  anggaran), `/compare [ref]`, dan `/skill-new|rm|list`. Gate: `check.sh`
  hijau, 598 test + 8 E2E, `/tmp/check-platform-round8b-2026-11-23.log`.

- **`/install-id` (2026-11-23 05:24)**: id instalasi stabil lokal di
  `<data>/install-id` (0600), tanpa telemetri/jaringan. Gate: `check.sh` hijau,
  594 test + 8 E2E, `/tmp/check-platform-round7-2026-11-23.log`.

- **`read` diperluas + `find` (2026-11-23 05:16)**: ringkasan SQLite (read-only),
  daftar entry ZIP/TAR, metadata PDF, header biner; tool `find` memilih path dari
  daftar kandidat nyata lewat model. Dependensi: `rusqlite` di `tui-tools`
  (alasan di `DEPENDENCIES.md`). Gate: `check.sh` hijau, 593 test + 8 E2E,
  `/tmp/check-platform-round6b-2026-11-23.log`.

- **Item lanjutan `platform-dan-integrasi.md` (2026-11-23 05:08)**: `/notifications`
  (JSONL + system prompt), `/prompts` (`use`/`clear` dari `<cwd>/prompts/*.md`),
  dan tool `debug` DAP generik lewat config `[debug]` (breakpoints/stack/evaluate/
  continue; mock adapter). Wiring LSP di `Boot::tool_registry()` juga dipulihkan.
  Gate: `check.sh` hijau, 588 test + 8 E2E, `/tmp/check-platform-round5b-2026-11-23.log`.

- **Item lanjutan `platform-dan-integrasi.md` (2026-11-23 04:57)**: tool
  `security_scan` (8 pola kredensial + redaksi), workspace multi-root
  (`/add-dir`, `/dirs`; `glob`/`grep` memakai semua root), `/edit` (editor
  eksternal `VISUAL`/`EDITOR`), `/budget` (token + sisa konteks). Gate:
  `check.sh` hijau, 584 test + 8 E2E, `/tmp/check-platform-round4b-2026-11-23.log`.

- **Pencarian web & GitHub (2026-11-23)**: tool `web_search` (DuckDuckGo HTML
  default, backend Brave via `BRAVE_SEARCH_API_KEY`) dan tool `github` dengan
  `read pr://owner/repo/n` / `issue://…` (token opsional `GITHUB_TOKEN`); test
  parser + server loopback. Gate akhir: `check.sh` hijau, 579 test + 8 E2E,
  `/tmp/check-platform-round3-2026-11-23.log`.

- **Lanjutan P0/P1 `platform-dan-integrasi.md` (2026-11-23)**: impor context files
  (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `.clinerules`, `.mdc`, Copilot) ke system
  prompt; tool `fetch` + `read` URL (HTML→markdown, cap 64 KiB); klien LSP generik
  `[lsp]` (diagnostics/references/definition/rename/hover); `ast_grep`/`ast_edit`
  dengan proposal dua langkah; `/context-note` + `/new-context`; `/checkpoint` +
  `/rewind` (patch git + pemotongan sesi); `/review [save]` + `/annotate`.
  Bukti: `check.sh` hijau, 573 test + 8 E2E, `/tmp/check-platform-round2d-2026-11-23.log`.
- **Konfigurasi baru**: `[lsp]` (enabled/command/args/timeout_secs) didokumentasikan
  di `docs/config-v1.md`; tidak ada crate baru untuk putaran ini.

- **Lanjutan platform & integrasi (2026-11-23)** — SDK RPC Go/Python/TypeScript
  dependency-free di `sdk/` + kebijakan semver (`docs/sdk-lintas-bahasa.md`);
  tool `acp_subagent` (agen ACP eksternal via config `[acp_client]`) dengan
  scrub kredensial/cancel/timeout + test child mock; `/export zip` (arsip
  stored ZIP tanpa dependensi: `transcript.md`, `session.jsonl`, `metadata.txt`);
  `/computer script [JSON]` (maks. 32 langkah, stop-on-error); frame RPC generik
  `ui_dialog_request`/`ui_dialog_answer`; multi-sesi live `maicli serve`
  (`session_id`, daftar, hapus, batas 16); `collab.remote_url` untuk
  `/collab join <room> <kunci>`. Bukti: `check.sh` hijau, 553 test + 8 E2E (1 live ignored)
  (`/tmp/check-platform-2026-11-23-final.log`), smoke `/export zip` memakai
  `zipfile.testzip()` → `None`, test SDK go/python/node hijau.
- **Dependensi (2026-11-23)**: `url` dinaikkan ke 2.5.9 (RUSTSEC-2026-0592);
  tidak ada crate ZIP baru — penulis arsip diimplementasikan lokal (stored).
- **Protokol lanjutan (2026-11-21)**: MCP `ping` kaya (server/protokol/uptime/roots)
  + `roots/list` mengumumkan workspace; RPC `set_ask_dialog`/`ask_dialog`/
  `ask_dialog_answer` (opsi + teks), `set_subagent_subscription` +
  push `subagent_event` (roster 300 ms). Relay **multi-room** (`room=kunci`),
  peran **View** read-only, failover control, dan **klien guest TUI read-only**
  (`/collab join <url> <room> <kunci>`) yang memproyeksikan siaran ke timeline.
- **`/mm` otomatis** (`memory.auto_refresh_models` + `auto_refresh_secs`),
  `/mm refresh all`, `/export anon` (redaksi kredensial + path), `/todo
  copy|export|import` + `append|start|drop`, `/delete <id>`, `/btw|/tan|/omfg|/cleanse`,
  `/mcp add|test|enable|disable`, advisor gateway kedua (`[advisor] provider/model/api_key_env`).
- **Grup 1 audit omp (13 command)**: `/fresh` (sesi baru + hapus grant sesi),
  `/pin`/`/unpin` (sematan pesan 1-based, entry `pin` di sesi + tanda 📌),
  `/jobs` (daftar subagent/job), `/git` (status git repo), `/local` (snapshot
  setting aktif), `/browser`/`/copy`/`/open` (URL/path/clipboard; path relatif
  dihitung dari cwd sesi), `/trace`/`/dump`/`/debug` (permintaan model terakhir
  dan state sesi ke `<config_root>/debug/`), `/elide` + `/shake thinking|images`
  (pangkas konteks tanpa model), `/modelpreset` (simpan/pakai/hapus preset
  model+effort), `/skillful on|off` (saklar blok skill di system prompt),
  `/collab`/`/live` (relay WS in-TUI + kunci room), `/mcp reauth` (OAuth 2.1
  PKCE + loopback + token 0600).
- **Grup 2**: memori persisten dengan backend **pilihan** `jsonl`/`markdown`/
  `sqlite` + embedding lokal hash 384-d (`tui-memory`), command `/memory
  view|add|search|forget|stats|backend|embedding` + tool `memory`; `/mm`
  mental model Markdown + refresh dari memori; marketplace extension
  (install/update/remove dari git, indeks HTTPS, `allow_insecure` untuk repo
  lokal) + `/plugins reload`; tool `ssh` (host bernama, BatchMode, timeout,
  output cap, tier Exec) + `/ssh add|list|remove|test`; `/advisor on|off|model`
  (review model kedua setelah turn); tool `computer` + `/computer
  status|screenshot|move|click|scroll|type|key` (`enigo` + X11/`xcap`);
  `/branch` dan `/fork pane|window|tab` (tmux/wezterm/kitty); `/fast`/`/slow`
  (`service_tier` priority/default); `/record on|off` (rekam event JSONL saat
  berjalan).
- **`cargo deny`**: `BSL-1.0` diizinkan (clipboard-win/error-code via arboard);
  `rusqlite` naik ke 0.41 (SQLite 3.54, menutup RUSTSEC-2026-0459) → MSRV 1.89.
- **`MAICLI_DATA_DIR`** kini juga memindahkan config global (config, sesi,
  marketplace, token MCP, rekaman) ke satu folder portabel.
- **Kelompok 2 (audit omp)**: `/stats`, `/changelog`, `/setup` +
  **`maicli --setup`** (wizard kapan pun), `/settings`, `/logout` (hapus kunci
  keyring), `/mcp list|remove <id>` (hapus = tulis ulang config global dengan
  validasi ketat), `/export [berkas]` (transkrip Markdown), `/handoff`
  (ringkas + instruksi sesi baru), `/commit` (prompt khusus; approval git tetap
  berjalan), `/todo add|rm <no>|done <no>|clear`.
- **7 command baru** (kelompok kecil audit omp): `/clear`, `/retry`,
  `/skills`, `/hub`, `/queue` (antrean prompt saat sibuk, otomatis dijalankan
  setelah turn selesai), `/extended-context on|off`, `/force` (yolo satu turn,
  approval pulih otomatis); **`/effort low|medium|high`** dikirim sebagai
  `reasoning_effort` ke gateway.
- **Wizard setup awal (first-run)**: tanpa kunci → pilih provider → kunci tersembunyi
  → keyring → discovery model → config tertulis; jalan di Windows/Linux.
- **Sistem GitHub**: workflow CI (matriks Linux+Windows, gerbang penuh) + Release
  otomatis dari tag v* (build 3 platform + catatan dari CHANGELOG) + LICENSE
  MIT + CONTRIBUTING + SECURITY + panduan setup.
- **Packaging Linux x64**: `maicli-0.1.0-linux-x64.tar.gz` (release + strip, 3,8 MB).
- **kill-tree POSIX**: child = leader proses group (`process_group(0)`) + `kill -9 -- -pgid`
  (tanpa unsafe); test pohon 2 level di Linux nyata.
- **Keyring Linux** (Secret Service) + **UI interaktif**: `/model` picker,
  `/provider add` form bertahap (simpan `[providers_eksternal.<id>]` di config global,
  kredensial tetap env-only).
- **Verifikasi Linux native**: `check.sh` SEMUA GERBANG HIJAU di server iqbal
  (x86_64; 501 test + 9 E2E); \cl_restricted\ POSIX kini memeriksa mode 600;
- **ACP transport (subagent D)**: `maicli --mode acp` — agen sebagai server ACP
  (JSON-RPC line-delimited stdio) untuk host eksternal dan lokal; multi-sesi + cancel.
- **Extensions (v1)**: loader extension eksternal — folder + manifest `extension.json`
  ketat; tool proses dengan env dibersihkan + approval Exec + nama `ext.<ext>.<tool>`;
  config `[extensions] enabled` + flag `--no-extensions`.
- **Agent Orchestrator**: tool `orchestrate` memunculkan banyak subagent paralel/berurutan;
  semua anak terlihat di Agent Hub.
- **Integrasi katalog runtime (v1)**: config `[catalog] path`, flag `--catalog <DIR>`,
  command interaktif `/catalog` (picker provider + validasi model bawaan + hint env).
- **Agent Hub v2**: transkrip asisten anak (toggle `[t]`) + broadcast steer ke semua
  anak berjalan (`[b]`).
- **`maicli serve` `GET /api/subagents`**: daftar subagent (status/turns) lewat HTTP API.
- **RPC `replay_spool`** (v2): klien menarik ulang frame yang tertahan di spool.
- **RPC `subagent_list`** (v2): klien memantau daftar subagent (status/turns/tool_calls);
  tool `subagent` kini aktif di mode RPC.
- **Event anak → induk (subagent tahap C)**: `SubagentHandle::events()` — seluruh event
  sesi anak diteruskan dengan retensi tunggakan 200.
- **SSE legacy notifikasi**: pesan tanpa id di stream GET diteruskan; tunggakan
  pra-langganan disimpan (maks 100).
- **MCP HTTP notifikasi long-lived + reconnect**: GET stream SSE dengan sambung ulang
  otomatis (EOF → 500 ms); server yang menolak stream dibiarkan diam.
- **RPC `login` → `login_status`** (v2): cek kesiapan kunci env provider tanpa
  membocorkan nilainya.
- **RPC `set_config` → `config_update`** (v2): klien mengganti `models.default` runtime
  (validasi discovery); kunci lain ditolak bernama.
- **Hook `before_tool`** (SDK/`AgentSession`): observasi (nama, preview argumen) sebelum
  eksekusi tool, setelah approval lolos.
- **KDL tahap 5 — `cari_axis`**: resolusi cascade → axis untuk (provider, model) dengan
  pencocokan glob family dan spesifisitas menimpa.
- **KDL tahap 3**: stratum **runtime/behavior** — heuristik OpenAI-Responses + evaluasi,
  provider pensiun/isolasi/append-only (oracle omp nyata).
- **KDL tahap 2**: strata **classes + taxonomy** ter-parse (40 aturan; namespace/family/override/
  revision; oracle omp nyata). Sisa: runtime/cascade.
- **SDK `AgentRegistry`**: sesi paralel dalam satu proses — daftarkan builder per id,
  bangun (ulang), daftar, hapus.
- **RPC `command` → frame `command_output`** (v2): klien menjalankan perintah slash read-only
  (usage/context/todo/tree/model) lewat protokol.
- **RPC `set_title` + frame `session_info_update`** (v2): klien mengganti judul sesi dan
  menerima info sesi (id/model/judul) setelah `set_title`, `model`, dan `open_session`.
- **Hook internal `after_tool`** (`AgentSession::set_after_tool`): observasi (nama tool,
  is_error, cuplikan hasil) di semua jalur eksekusi — sukses, gagal, ditolak approval, paralel.
- **Notifikasi MCP di timeline**: `[mcp:<server>]` diteruskan ke TUI/teks; task penerus
  berhenti sendiri (idle-timeout 3×2 dtk) + abort saat keluar — anti-deadlock.
- **SSE push `GET /api/events/stream`**: tamu menerima event riwayat realtime (Last-Event-ID,
  koneksi ditahan sampai putus).
- **Token serve otomatis**: `MAICLI_SERVE_TOKEN` env atau token acak di `<data>/serve-token` (0600/ACL)
  — server HTTP lokal tidak pernah berjalan tanpa otentikasi.
- **Session tree**: `tui_session::pohon` (relasi fork dari header) + perintah **`/tree`**
  di mode teks dan TUI (indentasi per kedalaman cabang).
- **Skills v1**: folder `skills/<nama>/SKILL.md` (+`MAICLI_SKILLS_DIR`) — daftar disuntik ke system prompt,
  tool `skill` membaca isi (tier Read).
- **Serve stateful**: transkrip+usage tersimpan antar request; `GET /api/session` (metadata),
  `POST /api/abort` (cancel token turn aktif); prompt saat sibuk → 409.
- **RPC `abort_and_restore_queue` (v2)**: abort turn berjalan dan seluruh antrean dikembalikan klien
  lewat frame `queue_restored` (antrean dikosongkan).
- **RPC antrean lanjutan (v2)**: `remove_queued_message` dan `promote_queued_message`
  (posisi antrean saat turn berjalan; notice + error posisi tidak ada).
- **secure_file POSIX**: file sesi/config/grant di Linux/macOS kini chmod 0600 (test unix; sebelumnya no-op).
- **`tui-katalog`** (F-06 tahap 1): kompilator KDL katalog omp — strata providers (98 aturan dari
  105 berkas) + auth (108) ter-parse dan terverifikasi terhadap katalog nyata.
- **Collab v1 (polling)**: `GET /api/events` — tamu memantau riwayat event sesi server
  (prompt/jawab/turn/tool/notice, 1000 terakhir). WebSocket relay penuh menyusul (butuh dependensi WS).
- **API server lokal**: `maicli --mode serve --port` — HTTP/1.1 minimal di loopback (tanpa dependensi baru):
  `GET /api/health`, `POST /api/prompt` (satu turn penuh), `GET /api/sessions`; auth opsional Bearer
  via `MAICLI_SERVE_TOKEN`; tool tier manusia ditolak otomatis di server tanpa UI.
- **RPC extension UI sub-protocol (v2)**: `set_ui_extension` — klien melanggani event sesi
  (`turn.start`/`turn.end`/`tool.start`/`tool.end`) dan menerima frame `ui_extension`.
- **MCP auth header + retry**: `mcp.servers.x.headers` (nilai `env:NAMA`; tidak pernah dicatat log),
  dan retry satu kali untuk 502/503/504 pada transport HTTP.
- **MCP legacy SSE (2024-11-05)**: `connect_sse` (GET event-stream → `endpoint` → POST; balasan
  di-stream GET, korelasi per id) + config `mcp.servers.x.url` dan `transport = "http"|"sse"`.
- **Spool backpressure RPC**: kanal keluar bounded 4096; frame yang tidak terkirim (kanal penuh/
  stdout putus) ditulis ke `rpc-spool-<pid>.jsonl` di direktori data (batas 64 MiB) — tidak ada
  frame hilang diam-diam.
- **RPC sesi/antrean**: `get_state` (frame `state`: model, pesan, busy, antrean, usage), `open_session`
  (id/prefix/terbaru; mengganti sesi aktif), dan **antrean prompt saat turn berjalan** (maksimum 8,
  dijalankan berurutan setelah turn selesai; penuh → `E_AGENT_BUSY`).
- **Subagent tahap C**: panel **F3** di TUI (mini Agent Hub) — daftar subagent dengan status,
  jumlah turn/tool, dan potongan hasil; tool `subagent` terhubung ke panel lewat `with_manager`.
- **Subagent tahap B**: `SubagentManager`/`SubagentHandle` — kontrol anak live (steer/abort/park/resume/kill),
  enam status siklus hidup, dan lineage sesi anak (`parent_session`) via `SessionStore::create_with_parent`.
- E2E **tool MCP lewat turn agent penuh** (`mode_teks_tool_mcp_lewat_turn_penuh`): server MCP PowerShell
  sungguhan via config proyek `[mcp.servers.e2e]`, model memanggil tool MCP, hasilnya kembali di jawaban.
- **Registry provider** (`tui-wire::providers`): **ModeluXAI** (bawaan, `https://api.modeluxai.com/v1`,
  kunci `MODELUXAI_API_KEY`) + 45 provider katalog omp (OpenAI, OpenRouter, DeepSeek, zAI/GLM, Groq,
  Moonshot, xAI, Mistral, Together, Fireworks, Cerebras, Gemini, Ollama/LM Studio/vLLM lokal, dll.).
  Pengguna cukup memasukkan **API key** (env var atau keyring lewat `maicli config key-import`). Command
  baru **`/provider`**: daftar provider dan ganti provider aktif (menulis config global; restart
  diperlukan sesuai kebijakan v1).
- **Subagent lokal tahap A (`tui-subagent`)**: tool `subagent` yang menjalankan sesi anak terisolasi
  (tool + approval + deadline sendiri) dan mengembalikan jawaban akhirnya ke induk; bertier `exec`,
  gagal anak tidak menjatuhkan induk, tidak bersarang. 5 test; dokumen `docs/subagent-v1.md`.
- **MCP Streamable HTTP**: `connect_http` dengan pelacakan `Mcp-Session-Id`, respons JSON atau SSE
  (korelasi id, `[DONE]` dilewati), dan `close()` (DELETE sesi); backend koneksi stdio/HTTP bersatu
  di `McpConnection`. **Peta P1 integrasi dasar selesai** (`docs/platform-dan-integrasi.md`).
- **Host URI schemes RPC (v2)**: `set_host_uri_schemes` (validasi nama skema, ganti set penuh) dan
  sub-protokol `host_uri_request`/`host_uri_result` untuk baca/tulis "berkas virtual" milik host;
  wiring ke tool file agent kini terpasang: `read`/`write` memantulkan URL skema host lewat
  `ToolContext.uri_bridge` (`UriResolver`) ke klien RPC.
- **Host tools RPC (v2)**: host mendaftarkan tool sendiri lewat `set_host_tools`; permintaan eksekusi
  model diteruskan sebagai `host_tool_call` dan dijawab `host_tool_result`; tool host bertier `exec`
  (approval tetap berlaku), timeout 120 detik. `ToolRegistry` dapat `replace_named`/`remove_named`.
- **RPC v1.1**: frame `prompt_result` (`status` ok/aborted/error, `agent_invoked`) dikirim setelah
  `result` dengan `id` sama, lalu `session_settled` menandai sesi siap prompt berikutnya.
- **`tui-mcp` (MCP stdio)**: klien handshake `initialize`/`tools/list`/`tools/call`, bridge tool
  `mcp.<server>.<tool>` bertier `exec`, timeout per request, env kredensial tidak diwariskan; 4 test dengan
  mock server MCP PowerShell. Terhubung lewat config `[mcp]`/`[mcp.servers.*]` (validasi ketat), boot `mcp_boot::connect_all`
  (server gagal tidak menjatuhkan aplikasi), registry semua mode, catatan status, dan label timeline
  `mcp`. Dokumen `docs/mcp-v1.md` + bagian `[mcp]` di `docs/config-v1.md`.
- **RPC v2 transport**: negosiasi versi (`ready.supported_protocol_versions`, `negotiate_protocol`,
  frame `negotiated`) dan chunking frame besar (`rpc_chunk` base64, reassembly sampai 64 MiB) dengan
  encoder/decoder tervalidasi; v1 tetap default dan kompatibel. Dokumen `docs/rpc-v2.md`.
- **SDK Rust v1 (`tui-sdk`)**: `SessionBuilder`/`Session`/`EventStream` untuk embedding agen
  tanpa TUI — provider/approver wajib, tool set bisa diganti, event dua tingkat, abort/pause/model.
  5 test + 1 doc-test; dokumen `docs/sdk-rust-v1.md`; status P1 di `docs/platform-dan-integrasi.md`.
- Dokumentasi `docs/platform-dan-integrasi.md`: matriks dukungan Windows/Linux/macOS native, SDK, API,
  RPC, WebSocket, MCP, dan agent-to-agent; analisis kesenjangan terhadap omp 18.22.0; peta jalan
  pembangunan P1–P3 beserta kriteria penerimaan dan keputusan scope yang diperlukan.
- Korpus golden baseline bertambah: `tiga-turn` (teks → tool_call → follow-up, 3 panggilan model) dan
  `abort` (pembatalan di tengah stream), total **11 fixture** (baseline sendiri, bukan omp).
- Status line TUI kini menampilkan **mode approval** (`tanya`/`tanya+cache`/`tulis`/`yolo`) sesuai C-09,
  di samping model, persen konteks, keadaan, dan status.
- Mode teks kini setara TUI untuk `/quit` saat turn berjalan: perintah itu **membatalkan turn** lalu meminta
  dijalankan lagi setelah idle, bukan ditolak sebagai “masih sibuk”.
- Mode teks kini menangani **Ctrl+C**: saat turn berjalan membatalkan turn (seperti TUI), saat idle tekanan
  pertama meminta konfirmasi dan tekanan kedua keluar. Fitur `tokio` `signal` ditambahkan.
- Konsistensi mode teks: baris yang diketik **saat turn berjalan** kini ditolak dengan pesan “masih sibuk;
  kirim ulang…” alih-alih diantrekan diam-diam sebagai prompt berikutnya (setara TUI/RPC); jawaban untuk
  tool `ask` tetap diproses.
- RPC `model` kini **divalidasi terhadap daftar discovery** seperti `/model` di TUI/teks: id tak dikenal ditolak
  `E_MODEL_UNKNOWN` tanpa mengubah model; daftar kosong (discovery gagal) meneruskan id apa adanya.
- Test `pause_setelah_tool_menghentikan_turn_sebelum_follow_up`: jeda yang dipicu dari dalam tool menghentikan
  turn sebelum follow-up diproses (sesuai “pause gate di batas turn” RENCANA C-03); memutus stream yang
  sedang berjalan tetap tugas `cancel`.
- **Penjagaan versi skema sesi**: file dengan `version` lebih baru dari yang didukung (atau versi 0) ditolak
  dengan pesan yang menyebut versi file dan versi implementasi; kebijakan kompatibilitas didokumentasikan
  di `docs/skema-sesi-v1.md`. Migrasi belum diperlukan karena tidak ada format sesi lama di v1.
- **Grant expiry** (deviasi kecil dari C-05, dicatat): `approval.grant_ttl_hours` (0 = tidak kedaluwarsa);
  grant `always` menyimpan `expires_at_ms`, grant basi tidak di-seed dan dipangkas saat start, file grant
  lama tanpa field itu tetap kompatibel.
- Smoke **live gateway** `#[ignore]` (`crates/tui-app/tests/live_gateway.rs`): discovery `/v1/models` lebih dulu
  (model tidak ditebak), lalu satu turn streaming; hanya jalan dengan `--ignored` dan kunci nyata.
- Hook **syncContextBeforeModelCall** (C-03): AgentSession::set_context_sync menerima pengganti konteks
  sebelum compaction/panggilan model (hasil kosong diabaikan).
- Metadata **`AgentTool`** (C-04): `label`, `loadMode`, `render` dipisah dari definisi wire; TUI memakai label
  dan petunjuk render (❯ perintah, ± diff, → biasa) di timeline, termasuk tool tak dikenal.
- `check.sh`: padanan POSIX `check.ps1` (fmt/clippy/test/deny + aturan dependensi), memenuhi C-01;
  belum dieksekusi di mesin ini karena Windows tanpa bash/WSL — dicatat apa adanya di `WORK.md`.
- Test **E2E redaksi kunci**: binary dijalankan dengan `MODELUXAI_API_KEY` palsu, lalu seluruh berkas di
  `MAICLI_DATA_DIR` (sesi, log, cache) dipindai — tidak ada yang memuat kunci (C-06 kriteria 5).
- Test **E2E approval mode teks lewat stdin**: tool `write` diminta model, prompt `[persetujuan]` dengan
  **argumen verbatim** tampil, jawaban `y` di stdin membuat tool berjalan dan berkas hasil tertulis.
- Override direktori data **`MAICLI_DATA_DIR`** (portable/test): memindahkan config, sesi, log, dan cache
  tanpa menyentuh profil pengguna; dipakai test E2E agar benar-benar terisolasi.
- Test **E2E crash di tengah turn**: proses `maicli` dibunuh saat streaming menggantung; berkas sesi yang
  sudah ditulis tetap bisa dibaca (`SessionStore::load`), header utuh, pesan pengguna bertahan.
- `--event-log` kini **di-flush sebelum proses keluar** (`event_log::flush`), sehingga transkrip lengkap
  dan deterministik.
- Test **E2E mode RPC + approval**: binary `maicli --mode rpc` melawan gateway tiruan, model meminta tool
  `write`, klien membalas frame `approval_request` dengan `once` → tool berjalan, berkas hasil berisi
  data mock, `result.stop_reason=stop`, lalu `shutdown` menutup server dengan exit 0.
- Test **E2E tanpa gateway nyata** (`crates/tui-app/tests/e2e_mock_gateway.rs`): binary `maicli` dijalankan
  mode teks melawan server HTTP tiruan (SSE + `/models`) dengan kunci palsu dan `MAICLI_DATA_DIR` terisolasi;
  memverifikasi stdout jawaban, exit 0, dan transkrip `--event-log` berisi `AgentStart`/`AgentEnd`.
- Perekam transkrip `--event-log <FILE>` (RENCANA C-12): setiap `AgentEvent` yang mengalir ke UI juga
  ditulis sebagai JSONL (direktori dibuat otomatis, kegagalan menulis tidak menggagalkan sesi), di mode
  TUI/teks/RPC; dipasangkan dengan runner `maicli --parity-expected/--parity-actual`.
- Runner parity (RENCANA C-12): `maicli --parity-expected <ACUAN.jsonl> --parity-actual <HASIL.jsonl>` membaca
  dua transkrip event agen, menormalisasi keduanya, mencetak laporan diff (skenario, indeks event pertama
  berbeda, event hilang/kelebihan), dan exit 1 bila berbeda. `AgentEvent`/`ProviderEvent` kini
  serializable agar transkrip bisa disimpan dan dibandingkan.
- Bukti langsung **kill-tree `bash`**: test `timeout_membunuh_anak_proses` menjalankan anak proses yang  menulis marker setelah 2,5 detik dengan timeout tool 1 detik; marker tidak pernah terbentuk, jadi
  anak proses benar-benar ikut dibunuh (`taskkill /T`), bukan hanya shell induk.
- `docs/tools-v1.md`: skema JSON + contoh panggilan untuk kedelapan tool v1 beserta tier,
  concurrency, batas output, kasus gagal, dan **shell yang dipakai `bash`** (`cmd.exe /C` di Windows).
- Test batas keamanan tool: path traversal gaya Windows (`..\`), path absolut di luar direktori kerja,
  dan semantik `edit` yang mencocokkan teks **persis** termasuk akhir baris (CRLF).
- Mode approval **`yolo`** kini selalu menampilkan banner peringatan saat start (TUI sebagai baris pertama,
  mode teks/`--prompt` di stdout, RPC sebagai event `notice`), menyebut bahwa tool berjalan tanpa
  persetujuan dan cara mengubahnya; default tetap `AlwaysAsk` dan tidak pernah ditulis otomatis
  (RENCANA §10 butir 11, `tui_app::banner`).
- File **config, grant approval, dan sesi** kini dibatasi ACL-nya ke pengguna saat ini saja
  (`icacls /inheritance:r /grant:r <user>:F`), termasuk berkas rotasi sesi; diterapkan saat menulis
  dan diverifikasi di test lewat `Get-Acl` (1 ACE, warisan mati, pengguna saat ini) — RENCANA §10 butir 9
  dibuktikan, bukan diasumsikan.
- Cache daftar model di **berkas** `models-cache.json` (RENCANA C-02): TTL 24 jam, tulisan atomik
  (tmp + rename), dipakai saat start agar `/model` punya daftar tanpa memanggil jaringan; file rusak,
  kosong, atau kedaluwarsa diabaikan dan diambil ulang dari gateway. Dependensi `serde` ditambahkan
  ke `tui-app` untuk (de)serialisasi cache.
- Test integrasi **timeout approval** (C-10): approval yang dibiarkan tanpa jawaban ditolak otomatis
  setelah `limits.approval_timeout_secs`; tool tidak pernah mulai (`ToolExecutionStart` tidak ada),
  penolakan tercatat di timeline, dan test memastikan penolakan benar-benar menunggu batas waktu.
- Perintah garis miring kini tampil dengan badge **`[perintah]`** di timeline TUI (prompt biasa tetap
  `>`), termasuk perintah yang dikirim dari pemilih sesi — jelas mana yang dijalankan aplikasi dan mana
  yang dikirim ke model (RENCANA C-08).
- Status turn eksplisit (RENCANA §7.2): `tui_core::status::TurnStatus`
  (`Idle`/`Streaming`/`ExecutingTools`/`AwaitingApproval`/`Aborted`/`Error`) dengan tabel transisi yang
  diuji, dan `AgentSession::status()` yang diperbarui di titik transisi (dijaga `debug_assert`).
- Gate **`beforeModelCall`** (RENCANA C-03): `AgentSession::set_before_model_call(callback)` dipanggil
  sebelum setiap permintaan model; callback mengembalikan `Some(alasan)` untuk menghentikan turn
  (`StopReason::Aborted`) **tanpa** memanggil provider. Diuji dua arah (menghentikan dan meloloskan).
- **Grant expiry** (deviasi kecil dari C-05, dicatat): `approval.grant_ttl_hours` (0 = tidak kedaluwarsa);
  grant `always` menyimpan `expires_at_ms`, grant basi tidak di-seed dan dipangkas saat start, file grant
  lama tanpa field itu tetap kompatibel.
- Smoke **live gateway** `#[ignore]` (`crates/tui-app/tests/live_gateway.rs`): discovery `/v1/models` lebih dulu
  (model tidak ditebak), lalu satu turn streaming; hanya jalan dengan `--ignored` dan kunci nyata.
- Hook **syncContextBeforeModelCall** (C-03): AgentSession::set_context_sync menerima pengganti konteks
  sebelum compaction/panggilan model (hasil kosong diabaikan).
- Metadata **`AgentTool`** (C-04): `label`, `loadMode`, `render` dipisah dari definisi wire; TUI memakai label
  dan petunjuk render (❯ perintah, ± diff, → biasa) di timeline, termasuk tool tak dikenal.
- Harness golden baseline bertambah menjadi **6 skenario** (teks murni, tool diizinkan, tool ditolak,
  follow-up 2 turn, `length`, dua tool sejajar) — semuanya baseline implementasi sendiri.
- Harness golden baseline (C-12): fixture `tests/golden/*.txt` untuk skenario teks murni, tool
  diizinkan, dan tool ditolak, dibandingkan dengan transkrip ternormalisasi melalui `parity::compare`;
  test mutasi membuktikan harness menjadi MERAH saat satu event diubah (laporan menyebut indeks).
  Fixture ini baseline implementasi sendiri, **bukan** transcript omp.
- Komponen pembanding parity (`tui_wire::parity`, bagian C-12): normalisasi event (membuang id acak,
  timestamp, dan event UI murni) serta `ParityDiff` yang menyebut skenario, indeks event pertama yang
  berbeda, dan daftar event hilang/kelebihan — laporan diff, bukan sekadar “gagal”.
- Test integrasi worker: prompt yang dikirim **saat turn sibuk** ditolak dengan notice “masih sibuk”
  (server SSE lambat tiruan), membuktikan C-03(4) di tingkat produk, bukan hanya core.
- Hapus sesi (`dropSession`, RENCANA C-06): `maicli --drop-session <id>` menghapus berkas sesi dan
  artefak lock-nya; menolak sesi yang sedang dipakai proses lain (`E_SESSION_LOCKED`) dan melaporkan
  sesi tak ada (`E_SESSION_NOT_FOUND`). Operasi destruktif hanya lewat flag eksplisit, tidak otomatis.
- Pencarian judul sesi (RENCANA C-06): `tui_session::search` (substring, tanpa peduli huruf besar),
  `/sessions <judul>` di aplikasi, dan `maicli --list-sessions [judul]` di CLI.
- Approval di mode teks kini menampilkan **argumen verbatim** (tidak hanya subjek) dan batas waktu
  keputusan, sehingga pengguna tidak menyetujui tool tanpa melihat apa yang akan dijalankan.
- `docs/uat-v1.md`: paket UAT U-01..U-12 (langkah, hasil yang diharapkan, kolom hasil untuk
  iqbal) sesuai RENCANA §11.3, termasuk prosedur log untuk bukti performa.
- Instrumentasi pengukuran C-09: dengan MAICLI_LOG=debug, TUI mencatat frame_ms per frame dan
  delta_ms (dari delta streaming diterima sampai digambar) ke berkas log; `logging::init_with`
  ditambahkan agar jalur ini bisa diuji (log benar-benar tertulis ke berkas).
- Test regresi C-05: seluruh 8 tool v1 dicoba dengan approver Deny — setiap panggilan berhenti di
  gerbang (placeholder error, tanpa efek samping), membuktikan tidak ada jalur eksekusi yang melewati gate.
- Berkas sesi punya **batas ukuran 16 MiB + rotasi**: saat batas tercapai, sesi lanjut di berkas baru
  dengan `parent_session` menunjuk berkas sebelumnya (berkas lama utuh), sehingga log tidak tumbuh
  tanpa batas (RENCANA C-06).
- Override config **runtime** lewat `maicli --set bagian.kunci=nilai` (boleh diulang): prioritas tertinggi,
  tidak menulis file, nilai divalidasi dulu, dan ditandai `<runtime --set>` di `config show`.
- `docs/config-v1.md`: skema config v1 (semua bagian/kunci/default), urutan pencarian kunci API,
  dan catatan bahwa perubahan config **butuh restart** (tanpa file-watching) sesuai C-11.
- Test regresi: `finish_reason: content_filter` dipetakan ke `stopReason` error dengan pesan yang menyebut
  `content_filter` (bukan sukses diam-diam), sesuai kriteria C-02.
- Test integrasi membuktikan turn **benar-benar tertunda** saat approval menunggu: tool belum
  dieksekusi sebelum keputusan, lalu berjalan setelah izin; blok approval diuji menempel tepat
  setelah baris histori terakhir.
- `F2` melompat ke blok persetujuan yang menunggu saat histori sudah ter-scroll jauh; hint
  tombol pada status ikut menyebutkannya.
- Dialog approval di TUI kini **inline di timeline** (overlay modal dihapus): detail **verbatim**
  (subjek + argumen utuh), pilihan `izinkan sekali`/`tolak` lewat **Tab** lalu **Enter** (pintas
  `y/2/3/n` tetap ada), dan **countdown** dari `limits.approval_timeout_secs`. Enter tanpa pilihan
  tidak menyetujui, dan keputusan/kedaluwarsa berlabel teks pada blok yang sama.
- Tool dijalankan sesuai konkurensinya: beberapa tool `shared` dalam satu respons dijalankan
  **bersamaan** (spawn), sedangkan tool `exclusive` selalu sendirian; `ToolDefinition` kini membawa
  field `concurrency` (default `shared`). Bukti: dua test dengan jam tokio dijeda.
- `/forget-approvals` menghapus semua grant yang diingat dari memori **dan** mengosongkan
  file `grants.json` (dulu grant hanya bisa bertambah); approver kini mengekspos `forget_grants`.
- Tool `todo` diturunkan ke tier `read` sesuai RENCANA C-04 (mengubah daftar kerja bukan
  menyentuh berkas), sehingga tidak memicu approval tulis.
- Timeline tool di TUI bisa **dibuka/diringkas dengan `Ctrl+E`**: detail disimpan per baris tool
  (mulai/selesai + cuplikan), dan perhitungan gulir ikut menghitung baris detail yang tampil.
- TUI punya **palet command Ctrl+K**: filter berdasarkan nama/usage/deskripsi, `↑`/`↓` memilih,
  Enter menjalankan command tanpa argumen atau mengisi composer untuk command yang butuh argumen,
  `Esc` menutup tanpa efek.
- Composer TUI kini **multi-baris**: `Shift+Enter`/`Alt+Enter` menambah baris baru (Enter tetap
  mengirim), tinggi kotak mengikuti isi sampai 12 baris dan menggulir internal agar baris terakhir
  terlihat; petunjuk tombol ikut diperbarui.
- `/model` memakai hasil discovery gateway: tanpa arg menampilkan model aktif **dan** daftar model
  yang tersedia (atau menyebut bahwa daftar belum diambil), argumen tak dikenal ditolak dengan daftar
  pilihan alih-alih dipakai menebak; daftar diambil sekali saat start dengan batas 3 detik dan tidak
  menggagalkan aplikasi bila gateway belum bisa dihubungi.
- Command tak dikenal kini menyebut semua command yang tersedia, bukan hanya menyuruh `/help`.
- `/new` memulai sesi baru (konteks dan usage direset) dan `/fork` menduplikasi sesi berjalan lalu
  melanjutkan salinannya — keduanya tersedia di mode teks dan TUI; di TUI histori layar dikosongkan
  lewat event `SessionSwitched` yang sama dengan `/switch`.
- C-07 `fitOutputTokens`: `max_tokens` per panggilan kini menyesuaikan sisa window konteks dengan
  lantai 1.024 token; bila `max_tokens` tidak dikonfigurasi, nilainya tetap kosong (tidak mengarang).
- `/context` menampilkan pemakaian konteks terhadap anggaran (persen, reserve, target pemulihan) dan
  `/todo` menampilkan daftar todo sesi dari tool `todo` (daftar kini dibagikan lewat `ToolRegistry`).
- Metode compaction **`shake`** sesuai RENCANA C-07: saat konteks melewati ambang, isi hasil tool
  lama dipangkas lebih dulu tanpa memanggil model; bila hasilnya kembali di bawah target pemulihan,
  ringkasan model tidak diperlukan. Minimal 8 hasil tool terbaru selalu dipertahankan, dan bila
  `shake` tidak cukup konteks diringkas dengan metode `soft` seperti sebelumnya.
- Composer TUI punya **riwayat prompt**: saat kosong, `↑` memanggil prompt sebelumnya dan `↓` yang
  lebih baru (melewati entri terbaru mengosongkan composer); saat berisi teks, panah tetap menggulir
  histori seperti sebelumnya.
- Dialog persetujuan tool di TUI sekarang punya overlay sendiri: nama tool + tier, subjek, dan
  **cuplikan argumen** (dipotong agar tidak memenuhi layar), sehingga yang diizinkan terlihat jelas.
- `/compact` melaporkan hasilnya: ukuran konteks sebelum dan sesudah (perkiraan token) serta jumlah
  pesan yang digantikan ringkasan; `compact_now` mengembalikan `CompactionReport` alih-alih `bool`.
- TUI menerima **tempel teks** (bracketed paste): baris baru diubah menjadi spasi, isi dibatasi
  4000 karakter seperti ketikan, dan `Ctrl+U` membersihkan composer. Fitur `bracketed-paste`
  crossterm diaktifkan beserta pasangan disable-nya di `TermGuard`.
- Tabel markdown di histori TUI dirender dengan lebar kolom seragam dan header tebal beraksen;
  hanya baris dengan pemisah `|---|` yang dianggap tabel, sehingga teks berpipa biasa tidak berubah.
- Blok kode berpagar (```) di histori TUI tampil redup dan **tidak** diproses sebagai penekanan,
  termasuk baris yang sedang streaming di dalamnya, sehingga kode berisi `**` tidak berubah gaya.
- Histori TUI memakai **penekanan ringan**: heading `# …` tebal berwarna aksen, `**tebal**` dan
  `` `kode` `` diberi gaya; penanda yang tidak ditutup dibiarkan sebagai teks biasa agar keluaran
  setengah jadi tidak hilang.
- Status bar TUI menampilkan **pemakaian konteks** (`konteks N%`) dari perkiraan token sesi dan
  `limits.context_window_tokens`; window 0 tidak ditampilkan (tidak ada pembagian nol). Event baru
  `ContextUsage` membawa angka itu ke UI.
- TUI punya **pemilih sesi**: `/sessions` membuka overlay berisi sesi terbaru (judul + jumlah
  pesan), `↑`/`↓`/`j`/`k` memilih, `Enter` langsung pindah sesi, `Esc` menutup tanpa efek; daftar
  kosong tetap menjadi pesan biasa. Event baru `SessionsListed` membawa daftar itu ke UI.
- `maicli --fork [ID]` membuat salinan sesi (judul "… (fork)", `parent_session` terisi) lalu melanjutkan
  salinannya; `--resume` dan `--fork` tidak boleh dipakai bersamaan.
- `/usage` merinci pemakaian **per model** bila sesi memakai lebih dari satu model (termasuk setelah
  `/model` berganti), dihitung dari akumulasi per model di `AgentSession`.
- Kunci API bisa dibaca dari keyring OS: env var tetap prioritas, keyring jadi cadangan, dan
  `maicli config key-import` menyimpan nilai dari env var ke Windows Credential Manager tanpa pernah
  mencetaknya (pesan galat pun tidak memuat nilai kunci).
- Entry `ModelUsage` kini ditulis setiap turn (selisih terhadap yang sudah tercatat) dan dijumlahkan
  saat resume, sehingga `/usage` melanjutkan hitungan sesi alih-alih mulai dari nol; sebelum ini
  entry tersebut hanya ada di skema.
- Entry `ModelChange` kini ditulis saat `/model` dan dibaca saat resume; model sesi bertahan antar
  proses dengan precedensi `--model` > model terakhir di sesi > `models.default` (sebelumnya entry
  ini hanya ada di skema dan tidak pernah dipakai).
- Entry `TitleChange` kini benar-benar dipakai: judul terakhir menang atas judul header, sehingga
  `/title` dan `--list-sessions` menampilkan nama yang benar (sebelumnya entry ditulis tapi diabaikan).
- `/title <teks>` mengganti nama sesi di mode teks dan TUI (ditulis sebagai entry `TitleChange`).
- TUI bisa pindah sesi dengan `/switch [id]`: worker membangun ulang sesi, membuka store lewat
  `spawn_blocking`, dan event baru `SessionSwitched` mengosongkan histori layar supaya pesan sesi
  lama tidak tercampur dengan sesi baru.
- `/switch [id]` di mode teks: pindah sesi tanpa keluar aplikasi (`latest`, id persis, atau awalan
  id yang unik; awalan ambigu ditolak dengan daftar kandidat), membuka store baru lewat
  `spawn_blocking`. Di TUI perintah ini menjawab terus terang bahwa pindah sesi belum didukung.
- TUI mendukung roda mouse: satu notch menggulir 3 baris, capture diaktifkan bersama raw mode dan
  dimatikan lagi saat keluar sehingga terminal tetap bersih.
- RPC membatasi satu frame pada 1 MiB: frame lebih besar dibuang tanpa disimpan ke memori, server
  membalas `E_PROTOCOL` dan melanjutkan; galat JSON hanya memuat pratinjau 200 karakter.
- `maicli config set <key> <value>` menulis satu nilai ke config global secara atomik, mempertahankan
  key lain, dan menolak key asing atau nilai tidak valid sebelum file disentuh.
- Slash `/sessions` menampilkan sesi tersimpan (terbaru lebih dulu) di mode teks dan TUI, memakai
  daftar yang sama dengan `maicli --list-sessions`.
- Mode teks sekarang bisa menjawab tool `ask`: stdin dibaca satu thread pembaca dan dibagi antara
  REPL dan pertanyaan; baris yang diketik saat turn berjalan disimpan untuk prompt berikutnya,
  bukan dibuang.
- Gulir TUI menghitung baris **visual** hasil word-wrap, jadi Home/End/PgUp/PgDn berhenti tepat di
  ujung isi meski ada paragraf panjang; lebar tampilan ikut diambil dari area render.
- TUI: warna dari config (`[ui] accent = "#rrggbb"`, `dim = "#rrggbb"`) dipakai untuk status,
  bingkai, dan hint; nilai bukan hex ditolak dengan pesan yang menyebut field-nya.
- Grant approval persisten: keputusan `Always` disimpan di `grants.json` (tulis atomik, direktori
  config pengguna) dan dimuat saat start, sehingga izin yang sudah diberikan tidak ditanyakan lagi.
- Kebijakan approval dari config kini benar-benar berlaku di TUI, teks, dan RPC (sebelumnya UI
  approver selalu bertanya dan mengabaikan `approval.mode`), termasuk `yolo` dan restricted folder.
- Audit approval: setiap keputusan gate ditulis ke transkrip sebagai entry `approval_decision`
  (tool, tier, subjek, keputusan) di mode teks, TUI, dan RPC; entry lama tanpa `tier` tetap terbaca.
- Semua approver memakai `limits.approval_timeout_secs` (default 600 detik): permintaan yang tidak
  dijawab otomatis ditolak, TUI mencatat “kedaluwarsa”, dan indikator itu tidak lagi mengklaim
  keputusan yang tidak sampai ke tool.
- RPC: approval interaktif — frame `approval_request` dengan `id`/`tool`/`tier`/`subject`, dan
  permintaan `approval` (`once`/`session`/`always`/`deny`); kebijakan config tetap berlaku, hanya
  permintaan yang butuh manusia yang dikirim ke klien.
- RPC: frame `question` dan permintaan `answer`, sehingga klien otomatis bisa menjawab tool `ask`
  (teks kosong berarti dilewati).
- Tool `ask` benar-benar bertanya di TUI: pertanyaan dari model muncul di composer, Enter menjawab,
  Esc melewati, batas tunggu 600 detik, dan jawaban dicatat di histori. Mode tanpa UI menolak
  dengan pesan jelas.
- Tool `read` menerima `offset`/`limit` dan menolak berkas biner; `grep` menerima `max_results`
  dengan penanda hasil terpotong; `ask` kini jujur menolak (belum ada UI pertanyaan) alih-alih
  mengaku menunggu jawaban.
- Pemakaian token: `Usage::accumulate` di wire, akumulasi kumulatif di sesi, `/usage` di mode
  teks dan TUI, serta permintaan `usage` di protokol RPC.
- Ringkasan compaction disimpan sebagai entry `compaction` di transkrip, dan resume memakainya
  (pesan sebelum ringkasan digantikan), sehingga sesi panjang tidak menghitung ulang ringkasan.
- Pause gate: `/pause` dan `/resume`; turn berikutnya ditolak `E_AGENT_PAUSED` dan turn berjalan
  berhenti di batas iterasi sebelum panggilan model berikutnya.
- Lock sesi: file `.lock` mencegah dua proses menulis sesi yang sama (`E_SESSION_LOCKED`),
  lock basi lebih dari satu jam diambil alih otomatis.
- Daftar dan lanjut sesi: `maicli --list-sessions`, `maicli --resume [ID]` (tanpa ID = sesi terakhir);
  mode teks, TUI, dan RPC memuat pesan lama sebagai konteks awal, dan file rusak dilewati
  saat mendaftar.
- Mode `--mode rpc`: server JSONL (`maicli-rpc/1`) dengan `ping`, `prompt`, `abort`, `model`,
  `shutdown`, event terproyeksi, dan approval berbasis config tanpa UI (`docs/rpc-v1.md`).
- Pembatalan kooperatif: `CancelToken` diteruskan ke tool; `bash` menghentikan pohon proses
  segera saat turn dibatalkan, dan turn yang sedang menunggu stream model berhenti dengan
  `stop_reason=aborted`.
- TUI: histori bisa digulir (PgUp/PgDn/Home/End/↑↓), overlay bantuan F1/`?`, status
  `model · keadaan · scroll`, dan `Esc` membatalkan turn yang berjalan.
- Workspace Rust `TUI/` dengan sembilan crate: `tui-wire`, `tui-context`, `tui-provider`,
  `tui-session`, `tui-approval`, `tui-config`, `tui-tools`, `tui-core`, `tui-app`.
- Binary `maicli` dengan mode `tui`, mode `text`, `--prompt` sekali jalan, dan `config show|path`.
- Klien gateway OpenAI-compatible: pembaca SSE tahan chunk terpotong, perakitan `tool_call`
  terfragmentasi, pemetaan `finish_reason`, retry 3 percobaan dengan jitter, taksonomi error stabil.
- Resolusi id model berlapis (persis → case-insensitive → tanpa prefiks provider → substring unik)
  dan cache discovery dengan TTL.
- Turn loop dua tingkat dengan antrean steering/follow-up dan aturan `stopReason`
  (`stop` ≡ `toolUse`, `length` tidak menjalankan tool).
- Crate `tui-context`: estimasi token, anggaran window (reserve 16.384 / keep 20.000 / band 0,8),
  pruning hasil tool lama, dan rencana compaction yang menjaga pasangan tool_use/tool_result.
- Compaction otomatis sebelum panggilan model (maks 2 per turn) plus `/compact` manual.
- Delapan tool v1: `read`, `write`, `edit`, `glob`, `grep`, `bash`, `todo`, `ask`.
- `bash` menghentikan seluruh pohon proses saat timeout (`taskkill /T` sebelum kill induk).
- Slash command v1: `/help`, `/model`, `/compact`, `/tools`, `/session`, `/quit` (mode teks dan TUI).
- Gerbang approval: 4 mode × 3 tier, kebijakan per-tool, folder terlarang, grant sesi.
- Store sesi JSONL v1 dengan tulis atomik, toleransi baris terakhir terpotong, dan fork.
- Config berlapis (default → global → proyek) dengan provenance dan redaksi kunci API.
- `check.ps1` (fmt, clippy, test, deny, aturan dependensi) dan `deny.toml`.

### Fixed
- lapisan: katalog tidak lagi bergantung tui-wire (aturan check.sh); test
  Windows-only diberi guard cfg; test bash/extension portabel (sleep/sh -c).
- Skema entry sesi kini **ketat**: field asing ditolak (`deny_unknown_fields`); hanya baris terakhir yang
  benar-benar terpotong (EOF) yang diabaikan sebagai sisa crash, sedangkan kesalahan data tetap
  `E_SESSION_CORRUPT`.
- `grants.json` dengan versi skema lebih baru kini ditolak dengan pesan jelas, bukan dibaca dengan
  asumsi versi lama (konsisten dengan penjagaan versi sesi).
- **Deadline turn** kini juga menghentikan tool yang sedang berjalan: penjaga deadline membatalkan turn,
  dan eksekusi setiap tool di-`select!` dengan cancel token (sebelumnya deadline hanya dicek sebelum
  panggilan model).
- Turn yang dibatalkan **saat menunggu approval** kini berhenti cepat: keputusan approval di-`select!`
  dengan cancel token (batal → deny + notice), bukan menggantung sampai batas waktu approval.
- Folder terlarang kini juga menutup tool `exec`: subjek `bash` berupa command string, sehingga path terlarang
  di dalam perintah dicocokkan sebagai substring (tanpa peduli huruf besar), bukan hanya sebagai path. Diuji
  untuk read/grep/write/edit/bash dan terhadap grant `always`.
- Turn yang dibatalkan kini mengirim **`message_end` sintetis** (`stop_reason=aborted`) agar pasangan event
  `message_start`/`message_end` tidak putus saat stream dihentikan — ditemukan lewat golden baru `abort`.
- Naskah bantuan/status TUI dikoreksi: F1 menyebut semua tombol yang dipakai (Ctrl+K, Ctrl+E, F2, Tab/Enter,
  ↑/↓) dan semantik Ctrl+C yang sebenarnya; hint saat sibuk tidak lagi menulis “Ctrl+C keluar”.
- Dokumen RPC dikoreksi: saat turn berjalan dilayani juga frame `approval` dan `answer`, bukan
  hanya abort/ping/shutdown (sebelumnya dokumen menyatakan sebaliknya).
- `bash` yang timeout/dibatalkan kini mengembalikan **output parsial** yang sudah dihasilkan (ditandai
  `[output parsial]` dan `truncated=true`), bukan hanya pesan timeout — sesuai C-04 kriteria 2. Pipe
  ditutup setelah proses dibunuh dan pembacaan dibatasi 2 detik agar turn tidak menggantung.
- `maicli config set` dengan kunci asing kini memakai pesan **“kunci config tidak dikenal: unknown field …”**
  alih-alih awalan “TOML parse error” yang menyesatkan (file tetap tidak ditulis, exit 2).
- Error provider kini tetap memasangkan event `message_start` dengan `message_end`: dibuat pesan
  sintetis ber-`stop_reason: error` (berisi alasan) sehingga UI tidak menggantung menunggu akhir pesan
  yang tidak pernah datang. Skenario golden `error-provider` mengunci urutan ini.
- Tool yang **tidak dikenal** kini mengirim event `tool_execution_end` (error) ke UI, bukan hanya
  menulis `tool_result` yang tidak terlihat; timeline tidak lagi tampak diam saat model memanggil
  tool asing (C-04 kriteria 3).
- Pemicu compaction kini memakai **max(estimasi lokal, usage provider)** sesuai RENCANA C-07; sebelumnya
  hanya estimasi lokal, sehingga konteks yang dilaporkan gateway lebih besar bisa terlambat diringkas.
  Ditambah test hysteresis: setelah compaction, turn berikutnya tidak langsung meringkas lagi.
- Dokumen RPC dikoreksi: saat turn berjalan dilayani juga frame `approval` dan `answer`, bukan
  hanya abort/ping/shutdown (sebelumnya dokumen menyatakan sebaliknya).
- SSE: `\r` di ujung satu chunk dan `\n` di chunk berikutnya tidak lagi menjadi dua newline yang
  memalsukan batas event; baris `data:` kosong tidak diteruskan sebagai payload JSON kosong.
- Penulisan `grants.json` dipindah ke `spawn_blocking` sesuai aturan async proyek (fs sinkron tidak
  dijalankan di dalam future); kegagalan menulis tidak lagi berisiko menjatuhkan turn dan diuji.
- Perhitungan lipat baris TUI memakai `unicode-width`, jadi teks CJK dan emoji dihitung 2 kolom;
  gulir tidak lagi berhenti terlalu jauh dari ujung isi pada teks non-Latin.
- Kegagalan membuka/membuat sesi tidak lagi hilang diam-diam: mode teks, TUI, dan RPC memberi tahu
  pengguna ("sesi tidak tersimpan: …") alih-alih berjalan tanpa persistensi tanpa penjelasan.
- Literal non-ASCII di test diganti escape Unicode (`\u{…}`) agar berkas tetap ASCII; penulisan
  ulang lewat PowerShell sebelumnya merusak karakter CJK/emoji menjadi mojibake.
