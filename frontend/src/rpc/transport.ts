// Transport JSONL ke `maicli --mode rpc`.
//
// Di desktop Electron, `desktop/main.js` menjalankan child process dan
// meneruskan tiap baris stdout lewat IPC `rpc-line`; `maicliDesktop.send`
// menulis satu baris ke stdin child. Di browser (dev/preview) tidak ada bridge,
// sehingga OfflineTransport dipakai dan UI tetap jujur tanpa data rekaan.

export interface RpcTransport {
  send(line: string): void;
  onLine(cb: (line: string) => void): () => void;
  /** Lepaskan sumber daya (listener IPC). */
  dispose?(): void;
}

/** Jembatan yang diekspos `desktop/preload.js` lewat contextBridge. */
interface DesktopBridge {
  send(line: string): Promise<void>;
  onLine(callback: (line: string) => void): () => void;
}

interface ElectronWindow extends Window {
  maicliDesktop?: DesktopBridge;
}

function bridge(): DesktopBridge | null {
  if (typeof window === "undefined") return null;
  return (window as ElectronWindow).maicliDesktop ?? null;
}

/** Transport nyata lewat bridge Electron. */
export class ElectronTransport implements RpcTransport {
  private off: (() => void) | null = null;

  constructor(private api: DesktopBridge) {
    this.off = api.onLine((line) => this.listeners.forEach((cb) => cb(line)));
  }

  private listeners = new Set<(line: string) => void>();

  private emitLocalError(message: string): void {
    const line = JSON.stringify({ type: "error", code: "E_RPC", message });
    this.listeners.forEach((cb) => cb(line));
  }

  send(line: string): void {
    void this.api.send(line).catch((error: unknown) => {
      this.emitLocalError(String(error));
    });
  }

  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }

  dispose(): void {
    this.listeners.clear();
    this.off?.();
    this.off = null;
  }
}

/** Transport tanpa bridge (browser biasa): tidak mengarang balasan. */
export class OfflineTransport implements RpcTransport {
  private listeners = new Set<(line: string) => void>();

  send(): void {
    const payload = JSON.stringify({
      type: "error",
      code: "E_NO_BRIDGE",
      message:
        "Bridge desktop tidak tersedia di browser ini; jalankan aplikasi desktop maicli.",
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
  const api = bridge();
  return api ? new ElectronTransport(api) : new OfflineTransport();
}
