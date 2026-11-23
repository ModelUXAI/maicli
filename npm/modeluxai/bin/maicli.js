#!/usr/bin/env node
/* Wrapper maicli: cari biner platform yang terpasang sebagai optionalDependencies
   (pola esbuild/oxc), lalu spawn dengan argumen apa adanya. Tanpa jaringan. */
"use strict";

const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");

const PLATFORM = {
  "linux-x64": "modeluxai-linux-x64",
  "darwin-arm64": "modeluxai-darwin-arm64",
  "darwin-x64": "modeluxai-darwin-x64",
  "win32-x64": "modeluxai-win32-x64",
};

const key = `${process.platform}-${process.arch}`;
const pkgName = PLATFORM[key];
if (!pkgName) {
  console.error(
    `maicli: platform tidak didukung: ${key}\n` +
      "yang tersedia: linux-x64, darwin-arm64, darwin-x64, win32-x64.\n" +
      "alternatif: build dari sumber (cargo install --path crates/tui-app)."
  );
  process.exit(1);
}

function resolveBinary() {
  // 1) di node_modules platform package (npm install standar)
  try {
    const dir = path.dirname(require.resolve(`${pkgName}/package.json`));
    const name = process.platform === "win32" ? "maicli.exe" : "maicli";
    const p = path.join(dir, name);
    if (fs.existsSync(p)) return p;
  } catch (_) {
    /* fallback */
  }
  // 2) sedi-sedi: biner di samping wrapper (tarball manual)
  const local = path.join(__dirname, process.platform === "win32" ? "maicli.exe" : "maicli");
  if (fs.existsSync(local)) return local;
  return null;
}

const binary = resolveBinary();
if (!binary) {
  console.error(
    `maicli: paket biner '${pkgName}' tidak terpasang.\n` +
      "coba: npm install -g @modeluxai/cli --force  (atau unduh dari GitHub Releases)."
  );
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: false,
});
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 0);
});
child.on("error", (err) => {
  console.error(`maicli: gagal menjalankan biner: ${err.message}`);
  process.exit(1);
});
