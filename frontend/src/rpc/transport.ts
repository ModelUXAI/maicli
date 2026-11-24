// Transport JSONL ke `maicli --mode rpc`.
//
// Di desktop, `gui/src/lib.rs` menjalankan child process dan meneruskan tiap
// baris stdout sebagai event Tauri `rpc-line`; `rpc_send` menulis satu baris ke
// stdin child. Di browser (dev) hanya FixtureTransport yang dipakai preview.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface RpcTransport {
  send(line: string): void;
  onLine(cb: (line: string) => void): () => void;
  /** Opsional: lepaskan sumber daya (mis. listener event Tauri). */
  dispose?(): void;
}

function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Transport nyata lewat bridge Tauri. */
export class TauriTransport implements RpcTransport {
  private listeners = new Set<(line: string) => void>();
  private unlisten: UnlistenFn | null = null;
  private disposed = false;

  constructor() {
    void listen<string>("rpc-line", (event) => {
      if (typeof event.payload === "string") {
        this.emitLine(event.payload);
      }
    }).then((off) => {
      if (this.disposed) off();
      else this.unlisten = off;
    });
  }

  private emitLine(line: string): void {
    this.listeners.forEach((cb) => cb(line));
  }

  private emitLocalError(message: string): void {
    this.emitLine(
      JSON.stringify({ type: "error", code: "E_RPC", message }),
    );
  }

  send(line: string): void {
    void invoke("rpc_send", { line }).catch((err: unknown) => {
      this.emitLocalError(String(err));
    });
  }

  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }

  dispose(): void {
    this.disposed = true;
    this.listeners.clear();
    this.unlisten?.();
    this.unlisten = null;
  }
}

/** Transport yang tidak terhubung ke proses apa pun (lingkungan tanpa Tauri). */
export class OfflineTransport implements RpcTransport {
  private listeners = new Set<(line: string) => void>();

  send(): void {
    const payload = JSON.stringify({
      type: "error",
      code: "E_NO_BRIDGE",
      message:
        "Bridge Tauri tidak tersedia di lingkungan ini; jalankan aplikasi desktop maicli.",
    });
    this.listeners.forEach((cb) => cb(payload));
  }

  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }
}

export function createTransport(): RpcTransport {
  return hasTauri() ? new TauriTransport() : new OfflineTransport();
}
