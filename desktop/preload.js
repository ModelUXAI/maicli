// Preload Electron: satu-satunya jembatan antara renderer dan main process.
// Renderer tidak pernah melihat Node.js / child process secara langsung.

const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("maicliDesktop", {
  /** Kirim satu baris JSONL ke core Rust. */
  send(line) {
    return ipcRenderer.invoke("rpc-send", line);
  },
  /** Terima frame JSONL dari core. Callback dipanggil dengan string. */
  onLine(callback) {
    const handler = (_event, line) => {
      if (typeof line === "string") callback(line);
    };
    ipcRenderer.on("rpc-line", handler);
    return () => ipcRenderer.removeListener("rpc-line", handler);
  },
});
