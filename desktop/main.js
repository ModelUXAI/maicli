// Electron main process untuk maicli.
//
// Peran: jalankan `maicli --mode rpc` lewat rpc-bridge.js, teruskan frame JSONL
// dua arah antara frontend TypeScript dan core Rust. Tidak ada logika agen di sini.

const { app, BrowserWindow, ipcMain } = require("electron");
const path = require("node:path");
const { startRpcBridge } = require("./rpc-bridge");

let bridge = null;
let mainWindow = null;

function ensureBridge() {
  if (bridge && bridge.child.exitCode === null) return bridge;
  bridge = startRpcBridge(
    (line) => {
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send("rpc-line", line);
      }
    },
    (error) => {
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send(
          "rpc-line",
          JSON.stringify({ type: "error", code: "E_RPC", message: String(error) }),
        );
      }
    },
  );
  return bridge;
}

ipcMain.handle("rpc-send", (_event, line) => {
  const active = ensureBridge();
  if (!active.send(line)) throw new Error("proses RPC tidak aktif");
});

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 860,
    minHeight: 600,
    title: "maicli — ModeluXAI",
    backgroundColor: "#0b141c",
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  mainWindow.loadFile(path.join(__dirname, "frontend-dist", "index.html")).catch((error) => {
    console.error("[maicli] gagal memuat frontend:", error);
  });


  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

app.whenReady().then(() => {
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
  bridge?.kill();
  bridge = null;
});
