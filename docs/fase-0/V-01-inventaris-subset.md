# V-01 — inventaris perilaku subset port (M1)

Tujuan: daftar perilaku omp yang **wajib** benar di v1, dengan rujukan path:baris.
Kolom **sumber**: `baca` = dibaca ulang di sesi ini; `S-1 §x` = dikutip dari
`laporan-cara-kerja-oh-my-pi.md` dan belum diverifikasi ulang.

## A. Loop dan stopReason

| # | Perilaku | Rujukan | Sumber | Diuji di |
|---|---|---|---|---|
| A1 | Dua tingkat: outer `while(true)` untuk follow-up, inner untuk tool + steering | `packages/agent/src/agent-loop.ts:1368,1372` | baca | `follow_up_menjalankan_turn_kedua` |
| A2 | `stop` diperlakukan sama dengan `toolUse` saat menentukan eksekusi tool | `agent-loop.ts:1660-1667,1680-1681` | baca | `tool_call_runnable_dieksekusi_lalu_lanjut` |
| A3 | `length` **tidak** mengeksekusi tool; setiap `tool_use` dipasangkan `tool_result` placeholder lalu turn berakhir | `agent-loop.ts:1758-1781` | baca | `turn_length_tidak_menjalankan_tool_dan_memasang_placeholder` |
| A4 | `error`/`aborted` kembali lebih awal (placeholder + stop) | `agent-loop.ts:1645-1655` | baca | (jalur error provider) |
| A5 | Deadline dicek di awal iterasi dan sebelum dispatch tool | `packages/agent/src/agent-loop.ts:1373-1377` (awal iterasi; verifikasi ulang 2026-11-18) + `:1715-1718` (sebelum dispatch: `hasMoreToolCalls && deadlinePassed` menghentikan rantai tool) | baca-ulang | **TERVERIFIKASI** — implementasi kita: `deadline_passed` awal iterasi (`agent.rs`) + penjaga deadline membatalkan tool berjalan (`deadline_membatalkan_tool_yang_sedang_berjalan`) |
| A6 | Pause gate proses menghentikan di batas turn | `packages/agent/src/agent-loop.ts:1378-1381` (`agentPauseGate.paused` → park; verifikasi ulang 2026-11-18) | baca-ulang | **TERPORT** — kita: pause gate di awal iterasi + setelah `TurnEnd` (`pause_menolak_turn_berikutnya`, `pause_di_tengah_turn_berhenti_sebelum_panggilan_model_berikut`, `pause_setelah_tool_menghentikan_turn_sebelum_follow_up`) |
| A7 | Antrean dipindahkan ke konteks tepat sebelum panggilan model | `agent-loop.ts:1386-1396` | baca | `steering_masuk_sebagai_pesan_pengguna…` |
| A8 | Tidak ada cap langkah global; yang ada: `MAX_PAUSED_TURN_CONTINUATIONS=8`, `MAX_DSML_LEAK_NUDGES=2`, `MAX_SOFT_TOOL_ESCALATIONS=3` | `agent-loop.ts:154,162,170`; penegakan `:1704-1708,1793-1825` | baca + F-03 | — |
| A9 | Force-loop soft tool dihentikan dengan error setelah 3 percobaan | `agent-loop.ts:1704-1708` | baca | — |

## B. Retry, kompaksi, approval, sesi

| # | Perilaku | Rujukan | Sumber | Catatan |
|---|---|---|---|---|
| B1 | Retry 3 percobaan, basis 500 ms, maks 30 dtk, jitter 75–100 %, patuhi `retry-after` | `packages/agent/src/oneshot-retry.ts:97-122` | S-1 §4.4 | di v1: `RetryPolicy` + test batas jeda |
| B2 | Tidak di-retry: parse tool-call, content-blocked, context-overflow, payload-rejected | `oneshot-retry.ts:97-122` | S-1 §4.4 | dipetakan ke `GatewayRejected` (non-retryable) |
| B3 | Urutan metode kompaksi `remote → snapcompact → handoff → shake → soft` | `session/compaction-methods.ts:10-51` | S-1 §5.1 | di luar v1 (C-07) |
| B4 | Ambang konteks: reserve 16.384, keepRecent 20.000, recovery band 0,8, MAX_SUMMARY 16.384 | `agent/src/compaction/compaction.ts:194-212` | S-1 §5.1 | di luar v1 (C-07) |
| B5 | 4 mode approval; default omp `yolo`, setting hilang → fail-closed `always-ask` | `tools/settings.ts:318` | S-1 §9 | v1 default **`always-ask`** (lebih aman; dicatat di WORK.md) |
| B6 | Urutan resolusi approval: folder terlarang → per-tool → default tool → tier vs mode/grant | `tools/approval.ts:97,366` | S-1 §9 + baca nama fungsi | `tui-approval` (7 test) |
| B7 | Sesi JSONL v3; header punya slot judul 256 byte; pohon `parentId`/`activeLeafId` | `session/session-entries.ts:17-70` | S-1 §6 | v1 memakai skema sendiri, lihat `docs/skema-sesi-v1.md` |
| B8 | Tulis sesi atomik (staging → rename) | `session/session-storage.ts` | S-1 §6 | `write_atomic` + test |

## C. Eksekusi tool

| # | Perilaku | Rujukan | Sumber | Diuji di |
|---|---|---|---|---|
| C1 | Tool dipanggil dengan `signal`; abort kooperatif (bash membunuh proses) | `agent-loop.ts:3731-3747`; `exec/bash-executor.ts:882-902` | F-03 (audit) | — (v1 memakai timeout sendiri) |
| C2 | Tool yang gagal → `isError: true` + teks error | `agent-loop.ts:3756-3763` | F-03 (audit) | `write`/`bash`/`ask` test |
| C3 | Tool tak dikenal = error bernama, bukan panic | — (kontrak v1) | — | `tool_tidak_dikenal_menghasilkan_error_bernama` |
| C4 | Path di luar cwd / `..` ditolak | — (kontrak v1, lihat `RENCANA.md` §13) | — | `read_menolak_traversal` |

## Kesimpulan V-01

Subset v1 sudah punya rujukan untuk 21 perilaku inti; 12 di antaranya diverifikasi ulang
langsung dari source di sesi ini. Perilaku B1–B8 dan C1–C2 masih bersandar pada laporan/audit
dan **wajib** diverifikasi ulang sebelum kartu terkait dinyatakan DONE.
