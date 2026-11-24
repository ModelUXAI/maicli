// Jembatan child process `maicli --mode rpc` — dipakai main.js dan smoke test.
// Tidak bergantung pada Electron sama sekali, sehingga bisa diuji langsung.

const { spawn } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");

function resolveBinary() {
  const env = process.env.MAICLI_BIN;
  if (env && fs.existsSync(env)) return env;
  const bundled = path.join(__dirname, "bin", process.platform === "win32" ? "maicli.exe" : "maicli");
  if (fs.existsSync(bundled)) return bundled;
  return "maicli";
}

/**
 * Jalankan core RPC dan teruskan tiap baris stdout ke `onLine`.
 * @returns {{ send(line: string): boolean, kill(): void, child: import("node:child_process").ChildProcess }}
 */
function startRpcBridge(onLine, onError = () => {}) {
  const child = spawn(resolveBinary(), ["--mode", "rpc"], {
    stdio: ["pipe", "pipe", "ignore"],
    env: { ...process.env, MODELUXAI_API_KEY: process.env.MODELUXAI_API_KEY || "dummy-local-for-smoke" },
  });

  let buffer = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let index;
    while ((index = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, index);
      buffer = buffer.slice(index + 1);
      if (line.trim()) onLine(line);
    }
  });
  child.on("error", onError);
  child.on("exit", (code) => onError(new Error(`proses RPC keluar dengan kode ${code}`)));

  return {
    child,
    send(line) {
      if (!child.stdin || child.exitCode !== null) return false;
      child.stdin.write(`${line}\n`);
      return true;
    },
    kill() {
      if (child.exitCode === null) child.kill();
    },
  };
}

module.exports = { startRpcBridge, resolveBinary };
