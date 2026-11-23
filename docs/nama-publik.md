# Rekomendasi nama publik

## Cek ketersediaan (2026-11-23)

| Nama | GitHub `modeluxai/<nama>` | npm | Catatan |
|---|---|---|---|
| `maicli` | tersedia (repo belum ada) | tersedia | nama teknis internal; arti tidak jelas untuk publik |
| `modelux` | tersedia | **terpakai** (200) | pendek, tapi npm sudah diambil |
| `modelux-cli` | tersedia | tersedia | jelas, tapi agak panjang |
| `modelux-code` | tersedia | tersedia | meniru "claude code/codex" |
| `modeluxai` | tersedia | tersedia | sama dengan org; bagus untuk repo utama |
| `modeluxai-cli` | tersedia | tersedia | paling eksplisit |
| `mxcode` | tersedia | **terpakai** | singkat, npm bentrok |
| `maicli` | tersedia | tersedia | singkat, belum dipakai |

## Rekomendasi

1. **Repo GitHub: `ModelUXAI/maicli`** — singkat, mudah diingat, langsung
   menyiratkan produk ModeluXAI. npm `modelux` sudah dipakai, jadi paket npm
   memakai nama lain (`@modeluxai/cli` atau `modeluxai-cli`).
2. **Alternatif paling aman: `ModelUXAI/modeluxai`** — sama dengan org,
   konflik minimal, npm `modeluxai` tersedia. Kurang "nama produk" tapi jelas.
3. **Jika ingin gaya "claude code / codex": `ModelUXAI/maicli-code`** —
   npm tersedia, langsung terdengar seperti CLI coding agent.
4. **Nama CLI/biner tetap `maicli`** agar semua dokumentasi, sesi, dan config
   yang ada tidak berubah; hanya nama repo/paket yang berubah.

## Rekomendasi akhir

Repo publik: **`ModelUXAI/maicli`**
Nama produk di README: **ModeluX — CLI agentic ModeluXAI** (`maicli`)
Paket npm: **`@modeluxai/cli`** (scope org, tidak bentrok)
