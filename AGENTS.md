# AGENTS.md — TUI Agentic ModeluXAI (Rust, adopsi oh-my-pi)

Aturan proyek ini. MENGIKAT semua agent dan manusia yang bekerja di repo ini.
Pelanggaran = pekerjaan ditolak, tidak peduli kodenya "jalan". File ini hanya
diubah atas perintah eksplisit iqbal, dicatat di riwayat amandemen di bawah.

## §0 Otoritas

1. `RENCANA.md` — otoritas tertinggi (keputusan, scope, kartu, kriteria).
2. `AGENTS.md` (file ini) — aturan kerja.
3. Prompt/instruksi lisan — hanya berlaku bila tidak bertentangan dengan 1–2.
4. Source omp @ 7c49c11 — spesifikasi perilaku untuk bagian yang di-port; rujukan
   wajib path + baris. Bila source omp bertentangan dengan RENCANA.md, RENCANA.md
   menang dan konfliknya dicatat di WORK.md.

## §1 Scope

- Kerjakan HANYA kartu aktif di RENCANA.md §6, sesuai spesifikasinya. Di luar
  kartu aktif = dilarang, termasuk "perbaikan kecil" dan "sekalian".
- RENCANA.md §13 adalah daftar larangan eksplisit. Mengerjakan isinya tanpa
  perintah iqbal = pelanggaran, walau hasilnya bagus.
- Temuan di luar scope → WORK.md bagian "Kandidat". Jangan dieksekusi.
- Bila spesifikasi kartu ambigu → BLOCKED + tanya iqbal. Menebak = pelanggaran.

## §2 Workflow dan gate

- Fase 0 (V-01..V-06) SELESAI + DISETUJUI iqbal sebelum satu baris kode produk
  ditulis. Tidak ada pengecualian.
- Kartu DONE hanya bila: semua kriteria biner YA + `check.ps1` hijau DI WINDOWS
  + entri CHANGELOG + bukti di WORK.md. Satu syarat hilang = TIDAK DONE.
- WORK.md dipelihara terus: marker TODO/DOING/DONE/BLOCKED per kartu + bukti
  (perintah + ringkasan output) + pertanyaan terbuka. WORK.md yang basi = kartu
  tidak DONE.
- Laporan akhir tiap fase: DONE (dengan bukti), BLOCKED (penyebab + kebutuhan),
  skor QA 1–10 dengan alasan. Skor < 7 = nyatakan belum siap.
- Jangan bertanya "lanjut?" bila plan/backlog sudah jelas. Kerjakan kartu
  berikutnya sesuai urutan. Berhenti HANYA di gate (Fase 0, akhir fase) atau
  saat BLOCKED.

## §3 JANGAN (larangan keras)

- JANGAN memakai Docker atau WSL2 untuk build/test/run apa pun.
- JANGAN memakai API khusus-Unix, path hardcode (`/home`, `C:\Users\...`),
  atau pemisah path literal. Selalu `std::path` + `std::process`.
- JANGAN menyimpan/meminta/mencetak secret (API key, password, token) di repo,
  log, sesi, screenshot, atau chat. Key hanya dari env var / OS keyring.
- JANGAN menebak model ID. Verifikasi lewat `GET /v1/models` dulu. JANGAN
  menguji ID model tebakan ke endpoint live (pelajaran IP-ban 2026-10-19).
- JANGAN `unwrap`, `expect`, `panic!`, `unreachable!`, atau `todo!` di kode
  library (semua crate kecuali `tui-app`). Ditegakkan clippy
  (`unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` = deny).
  `tui-app` boleh `expect` HANYA di startup dengan pesan yang menyebut apa
  yang gagal + apa yang harus dilakukan pengguna.
- JANGAN `unsafe` tanpa komentar `// SAFETY:` yang membuktikan keamanannya +
  persetujuan iqbal yang dicatat di WORK.md.
- JANGAN print ke stdout/stderr di jalur TUI/agent (merusak render/protokol).
  Log hanya ke file via `tracing`. Satu-satunya pengecualian: mode non-TUI
  yang didokumentasikan (belum ada di v1).
- JANGAN menulis prompt/instruksi sebagai string di kode. Prompt hidup di
  file `.md` di `prompts/` dan di-embed saat build.
- JANGAN mengedit file generated. Bila ada file generated, generatornya yang
  diubah + hasilnya di-regenerate + dicatat.
- JANGAN menambah dependensi tanpa alasan satu baris di `DEPENDENCIES.md` +
  `cargo deny check` bersih. Dependensi yang menelepon jaringan sendiri =
  dilarang tanpa persetujuan iqbal.
- JANGAN membuat file markdown tak diminta. Dokumen yang diizinkan: yang
  diminta RENCANA.md. Draf, catatan, dan ringkasan pribadi dilarang di repo.
- JANGAN meninggalkan `TODO`/`FIXME`/`HACK` di kode kartu DONE. Hal tertunda
  dicatat di WORK.md, bukan di kode.
- JANGAN memformat ulang file yang tidak disentuh kartu aktif. `cargo fmt`
  dijalankan, commit format dipisah dari commit perilaku.
- JANGAN mengubah perintah check (`check.ps1`/`check.sh`) untuk mengakali
  kegagalan. JANGAN menandai test `#[ignore]` untuk menyembunyikan kegagalan
  (satu-satunya `#[ignore]` yang diizinkan: smoke test live gateway C-02).
- JANGAN mengubah file ini (AGENTS.md) atau RENCANA.md tanpa perintah eksplisit
  iqbal. Usulan perubahan → WORK.md, tunggu keputusan.

## §4 Rust

- Edition + MSRV: diputuskan di V-06, dikunci di `Cargo.toml` workspace, diubah
  hanya via amandemen plan.
- Error: crate library memakai `thiserror` (error bernama, taksonomi RENCANA.md
  §7.3). `anyhow` hanya di `tui-app`. Error untuk pengguna berbahasa Indonesia
  dan menyebut tindakan yang bisa dilakukan.
- Async: tokio. Kode blocking (fs sinkron, proses, komputasi berat) HANYA via
  `spawn_blocking`. Dilarang `.await` di dalam `Mutex` std; pakai `tokio::sync`
  atau restrukturisasi.
- Tipe wire gateway: parsing PERMISIF terhadap field tak dikenal (gateway boleh
  menambah field); field yang dipakai wajib divalidasi; bentuk tak dikenal =
  error bernama, bukan diam, bukan panic.
- Tipe milik sendiri (sesi, config): parsing KETAT (`deny_unknown_fields`
  di serde) agar kesalahan skema ketahuan, bukan diam-diam diabaikan.
- Tidak ada kode mati: `dead_code` = deny di semua crate. Tidak ada `allow`
  clippy tanpa komentar alasan + batas waktu peninjauan.
- Komentar: hanya MENGAPA, bukan APA. Item publik wajib doc comment (contoh
  pemakaian bila tidak sepele). Komentar yang mengulang kode = pelanggaran.
- Test: tiap modul punya test perilaku (bukan test "ada fungsi"). Test
  deterministik (waktu/acak di-inject). Test tidak memanggil jaringan nyata
  kecuali smoke `#[ignore]`. Setiap bug → test regresi dulu, baru perbaikan.

## §5 Keamanan

- Key: hanya env var / keyring, hanya di memori, tidak pernah ditulis ke file
  (termasuk file sesi, log, crash dump, dan `Debug` formatting — tipe yang
  membawa key wajib `Debug` manual yang meredaksi).
- Approval gate: SEMUA eksekusi tool melewati gate C-05, tanpa kecuali, tanpa
  jalur pintas untuk "tool aman". Jalur pintas = pelanggaran keamanan.
- `bash`: timeout + cap output selalu aktif; perintah yang gagal mengembalikan
  error bernama + output parsial yang ditandai. Dilarang mengeksekusi string
  shell yang dibangun dari input model tanpa melewati approval + logging.
- Path: traversal (`..`, symlink keluar cwd, path absolut di luar proyek tanpa
  approval) ditolak atau diminta approval eksplisit — perilaku didokumentasikan
  di C-04 dan diuji.
- Log: tidak pernah mencatat isi argumen tool yang mengandung pola kredensial.
  Bila ragu, redact. Log level default = info; debug hanya atas flag.
- File config/grant/sesi: izin paling ketat yang didukung Windows (ACL pengguna
  saja). Dibuktikan, bukan diasumsikan.
- `cargo deny check` bersih sebelum kartu DONE (lisensi + advisory + ban).

## §6 Commit dan dokumen

- Satu commit = satu perubahan logis. Pesan commit: kata kerja imperatif +
  objek + alasan bila tidak jelas. Contoh: `tambah parser SSE tahan chunk terpotong`.
- Setiap kartu menutup dengan entri CHANGELOG.md di bawah `[Unreleased]`
  (format Keep-a-Changelog: Added/Fixed/Changed/Removed).
- `DEPENDENCIES.md`: tiap dependensi langsung + alasan satu baris + kapan
  ditambahkan. Dependensi tanpa alasan = kartu tidak DONE.
- `docs/`: hanya bukti Fase 0, skema, dan keputusan desain yang diminta plan.
  Dokumen usang dihapus, bukan dibiarkan membusuk.

## §7 Definisi DONE (checklist kartu — semua harus YA)

1. Spesifikasi kartu terpenuhi seluruhnya?
2. Semua kriteria biner kartu = YA?
3. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
   `cargo deny check`, `check.ps1` — hijau DI WINDOWS?
4. Test baru mencakup perilaku baru + kasus gagal + regresi bug yang ditemukan?
5. CHANGELOG + WORK.md + DEPENDENCIES.md (bila ada dependensi baru) diperbarui?
6. Tidak ada secret, artefak, atau file tak diminta di git status?
7. Tidak ada pelanggaran §3 di diff kartu ini (diperiksa dengan membaca diff)?
8. Skor QA jujur ≥ 7/10 dengan alasan tertulis?

---

*Riwayat amandemen: (kosong). Amandemen hanya oleh iqbal, dengan tanggal + alasan.*
