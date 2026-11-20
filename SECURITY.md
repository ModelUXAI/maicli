# Kebijakan Keamanan

## Melaporkan kerentanan

**Jangan buka issue publik** untuk laporan keamanan. Gunakan:

- GitHub **Private vulnerability reporting** (tab Security → Report a vulnerability), atau
- email: `iqbalnaveliano@gmail.com` (subject: `[SECURITY] mx`).

Sertakan: versi/commit, platform, langkah reproduksi, dampak. Balasan target: 7 hari.

## Ruang lingkup keamanan proyek ini (yang dijaga desain)

- **Kredensial**: API key hanya dari env var / OS keyring; tidak pernah ditulis ke
  file config/sesi/log, tidak pernah dicetak (termasuk `Debug` — tipe pembawa kunci
  meredaksi); **tidak diwariskan** ke proses anak (tool bash/extension/MCP membuang
  env bernama kredensial / membersihkan env).
- **Approval gate**: semua eksekusi tool melewati gate approval — tidak ada jalur
  pintas untuk "tool aman"; grant persisten terikat subjek tool + kedaluwarsa;
  folder terlarang menutup semua tier.
- **Path traversal**: `read`/`write`/`glob`/`edit` menolak `..`, junction/symlink
  keluar cwd, path absolut luar proyek (diuji).
- **File sensitif** (config global, grant, sesi, token serve/relay): izin
  pengguna-saja (Windows ACL; POSIX mode 600 — diuji di kedua platform).
- **Server lokal** (`serve`/`relay`): bind loopback saja; **tidak pernah berjalan
  tanpa token** (token acak otomatis ditulis dengan izin ketat); guest relay hanya
  view; perintah guest tetap lewat approval host.
- **Extension eksternal**: manifest parsing ketat; program di luar folder ditolak;
  env proses dibersihkan; timeout + kill pohon; tier Exec.

## Di luar scope

- Eksploitasi yang membutuhkan pengguna menjalankan perintah arbitrer pada
  kesadarannya sendiri (approval gate memang memintakan izin);
- Kelemahan dependensi — dilaporkan ke upstream; kita memantau via `cargo deny`.

## Versi yang didukung

Hanya `main` + release terbaru.
