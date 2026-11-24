// Jembatan ke `maicli --mode rpc` (protokol JSONL v1, docs/rpc-v1.md).
// Di desktop Tauri, proses dijalankan oleh crate `gui`; di browser (dev) klien
// bisa diarahkan ke endpoint WebSocket lokal bila tersedia.

export type Frame = Record<string, unknown> & { type?: string };

export interface RpcTransport {
  send(line: string): void;
  onLine(cb: (line: string) => void): () => void;
}

export interface SessionItem {
  path?: string;
  messages?: number;
  title?: string;
  [key: string]: unknown;
}

/** Klien minimal untuk satu sesi prompt. */
export class RpcClient {
  private seq = 0;
  private off?: () => void;
  private pending = new Map<string, (frame: Frame) => void>();

  constructor(private transport: RpcTransport) {
    this.off = transport.onLine((line) => {
      let frame: Frame;
      try {
        frame = JSON.parse(line) as Frame;
      } catch {
        return;
      }
      const id = frame.id;
      if (typeof id === "string" && this.pending.has(id)) {
        this.pending.get(id)?.(frame);
        this.pending.delete(id);
      }
      this.onFrame?.(frame);
    });
  }

  onFrame?: (frame: Frame) => void;

  dispose(): void {
    this.off?.();
    this.pending.clear();
  }

  private request(type: string, fields: Frame = {}): Promise<Frame> {
    const id = `g${++this.seq}`;
    const { promise, resolve } = Promise.withResolvers<Frame>();
    this.pending.set(id, resolve);
    this.transport.send(JSON.stringify({ type, id, ...fields }));
    return promise;
  }

  async ping(): Promise<Frame> {
    return this.request("ping");
  }

  async prompt(message: string): Promise<Frame> {
    return this.request("prompt", { message });
  }
  async listSessions(): Promise<SessionItem[]> {
    // Daftar sesi datang lewat event `sessions_listed` bila didukung host.
    const { promise, resolve } = Promise.withResolvers<SessionItem[]>();
    const timer = setTimeout(() => resolve([]), 1500);
    const prev = this.onFrame;
    this.onFrame = (frame) => {
      prev?.(frame);
      const payload = frame.event as Frame | undefined;
      if (payload?.event === "sessions_listed") {
        clearTimeout(timer);
        this.onFrame = prev;
        resolve((payload.sessions as SessionItem[]) ?? []);
      }
    };
    this.transport.send(JSON.stringify({ type: "list_sessions", id: `g${++this.seq}` }));
    return promise;
  }
}
