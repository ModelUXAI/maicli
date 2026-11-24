// Bentuk frame protokol JSONL maicli (docs/rpc-v1.md, docs/rpc-v2.md).
// Semua fungsi di sini murni: tidak menyentuh DOM, tidak menyimpan state global.

export type RpcFrame = { type: string; id?: string } & Record<string, unknown>;

/** Batas frame logis setelah reassembly (cermin `MAX_REASSEMBLED_FRAME_BYTES`). */
export const MAX_REASSEMBLED_FRAME_BYTES = 64 * 1024 * 1024;

export class ChunkError extends Error {
  constructor(
    readonly code: "E_CHUNK_INVALID" | "E_CHUNK_TOO_LARGE",
    message: string,
  ) {
    super(message);
    this.name = code;
  }
}

/**
 * Satu-satunya guard objek-JSON di paket ini (dipakai parseFrame + decodeChunk).
 * Field tetap `unknown`; pemakaian memeriksa properti dengan `typeof`.
 */
export function isJsonObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export interface ReadyFrame extends RpcFrame {
  type: "ready";
  protocol: string;
  model: string;
  supported_protocol_versions: number[];
  max_reassembled_frame_bytes: number;
}

export interface StateFrame extends RpcFrame {
  type: "state";
  id: string;
  model: string;
  messages: number;
  busy: boolean;
  queued: number;
  input_tokens: number;
  output_tokens: number;
  /** Tidak dikirim server saat ini; dibaca bila suatu saat ada. */
  context_window?: number;
}

export interface UsageFrame extends RpcFrame {
  type: "usage";
  id: string;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
}

export interface ApprovalRequestFrame extends RpcFrame {
  type: "approval_request";
  id: string;
  tool: string;
  tier: string;
  subject: string;
}

export interface AskDialogFrame extends RpcFrame {
  type: "ask_dialog";
  id: string;
  question: string;
  options: string[];
  recommended?: number;
  timeout_secs: number;
}

export interface SessionInfoFrame extends RpcFrame {
  type: "session_info_update";
  session_id?: string | null;
  model: string;
  title?: string | null;
}

export type AgentEventName =
  | "agent_start"
  | "turn_start"
  | "message_start"
  | "text_delta"
  | "text_end"
  | "tool_start"
  | "tool_end"
  | "turn_end"
  | "agent_end"
  | "notice";

export interface AgentEventPayload {
  event: AgentEventName | string;
  text?: string;
  tool_call_id?: string;
  tool_name?: string;
  preview?: string;
  is_error?: boolean;
  index?: number;
  turns?: number;
}

export interface EventFrame extends RpcFrame {
  type: "event";
  event: AgentEventPayload;
}

export interface PromptResultFrame extends RpcFrame {
  type: "prompt_result";
  id: string;
  agent_invoked: boolean;
  status: "ok" | "aborted" | "error" | string;
  stop_reason?: string | null;
  turns?: number | null;
  error?: string | null;
}

export interface CommandOutputFrame extends RpcFrame {
  type: "command_output";
  text: string;
}

export interface ErrorFrame extends RpcFrame {
  type: "error";
  id?: string;
  code: string;
  message: string;
}

export interface RpcChunkFrame extends RpcFrame {
  type: "rpc_chunk";
  chunk_id: string;
  index: number;
  count: number;
  byte_length: number;
  data: string;
}

export function isReady(frame: RpcFrame): frame is ReadyFrame {
  return frame.type === "ready";
}

export function isState(frame: RpcFrame): frame is StateFrame {
  return frame.type === "state";
}

export function isUsage(frame: RpcFrame): frame is UsageFrame {
  return frame.type === "usage";
}

export function isApprovalRequest(frame: RpcFrame): frame is ApprovalRequestFrame {
  return frame.type === "approval_request";
}

export function isAskDialog(frame: RpcFrame): frame is AskDialogFrame {
  return frame.type === "ask_dialog";
}

export function isSessionInfo(frame: RpcFrame): frame is SessionInfoFrame {
  return frame.type === "session_info_update";
}

export function isEvent(frame: RpcFrame): frame is EventFrame {
  return frame.type === "event" && isJsonObject(frame.event);
}

export function isPromptResult(frame: RpcFrame): frame is PromptResultFrame {
  return frame.type === "prompt_result";
}

export function isCommandOutput(frame: RpcFrame): frame is CommandOutputFrame {
  return frame.type === "command_output" && typeof frame.text === "string";
}

export function isError(frame: RpcFrame): frame is ErrorFrame {
  return frame.type === "error" && typeof frame.code === "string";
}

export function isRpcChunk(frame: RpcFrame): frame is RpcChunkFrame {
  return (
    frame.type === "rpc_chunk" &&
    typeof frame.chunk_id === "string" &&
    typeof frame.index === "number" &&
    typeof frame.count === "number" &&
    typeof frame.byte_length === "number" &&
    typeof frame.data === "string"
  );
}

/**
 * Parse satu baris JSONL. Baris rusak/bukan objek menghasilkan `null`
 * supaya frame cacat tidak pernah menjatuhkan UI.
 */
export function parseFrame(line: string): RpcFrame | null {
  const trimmed = line.trim();
  if (trimmed.length === 0) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return null;
  }
  if (!isJsonObject(parsed)) return null;
  if (typeof parsed.type !== "string" || parsed.type.length === 0) return null;
  if (parsed.id !== undefined && typeof parsed.id !== "string") return null;
  return parsed as RpcFrame;
}

export interface ChunkEntry {
  count: number;
  byteLength: number;
  parts: Map<number, Uint8Array>;
}

export type ChunkAccumulator = Map<string, ChunkEntry>;

function decodeBase64(data: string): Uint8Array {
  let binary: string;
  try {
    binary = atob(data);
  } catch {
    throw new ChunkError("E_CHUNK_INVALID", "chunk base64 tidak valid");
  }
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/**
 * Kumpulkan `rpc_chunk` per `chunk_id`. Mengembalikan teks frame logis bila
 * seluruh potongan sudah lengkap, `null` bila belum. Akumulasi dibuang dan
 * error dilempar bila potongan tidak konsisten.
 */
export function decodeChunk(
  acc: ChunkAccumulator,
  frame: RpcFrame,
): string | null {
  if (!isRpcChunk(frame)) return null;
  const { chunk_id, index, count, data, byte_length } = frame;

  if (
    !Number.isInteger(index) ||
    !Number.isInteger(count) ||
    count <= 0 ||
    index < 0 ||
    index >= count
  ) {
    acc.delete(chunk_id);
    throw new ChunkError(
      "E_CHUNK_INVALID",
      `chunk ${chunk_id}: indeks ${index} di luar 0..${count - 1}`,
    );
  }

  if (byte_length > MAX_REASSEMBLED_FRAME_BYTES) {
    acc.delete(chunk_id);
    throw new ChunkError(
      "E_CHUNK_TOO_LARGE",
      `chunk ${chunk_id}: ${byte_length} byte melebihi batas ${MAX_REASSEMBLED_FRAME_BYTES}`,
    );
  }

  const bytes = decodeBase64(data);
  const existing = acc.get(chunk_id);

  if (!existing) {
    acc.set(chunk_id, { count, byteLength: byte_length, parts: new Map([[index, bytes]]) });
  } else {
    if (existing.count !== count || existing.byteLength !== byte_length) {
      acc.delete(chunk_id);
      throw new ChunkError(
        "E_CHUNK_INVALID",
        `chunk ${chunk_id}: metadata berubah di tengah reassembly`,
      );
    }
    if (existing.parts.has(index)) {
      acc.delete(chunk_id);
      throw new ChunkError(
        "E_CHUNK_INVALID",
        `chunk ${chunk_id}: indeks ${index} duplikat`,
      );
    }
    existing.parts.set(index, bytes);
  }

  const entry = acc.get(chunk_id);
  if (!entry || entry.parts.size < entry.count) return null;

  const total = new Uint8Array(entry.byteLength);
  let offset = 0;
  for (let i = 0; i < entry.count; i += 1) {
    const part = entry.parts.get(i);
    if (!part) {
      acc.delete(chunk_id);
      throw new ChunkError("E_CHUNK_INVALID", `chunk ${chunk_id}: potongan ${i} hilang`);
    }
    if (offset + part.byteLength > total.byteLength) {
      acc.delete(chunk_id);
      throw new ChunkError(
        "E_CHUNK_INVALID",
        `chunk ${chunk_id}: isi melebihi byte_length`,
      );
    }
    total.set(part, offset);
    offset += part.byteLength;
  }
  if (offset !== entry.byteLength) {
    acc.delete(chunk_id);
    throw new ChunkError(
      "E_CHUNK_INVALID",
      `chunk ${chunk_id}: ${offset} byte terkumpul, seharusnya ${entry.byteLength}`,
    );
  }

  acc.delete(chunk_id);
  const text = new TextDecoder("utf-8").decode(total);

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new ChunkError("E_CHUNK_INVALID", `chunk ${chunk_id}: hasil bukan JSON`);
  }
  if (!isJsonObject(parsed)) {
    throw new ChunkError("E_CHUNK_INVALID", `chunk ${chunk_id}: hasil bukan objek`);
  }
  return text;
}
