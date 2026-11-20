# F-02 — inventaris definitif

Status: DONE (untuk tool/slash/sesi). Angka provider dicatat dengan sumber masing-masing.

## Tool

```
> (baca) packages/coding-agent/src/tools/builtin-names.ts:3-33 → BUILTIN_TOOL_NAMES
```

| Kelompok | Jumlah | Catatan |
|---|---|---|
| Builtin terdaftar | **29** | `read`…`learn` |
| Hidden | **3** | `think`, `yield`, `goal` |
| Khusus `detected`/host | `find`, `ida`, MCP, extension tool, xdev | bukan `AgentTool` builtin |

Konsekuensi: `RENCANA.md` Lampiran B menulis "30 + 3" → **selisih 1**, amandemen oleh iqbal.

## Slash command

89 top-level dari 8 modul (`src/slash-commands/builtin-registry.ts:46-55`), dengan 12 di antaranya
milik v1 (`new`, `compact`, `model`, `todo`, `usage`, `session`, `tools`, `context`, `resume`, `fork`,
`pause`, `quit`).

## Provider / katalog

| Metrik | Angka | Sumber |
|---|---|---|
| Berkas KDL | 105 provider + 109 auth + 23 taxonomy + 19 classes + 1 runtime = 257 | `packages/catalog/src/compat/rules/` |
| Kompilator KDL | 8 berkas / 2.829 baris | `packages/catalog/scripts/compat-compiler/` |
| Provider ber-entri katalog | **85** — diverifikasi ulang 2026-11-18: `packages/catalog/src/models.json` berisi 85 entri provider (angka 96 milik laporan versi omp lama) |
| Provider di `models.json` | 85 | `packages/catalog/src/models.json` |
| Transport ber-kode | 5 | `packages/ai/src/registry/registry.ts:26-46` |

## Sesi

`CURRENT_SESSION_VERSION = 3`; slot judul 256 byte (`session/session-entries.ts:17`).

## Jawaban Unknown yang berdampak desain (F-03)

| # | Pertanyaan | Jawaban | Bukti |
|---|---|---|---|
| U-3 | Ada cap langkah global? | **Tidak ada.** Hanya `MAX_PAUSED_TURN_CONTINUATIONS=8`, `MAX_DSML_LEAK_NUDGES=2`, `MAX_SOFT_TOOL_ESCALATIONS=3`; deadline = waktu, bukan langkah | `agent-loop.ts:1368,1372,154,162,170` |
| U-4 | Deadline saat tool berjalan? | Signal deadline diteruskan ke tool; bash memasang listener abort → membunuh proses. Abort bersifat **kooperatif** | `agent-loop.ts:1258,3412,3731`; `exec/bash-executor.ts:882-902` |
| U-5 | Ada ketergantungan TTY/home di sesi? | TTY: tidak. `~`: ya, tapi bisa di-override (`options.agentDir ?? getAgentDir()`) | `sdk.ts:2015-2016`; `utils/src/dirs.ts:367` |
| U-6 | Field `AgentTool` | 25+ field; `loadMode` = presentasi, `approval` default **`exec`**, `concurrency` shared/exclusive, `interruptible` menentukan signal | `agent/src/types.ts:1170-1313` |
| U-8 | Hak extension/hook | **In-process, tanpa sandbox**: `import()` + `new Function`, akses `pi.exec`, `pi` SDK penuh | `extensibility/plugins/legacy-pi-compat.ts:2170,2751`; `docs/extension-loading.md:141` |
| U-9 | Multi-tenant/auth di RPC/ACP/SDK? | **Tidak ada.** Hanya approval per panggilan | `modes/rpc/*`, `sdk.ts:2003` |
| U-17 | Alur MCP | Konfig `mcp.json`, transport stdio/http/sse, tool `mcp__<server>_<tool>`, `approval = "write"` → gate yang sama; kegagalan per-server terisolasi | `mcp/tool-bridge.ts:572,767`; `docs/mcp-runtime-lifecycle.md:25-57` |

Konsekuensi untuk v1: extension/plugin **tidak** diport sebagai kode in-process; ABI baru
(WASM/deklaratif) diputuskan di F-08 (belum dijalankan).
