// Smoke test jembatan RPC — dijalankan dengan Node biasa (tanpa Electron/layar).
// node desktop/smoke-rpc.js
//
// Memakai rpc-bridge.js yang SAMA dengan main.js, jadi ini benar-benar menguji
// jalur produksi, bukan duplikasi logika.

const { startRpcBridge } = require("./rpc-bridge");
const fs = require("node:fs");

const received = [];
const bridge = startRpcBridge(
  (line) => received.push(JSON.parse(line)),
  (error) => console.error("[smoke] error RPC:", String(error)),
);

function waitFor(predicate, timeoutMs) {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const check = () => {
      const hit = received.find(predicate);
      if (hit) return resolve(hit);
      if (Date.now() - started > timeoutMs) return reject(new Error("timeout"));
      setTimeout(check, 50);
    };
    check();
  });
}

(async () => {
  let result;
  try {
    const ready = await waitFor((frame) => frame.type === "ready", 10000);
    bridge.send(JSON.stringify({ type: "ping", id: "smoke-p1" }));
    const pong = await waitFor(
      (frame) => frame.type === "pong" && frame.id === "smoke-p1",
      10000,
    );
    const state = await (async () => {
      bridge.send(JSON.stringify({ type: "get_state", id: "smoke-s1" }));
      return waitFor((frame) => frame.type === "state" && frame.id === "smoke-s1", 10000);
    })();
    result = {
      ok: true,
      protocol: ready.protocol,
      pong_id: pong.id,
      state_model: state.model,
      frames: received.map((frame) => frame.type),
    };
    console.log("[smoke] OK — ready, pong, state diterima lewat jembatan produksi");
  } catch (error) {
    result = { ok: false, error: String(error) };
    console.error("[smoke] GAGAL:", error);
    process.exitCode = 1;
  } finally {
    fs.writeFileSync("/tmp/electron-rpc-smoke.json", JSON.stringify(result, null, 2));
    bridge.kill();
  }
})();
