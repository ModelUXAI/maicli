// Klien RPC bertipe untuk protokol maicli.
//
// Tanggung jawab: korelasi request/balasan lewat `id`, reassembly `rpc_chunk`
// v2, negosiasi protokol, dan meneruskan semua frame ke `onFrame` tanpa
// mengubah bentuknya.

import {
  ChunkError,
  decodeChunk,
  isError,
  isReady,
  isRpcChunk,
  parseFrame,
  type ChunkAccumulator,
  type RpcFrame,
} from "./frames";
import type { RpcTransport } from "./transport";

export type ApprovalDecision = "once" | "session" | "always" | "deny";

export class RpcError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "RpcError";
  }
}

/**
 * Frame ber-`id` yang menutup sebuah request. Frame tanpa id (mis.
 * `command_output`) tidak pernah menutup request; teksnya diambil terpisah.
 */
function settles(type: string): boolean {
  return (
    type === "pong" ||
    type === "ack" ||
    type === "result" ||
    type === "prompt_result" ||
    type === "usage" ||
    type === "state" ||
    type === "negotiated" ||
    type === "ask_dialog_configured" ||
    type === "config_update"
  );
}

interface Pending {
  resolve: (frame: RpcFrame) => void;
  reject: (error: Error) => void;
  /** Teks `command_output` yang tiba sebelum `ack` (frame itu tanpa `id`). */
  commandText?: string | null;
}

export class RpcClient {
  private seq = 0;
  private pending = new Map<string, Pending>();
  private off: () => void;
  private chunks: ChunkAccumulator = new Map();
  /**
   * Antrean `command` yang belum menerima `ack`. `command_output` tidak
   * membawa id, jadi teksnya diberikan ke perintah paling depan yang belum
   * menerima output — bukan ke satu variabel bersama.
   */
  private commandQueue: Pending[] = [];
  private negotiated = false;
  private disposed = false;

  /** Semua frame (sesudah reassembly) diteruskan apa adanya ke sini. */
  onFrame?: (frame: RpcFrame) => void;

  constructor(private transport: RpcTransport, private protocolVersion: number | null = null) {
    this.off = transport.onLine((line) => this.handleLine(line));
  }

  dispose(): void {
    this.disposed = true;
    this.off();
    // Transport Tauri menyimpan `unlisten`; lepas juga agar tidak bocor.
    this.transport.dispose?.();
    for (const [, pending] of this.pending) {
      pending.reject(new RpcError("E_DISPOSED", "klien RPC sudah ditutup"));
    }
    this.pending.clear();
    this.chunks.clear();
    this.commandQueue = [];
  }

  private handleLine(line: string): void {
    let effective = line;

    const raw = parseFrame(line);
    if (raw && isRpcChunk(raw)) {
      try {
        const complete = decodeChunk(this.chunks, raw);
        if (complete === null) return;
        effective = complete;
      } catch (error) {
        const chunkError = error as ChunkError;
        this.deliver({
          type: "error",
          code: chunkError.code ?? "E_CHUNK_INVALID",
          message: chunkError.message ?? String(error),
        });
        return;
      }
    }

    const frame = parseFrame(effective);
    if (!frame) return;

    if (isReady(frame)) {
      this.maybeNegotiate(frame);
    }

    this.deliver(frame);
  }

  private maybeNegotiate(frame: RpcFrame): void {
    if (this.negotiated) return;
    const versions = frame.supported_protocol_versions;
    if (!Array.isArray(versions) || !versions.includes(2)) return;
    this.negotiated = true;
    this.transport.send(
      JSON.stringify({ type: "negotiate_protocol", id: `g${++this.seq}`, protocol_version: 2 }),
    );
  }

  private deliver(frame: RpcFrame): void {
    // `command_output` tidak membawa id (lihat RpcFrame::CommandOutput), jadi
    // teksnya ditempelkan ke perintah paling depan yang belum menerima output.
    if (frame.type === "command_output") {
      const target = this.commandQueue.find((pending) => pending.commandText === null);
      if (target) {
        target.commandText = typeof frame.text === "string" ? frame.text : "";
      }
    }

    const id = frame.id;
    if (typeof id === "string") {
      const pending = this.pending.get(id);
      if (pending) {
        if (isError(frame)) {
          this.pending.delete(id);
          pending.reject(new RpcError(frame.code, frame.message));
        } else if (settles(frame.type)) {
          this.pending.delete(id);
          if (pending.commandText !== undefined) {
            this.commandQueue = this.commandQueue.filter((entry) => entry !== pending);
            pending.resolve({ type: "command_output", text: pending.commandText });
          } else {
            pending.resolve(frame);
          }
        }
      }
    }
    this.onFrame?.(frame);
  }

  private sendFrame(type: string, fields: Record<string, unknown> = {}): string {
    const id = `g${++this.seq}`;
    this.transport.send(JSON.stringify({ type, id, ...fields }));
    return id;
  }

  private request(
    type: string,
    fields: Record<string, unknown> = {},
    timeoutMs: number | null = null,
  ): Promise<RpcFrame> {
    if (this.disposed) {
      return Promise.reject(new RpcError("E_DISPOSED", "klien RPC sudah ditutup"));
    }
    const id = this.sendFrame(type, fields);
    const { promise, resolve, reject } = Promise.withResolvers<RpcFrame>();
    this.pending.set(id, { resolve, reject });
    if (timeoutMs !== null) {
      setTimeout(() => {
        if (this.pending.delete(id)) {
          reject(new RpcError("E_TIMEOUT", `${type} tidak dijawab dalam ${timeoutMs} ms`));
        }
      }, timeoutMs);
    }
    return promise;
  }

  // --- API yang dipakai UI ---

  ping(): Promise<RpcFrame> {
    return this.request("ping", {}, 5_000);
  }

  getState(): Promise<RpcFrame> {
    return this.request("get_state", {}, 5_000);
  }

  getUsage(): Promise<RpcFrame> {
    return this.request("usage", {}, 5_000);
  }

  /** Kirim prompt; selesai saat `prompt_result` (atau `result` server lama). */
  async prompt(message: string): Promise<RpcFrame> {
    if (this.disposed) throw new RpcError("E_DISPOSED", "klien RPC sudah ditutup");
    const id = this.sendFrame("prompt", { message });
    const { promise, resolve, reject } = Promise.withResolvers<RpcFrame>();
    this.pending.set(id, { resolve, reject });
    return promise;
  }

  /** Abort tanpa id: server tidak membalas `ack` untuk abort. */
  abort(): void {
    this.transport.send(JSON.stringify({ type: "abort" }));
  }

  async setModel(model: string): Promise<RpcFrame> {
    return this.request("model", { model }, 15_000);
  }

  /** Selesai saat `ack`, membawa teks `command_output` yang tiba lebih dulu. */
  async command(text: string): Promise<RpcFrame> {
    if (this.disposed) throw new RpcError("E_DISPOSED", "klien RPC sudah ditutup");
    const id = this.sendFrame("command", { text });
    const { promise, resolve, reject } = Promise.withResolvers<RpcFrame>();
    const pending: Pending = { resolve, reject, commandText: null };
    this.pending.set(id, pending);
    this.commandQueue.push(pending);
    setTimeout(() => {
      if (this.pending.delete(id)) {
        this.commandQueue = this.commandQueue.filter((entry) => entry !== pending);
        reject(new RpcError("E_TIMEOUT", "command tidak dijawab dalam 20000 ms"));
      }
    }, 20_000);
    return promise;
  }

  /**
   * Server tidak mengirim `ack` untuk `approval`: jawaban langsung memutus
   * penantian agen, dan ketiadaan approval aktif dibalas `error` tanpa id.
   * Karena itu kirim saja tanpa menunggu balasan ber-id.
   */
  approve(id: string, decision: ApprovalDecision): void {
    this.sendFrame("approval", { id, decision });
  }

  /** Sama seperti `approve`: `ask_dialog_answer` tidak dibalas `ack`. */
  askAnswer(id: string, selected: number | null, text: string): void {
    this.sendFrame("ask_dialog_answer", { id, selected, text });
  }

  /** Balasan `question` (tanpa id) juga tidak dibalas `ack`. */
  answer(text: string): void {
    this.transport.send(JSON.stringify({ type: "answer", text }));
  }

  async negotiateProtocol(version: number): Promise<RpcFrame> {
    this.protocolVersion = version;
    return this.request("negotiate_protocol", { protocol_version: version }, 5_000);
  }

  /** Diuji di frames/client test; tidak dipakai UI secara langsung. */
  get protocol(): number | null {
    return this.protocolVersion;
  }
}
