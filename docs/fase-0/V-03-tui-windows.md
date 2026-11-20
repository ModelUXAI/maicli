# V-03 — spike TUI di Windows

Status: PARTIAL (terbangun & berjalan; verifikasi visual oleh iqbal belum).

## Bukti

```powershell
> rustc --version
rustc 1.99.1 (58cf8f9c3 2026-10-13)
> cargo tree -p tui-app -e normal | Select-String 'ratatui|crossterm'
ratatui v0.30.2
crossterm v0.29.0
> .\target\debug\mx.exe --version
mx 0.1.0
```

OS: Microsoft Windows 10 IoT Enterprise LTSC 10.0.19044 (x64). Build dengan MSVC toolchain
default rustup; tidak ada Docker/WSL2 (sesuai `AGENTS.md` §3).

## Yang sudah terbukti

- `ratatui` + `crossterm` **kompilasi** di Windows tanpa dependensi sistem tambahan.
- TUI berjalan: alternate screen, raw mode, layout 4 baris (status / viewport / composer / hint),
  streaming token masuk ke viewport lewat `AgentEvent`, dan prompt approval inline (y/1/2/3/n).
- Terminal dipulihkan saat keluar lewat `TermGuard` (Drop: `disable_raw_mode` + `LeaveAlternateScreen`).

## Yang belum terbukti (jangan diklaim)

- Tampilan nyata di Windows Terminal/conhost (perlu mata manusia): TUI belum pernah dilihat iqbal.
- ~~Frame emas belum ada harness-nya~~ — **sejak 2026-11-18**: `tui_mode::tests::render_frame` (ratatui `TestBackend`) + `frame_emas_tiga_ukuran` (80×24 / 120×40 / 200×50: konten terlihat, hint/status/composer hadir) + `frame_resize_tidak_kehilangan_konten` (50 baris, resize 80×24 → 200×50 tanpa kehilangan `baris-49`). Catatan jujur: ini **harness render**, bukan penggant verifikasi visual mata manusia (tetap butuh iqbal).
- Perilaku resize, scroll histori > 1 layar, dan latensi render < 100 ms — belum diukur.
- Kill-tree proses anak saat timeout (masalah Windows, lihat `WORK.md` Kandidat).

## Langkah berikut (C-09)

1. Tangkapan layar dari iqbal sebagai bukti visual pertama.
2. Harness frame emas: render `TestBackend` untuk 1 skenario tetap, bandingkan buffer.
3. Ukur latensi render dari stempel waktu event → frame.
