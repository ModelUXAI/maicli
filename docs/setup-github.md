# Setup GitHub (release + versioning + CI/CD) — untuk iqbal

Sistem sudah disiapkan di repo; tinggal langkah sekali-jalan di bawah.

## Yang sudah ada di repo

| Berkas | Isi |
|---|---|
| `.github/workflows/ci.yml` | PR/push main → **gerbang sama** dengan check (fmt, clippy, test, deny) di **Linux + Windows**, macOS allow-failure; cache cargo; E2E mock gateway (tanpa kunci nyata) |
| `.github/workflows/release.yml` | tag `vX.Y.Z` → test ulang → build release → strip → arsip (.tar.gz linux/macos, .zip windows) → **GitHub Release** dengan catatan otomatis dari `CHANGELOG.md` |
| `LICENSE` | MIT |
| `CONTRIBUTING.md` | alur PR + aturan kode ringkas |
| `SECURITY.md` | pelaporan kerentanan privat + ruang lingkup keamanan yang dijaga desain |
| `.gitignore` | target/, data runtime, token/relay-key, artefak packaging |

## Versioning

- Satu sumber versi: `[workspace.package] version` di `Cargo.toml` root (sekarang `0.1.0`);
- Alur: ubah kode → entri `CHANGELOG.md` di bawah `[Unreleased]` → merge ke `main`
  → saat rilis: pindahkan entri ke `## [X.Y.Z] - tanggal` → commit `rilis: v0.1.0`
  → tag `v0.1.0` → push tag → release otomatis.

## Langkah sekali-jalan (kamu jalankan)

1. **Buat repo GitHub kosong** (tanpa README inisial): `modeluxai-tui`, Private/Public sesuai mau;
2. **Push awal** (dari folder TUI):
   ```
   git remote add origin git@github.com:<akunmu>/modeluxai-tui.git
   git push -u origin main
   git push origin v0.1.0        # rilis pertama (workflow release berjalan)
   ```
3. **Branch protection** (Settings → Branches → main):
   - Require pull request (tanpa direct push);
   - Required checks: `gate (ubuntu-latest)`, `gate (windows-latest)`;
   - Require branches up to date; 1 review bila ingin;
   - (Opsional tapi disarankan untuk opensource) Require linear history.
4. **Security** (Settings → Code security): aktifkan **Private vulnerability reporting**,
   Dependabot alerts, dan (opsional) Code scanning.

## Aturan merge yang diberlakukan CI

- PR → CI wajib hijau di **Linux dan Windows** (matriks) sebelum merge;
- `continue-on-error` hanya untuk **macOS** (belum ada mesin verifikasi manual) —
  angkat ke wajib setelah verifikasi macOS pertama;
- Tidak ada rilis dari kode merah: release.yml **menguji ulang** sebelum build;
- Semua kunci CI = mock (env `MODELUXAI_API_KEY=ci-kunci-mock-tidak-nyata` di CI) —
  **tidak ada secret di GitHub Actions** (live smoke test tetap butuh izin manual).

## Saya bisa bantu lanjut

- Setelah repo dibuat: minta saya **verifikasi workflow pertama** (baca hasil run via `gh run view` — butuh `gh auth login` olehmu);
- Kalau mau: buatkan **template issue/PR** + label (`feat`, `fix`, `security`).
