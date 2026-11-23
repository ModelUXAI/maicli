import test from "node:test";
import assert from "node:assert/strict";
import { Client } from "../src/client.mjs";

test("prompt roundtrip", async () => {
  const frames = [
    { type: "ready", protocol: "maicli-rpc/1", model: "mock" },
    { type: "event", event: { event: "agent_start" } },
    { type: "prompt_result", id: "ts-1", status: "ok" },
    { type: "result", id: "ts-1", stop_reason: "stop", turns: 1 },
  ];
  const lines = frames.map((f) => JSON.stringify(f));
  const written = [];
  let events = 0;
  const client = new Client(lines, (line) => written.push(line), {
    onEvent: () => events++,
  });
  const hasil = await client.prompt("halo");
  assert.equal(hasil.stop_reason, "stop");
  assert.equal(events, 1);
  assert.ok(written[0].includes('"message":"halo"'));
});
