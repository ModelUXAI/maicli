# F-05 — skema wire RPC dan generator

Status: DONE (audit baca-source). **DITUTUP untuk implementasi (keputusan iqbal, opsi B):**
generator wire omp tidak dikerjakan — paritas omp dilepas dan kita sudah punya
tipe wire + decoder sendiri (`tui-wire::rpc`: v1+v2, `rpc_chunk`, host tools/URIs).
Audit di bawah dipertahankan sebagai referensi.

## Skema

| Metrik | Nilai |
|---|---|
| Path | `packages/coding-agent/src/modes/rpc/wire/rpc-wire.schema.json` |
| Ukuran | 136.168 byte / 7.269 baris |
| `$defs` | **300** |
| Perintah (`x-rpc.commands`) | **74** |
| Versi | tidak ada key `version`; versi = field `ready.protocolVersion` |
| Kesegaran | `test/rpc-wire/generated.test.ts` (regenerasi penuh) |

Contoh definisi: `StopReason` = enum `["stop","length","toolUse","error","aborted"]` (baris 839-848);
`RpcServerFrame` = `anyOf` [`RpcResponse`, `RpcHostRequest`, `RpcNotification`] (6148-6161).

## Generator

| Berkas | Baris | Isi |
|---|---|---|
| `scripts/gen-rpc/index.ts` | 41 | orkestrasi 5 emitter |
| `scripts/gen-rpc/model.ts` | 209 | IR bersama (`WireType`/`WireField`/`WireDef`/`WireModel`) |
| `scripts/gen-rpc/typescript.ts` | 78 | tipe saja |
| `scripts/gen-rpc/rust.ts` | **760** | tipe + serde + decoder + `Command` impl |
| `go.ts` / `python.ts` | 795 / 811 | setara Go/Python |

Keluaran Rust: `sdk/rust/omp-rpc/src/wire.rs` — 7.460 baris (6.720 non-kosong), **337 struct**,
**55 enum**, **74 `impl Command`**, **0** `#[serde(tag=…)]` (union ditangani `from_value` manual).

## Framing yang wajib ditiru

`frame.rs` (199 baris): `MAX_FRAME_BYTES = 1 MiB`, `MAX_REASSEMBLED_BYTES = 64 MiB`,
`CHUNK_PAYLOAD_BYTES = 256 KiB`, `MAX_CHUNK_COUNT = 256`; base64 kanonik; error framing = fatal.
Handshake: server menulis `ready` lebih dulu dengan `maxFrameBytes: 1048576` +
`maxReassembledFrameBytes: 67108864`, baru v2 (`rpc_chunk`) boleh dipakai setelah `negotiate_protocol`.

## Verdict

Porting generator = pekerjaan mekanis: `model.ts` (209 baris) + `rust.ts` (760 baris) →
perkiraan **1.000–1.280 baris Rust** untuk `xtask gen-wire`. Tidak perlu omptype/Bun.
Catatan: header hasil generate omp memakai `#![allow(dead_code, clippy::all, rustdoc::all)]` —
bertentangan dengan `AGENTS.md` §4, jadi generator kita harus memakai `allow` sempit atau
menandai modul generated secara eksplisit.
