# V-02 — probe gateway live (DONE)

Status: **DONE** — dieksekusi 2026-11-18 dengan kunci nyata (nilai kunci **tidak**
dicatat di dokumen mana pun; hanya env var proses + keyring OS).

## Konfigurasi terverifikasi

| Item | Nilai |
|---|---|
| Endpoint | `https://api.modeluxai.com/v1` (bawaan `gateway.base_url`) |
| Kunci | env `MODELUXAI_API_KEY` + **Windows Credential Manager** (`mx config key-import`, service `mx-modeluxai`) |
| Model uji | `deepseek-v4.1-flash` |

## Bukti (semua perintah dijalankan di sesi ini)

1. **Discovery**: `GET /v1/models` → **25 model**; `deepseek-v4.1-flash` ada di
   daftar (juga varian `bansos/deepseek-v4.1-flash`, claude-opus/sonnet, dll.).
2. **Smoke test otomatis** (`cargo test -p tui-app --test live_gateway -- --ignored`):
   `smoke_live_discovery_dan_satu_turn ... ok` (3,61 dtk) — discovery dulu, lalu
   satu turn streaming.
3. **Turn teks nyata** (`mx --mode text --prompt …`): jawaban benar
   ("Soekarno"), `[selesai: 1 turn, Stop]`; event log 27 event
   (`AgentStart → TurnStart → MessageStart → …`).
4. **Tool call nyata** (approval `yolo` via `--set`): model memanggil tool `read`
   (`ToolExecutionStart/End` di event log), isi berkas kembali persis
   ("ModeluXAI live test"), model mengutipnya benar.
5. **Keyring nyata**: `mx config key-import` → "kunci API disimpan di keyring";
   env var dihapus, aplikasi tetap jalan (kunci dibaca dari Credential Manager)
   — turn live sukses tanpa env.
6. **Cache model**: `models-cache.json` tertulis di direktori data uji (TTL 24 jam).

## Sisa

- Tidak ada. V-02 tertutup penuh; korpus golden live (`--event-log`) kini bisa
  direkam untuk harness differential (V-05).