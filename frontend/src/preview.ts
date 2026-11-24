// Preview DEV. Hanya dimuat lewat dynamic import saat
// `import.meta.env.DEV && location.search` punya `preview`, jadi tidak pernah
// masuk bundle produksi. Frame di sini adalah hasil konversi transkrip golden
// `TUI/tests/golden/*.txt` + frame status buatan untuk verifikasi visual.

import { RpcClient } from "./rpc/client";
import { parseFrame, type RpcFrame } from "./rpc/frames";
import type { RpcTransport } from "./rpc/transport";

type Scenario = "main" | "welcome" | "approval" | "ask";

function event(payload: Record<string, unknown>): RpcFrame {
  return { type: "event", event: payload };
}

/**
 * Konversi `tests/golden/dua-tool.txt` + `tool-diizinkan.txt` ke frame RPC.
 * Baris golden: `tool_start:echo:{"n":1}` → `tool_start` dengan preview JSON.
 */
const TRANSCRIPT: RpcFrame[] = [
  { type: "ready", protocol: "maicli-rpc/1", model: "deepseek-v4.1-flash", supported_protocol_versions: [1], max_reassembled_frame_bytes: 67108864 },
  event({ event: "agent_start" }),
  { type: "state", id: "s1", model: "deepseek-v4.1-flash", messages: 4, busy: false, queued: 0, input_tokens: 18432, output_tokens: 3072 },
  { type: "usage", id: "u1", input_tokens: 18432, output_tokens: 3072, cached_tokens: 12800 },
  event({ event: "turn_start", index: 1 }),
  event({ event: "message_start" }),
  event({
    event: "text_delta",
    text: "Saya cek dulu dua berkas konfigurasi lalu jalankan test yang relevan.\n",
  }),
  event({ event: "text_delta", text: "Rencana: baca Cargo.toml, jalankan cargo test --workspace.\n" }),
  event({ event: "message_start" }),
  event({ event: "tool_start", tool_call_id: "c1", tool_name: "echo", preview: '{"n":1}' }),
  event({ event: "tool_end", tool_call_id: "c1", tool_name: "echo", is_error: false, preview: '{"n":1}' }),
  event({ event: "tool_start", tool_call_id: "c2", tool_name: "echo", preview: '{"n":2}' }),
  event({ event: "tool_end", tool_call_id: "c2", tool_name: "echo", is_error: false, preview: '{"n":2}' }),
  event({
    event: "tool_start",
    tool_call_id: "c3",
    tool_name: "apply_patch",
    preview:
      "diff --git a/TUI/frontend/src/app.css b/TUI/frontend/src/app.css\nindex 3f2a1c9..9b1d4e0 100644\n--- a/TUI/frontend/src/app.css\n+++ b/TUI/frontend/src/app.css\n@@ -1,6 +1,7 @@\n-:root {\n-  --accent: #d7a05b;\n+:root {\n+  --color-primary: #a2c9ff;\n+  --color-tertiary: #6fdd78;\n   --radius: 10px;\n }\n@@ -18,3 +19,4 @@\n .chip {\n-  border-radius: 999px;\n+  border-radius: 12px;\n }\n+/* token mengikuti code.html referensi */\n",
  }),
  event({
    event: "tool_end",
    tool_call_id: "c3",
    tool_name: "apply_patch",
    is_error: false,
    preview: "applied 2 hunks",
  }),
  event({ event: "text_delta", text: "\nSelesai: dua tool echo berhasil, patch diterapkan.\n" }),
  event({ event: "turn_end", index: 1 }),
  event({ event: "agent_end", turns: 1 }),
  { type: "prompt_result", id: "p1", agent_invoked: true, status: "ok", stop_reason: "stop", turns: 1 },
  { type: "session_settled" },
];

const APPROVAL: RpcFrame[] = [
  {
    type: "approval_request",
    id: "ap0",
    tool: "bash",
    tier: "exec",
    subject: "sudo systemctl enable --now agent-worker.service",
  },
];

const ASK: RpcFrame[] = [
  {
    type: "ask_dialog",
    id: "ask-1",
    question: "Rename `main.tsx` menjadi `App.tsx`?",
    options: ["Ya, rename", "Biarkan", "Tampilkan diff dulu"],
    recommended: 0,
    timeout_secs: 60,
  },
];

interface FixtureTransportOptions {
  scenario: Scenario;
}

/** Angka status awal per skenario supaya UI tidak menampilkan pesan yang tidak ada. */
function initialStateFor(scenario: Scenario): { messages: number; input: number; output: number; cached: number } {
  if (scenario === "welcome" || scenario === "ask") {
    return { messages: 0, input: 0, output: 0, cached: 0 };
  }
  return { messages: 4, input: 18432, output: 3072, cached: 12800 };
}

class FixtureTransport implements RpcTransport {
  private listeners = new Set<(line: string) => void>();
  private queue: RpcFrame[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private readonly counters: ReturnType<typeof initialStateFor>;

  constructor(options: FixtureTransportOptions) {
    this.counters = initialStateFor(options.scenario);
    const initial: RpcFrame[] = [
      { type: "ready", protocol: "maicli-rpc/2", model: "deepseek-v4.1-flash", supported_protocol_versions: [1, 2], max_reassembled_frame_bytes: 67108864 },
      { type: "state", id: "s0", model: "deepseek-v4.1-flash", messages: this.counters.messages, busy: false, queued: 0, input_tokens: this.counters.input, output_tokens: this.counters.output },
      { type: "usage", id: "u0", input_tokens: this.counters.input, output_tokens: this.counters.output, cached_tokens: this.counters.cached },
    ];
    this.queue.push(...initial);
    if (options.scenario === "main") this.queue.push(...TRANSCRIPT);
    if (options.scenario === "approval") this.queue.push(...TRANSCRIPT.slice(0, 8), ...APPROVAL);
    if (options.scenario === "ask") this.queue.push(...ASK);
    this.drain();
  }

  private drain(): void {
    if (this.timer) return;
    const tick = () => {
      const next = this.queue.shift();
      if (!next) {
        this.timer = null;
        return;
      }
      const line = JSON.stringify(next);
      this.listeners.forEach((cb) => cb(line));
      this.timer = setTimeout(tick, 60);
    };
    this.timer = setTimeout(tick, 40);
  }

  send(line: string): void {
    const frame = parseFrame(line);
    if (!frame) return;
    const id = typeof frame.id === "string" ? frame.id : undefined;
    switch (frame.type) {
      case "ping":
        if (id) this.enqueue({ type: "pong", id });
        break;
      case "get_state":
        if (id)
          this.enqueue({
            type: "state",
            id,
            model: "deepseek-v4.1-flash",
            messages: this.counters.messages,
            busy: false,
            queued: 0,
            input_tokens: this.counters.input,
            output_tokens: this.counters.output,
          });
        break;
      case "usage":
        if (id)
          this.enqueue({
            type: "usage",
            id,
            input_tokens: this.counters.input,
            output_tokens: this.counters.output,
            cached_tokens: this.counters.cached,
          });
        break;
      case "command": {
        // Urutan nyata dari server: command_output (tanpa id) lalu ack.
        const text = typeof frame.text === "string" ? frame.text : "";
        const body =
          text.includes("model")
            ? "model aktif: deepseek-v4.1-flash\ntersedia: deepseek-v4.1-flash, qwen3-coder-plus, glm-4.6"
            : text.includes("settings")
              ? "approval.mode = always-ask\nmodels.default = deepseek-v4.1-flash\ncontext_window_tokens = 262144"
              : `output untuk ${text}`;
        this.enqueue({ type: "command_output", text: body });
        if (id) this.enqueue({ type: "ack", id });
        break;
      }
      case "model":
        if (id)
          this.enqueue(
            { type: "session_info_update", session_id: "sess-0x8f94", model: String(frame.model ?? ""), title: "Resume Session (current folder)" },
            { type: "ack", id },
          );
        break;
      case "prompt": {
        if (id) {
          this.enqueue({ type: "ack", id });
          this.enqueue(
            event({ event: "text_delta", text: "Preview: permintaan dikirim ke core.\n" }),
          );
          this.enqueue({ type: "prompt_result", id, agent_invoked: true, status: "ok", stop_reason: "stop", turns: 1 });
          this.enqueue({ type: "session_settled" });
        }
        break;
      }
      default:
        break;
    }
  }

  private enqueue(...frames: RpcFrame[]): void {
    this.queue.push(...frames);
    this.drain();
  }

  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }
}

export function createPreviewClient(search: URLSearchParams): RpcClient {
  const raw = search.get("preview");
  const scenario: Scenario =
    raw === "welcome" || raw === "approval" || raw === "ask" ? raw : "main";
  return new RpcClient(new FixtureTransport({ scenario }));
}

export type { Scenario };
