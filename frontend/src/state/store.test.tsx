import { describe, expect, it } from "vitest";
import { actionsForFrame, initialState, reducer, type AppState } from "./store";
import type { RpcFrame } from "../rpc/frames";

function apply(state: AppState, frames: RpcFrame[]): AppState {
  return frames.flatMap(actionsForFrame).reduce(reducer, state);
}

const delta = (text: string): RpcFrame => ({
  type: "event",
  event: { event: "text_delta", text },
});

describe("reducer", () => {
  it("menggabungkan tiga text_delta ke satu entri assistant", () => {
    const state = apply(initialState, [delta("Halo "), delta("dunia "), delta("!")]);
    expect(state.turn).toHaveLength(1);
    expect(state.turn[0]).toEqual({ kind: "assistant", text: "Halo dunia !" });
  });

  it("memisahkan assistant sebelumnya bila user mengirim prompt baru", () => {
    let state = apply(initialState, [delta("awal")]);
    state = reducer(state, { type: "prompt_sent", text: "lanjut" });
    state = apply(state, [delta("jawaban")]);
    expect(state.turn.map((entry) => entry.kind)).toEqual(["assistant", "user", "assistant"]);
  });

  it("tool_end dengan id tak dikenal tidak membuat entri yatim", () => {
    const state = apply(initialState, [
      { type: "event", event: { event: "tool_end", tool_call_id: "ghost", tool_name: "x", is_error: false, preview: "" } },
    ]);
    expect(state.tools).toHaveLength(0);
  });

  it("mencocokkan tool_end ke tool_start lewat tool_call_id", () => {
    let state = apply(initialState, [
      { type: "event", event: { event: "tool_start", tool_call_id: "c1", tool_name: "echo", preview: '{"n":1}' } },
    ]);
    expect(state.tools).toHaveLength(1);
    expect(state.tools[0].endedAt).toBeUndefined();
    state = apply(state, [
      { type: "event", event: { event: "tool_end", tool_call_id: "c1", tool_name: "echo", is_error: true, preview: "gagal" } },
    ]);
    expect(state.tools).toHaveLength(1);
    expect(state.tools[0].isError).toBe(true);
    expect(state.tools[0].endedAt).toBeTypeOf("number");
  });

  it("approval_request lalu approval_closed mengosongkan modal", () => {
    let state = apply(initialState, [
      { type: "approval_request", id: "ap0", tool: "bash", tier: "exec", subject: "rm -rf /tmp/x" },
    ]);
    expect(state.approval).toEqual({ id: "ap0", tool: "bash", tier: "exec", subject: "rm -rf /tmp/x" });
    state = reducer(state, { type: "approval_closed" });
    expect(state.approval).toBeNull();
  });

  it("approval baru menimpa approval yang sedang aktif", () => {
    let state = apply(initialState, [
      { type: "approval_request", id: "a1", tool: "read", tier: "read", subject: "x" },
      { type: "approval_request", id: "a2", tool: "bash", tier: "exec", subject: "y" },
    ]);
    expect(state.approval?.id).toBe("a2");
  });

  it("prompt_result menurunkan busy", () => {
    let state = reducer(initialState, { type: "prompt_sent", text: "halo" });
    expect(state.busy).toBe(true);
    state = apply(state, [
      { type: "prompt_result", id: "p1", agent_invoked: true, status: "ok", stop_reason: "stop", turns: 1 },
    ]);
    expect(state.busy).toBe(false);
  });

  it("session_settled juga menurunkan busy", () => {
    let state = reducer(initialState, { type: "prompt_sent", text: "halo" });
    state = apply(state, [{ type: "session_settled" }]);
    expect(state.busy).toBe(false);
  });

  it("state dan usage mengisi angka dari frame, bukan mengarang", () => {
    const state = apply(initialState, [
      {
        type: "state",
        id: "s1",
        model: "m",
        messages: 3,
        busy: false,
        queued: 1,
        input_tokens: 100,
        output_tokens: 50,
      },
      { type: "usage", id: "u1", input_tokens: 111, output_tokens: 55, cached_tokens: 9 },
    ]);
    expect(state.messages).toBe(3);
    expect(state.queued).toBe(1);
    expect(state.inputTokens).toBe(111);
    expect(state.outputTokens).toBe(55);
    expect(state.cachedTokens).toBe(9);
    expect(state.contextWindow).toBeNull();
  });

  it("context_window hanya diisi bila server mengirimnya", () => {
    const state = apply(initialState, [
      {
        type: "state",
        id: "s1",
        model: "m",
        messages: 0,
        busy: false,
        queued: 0,
        input_tokens: 0,
        output_tokens: 0,
        context_window: 262144,
      },
    ]);
    expect(state.contextWindow).toBe(262144);
  });

  it("error menyimpan pesan untuk banner", () => {
    const state = apply(initialState, [{ type: "error", code: "E_MODEL_UNKNOWN", message: "tidak ada" }]);
    expect(state.error).toBe("E_MODEL_UNKNOWN: tidak ada");
  });

  it("prompt_result error mengisi error dan menurunkan busy", () => {
    let state = reducer(initialState, { type: "prompt_sent", text: "x" });
    state = apply(state, [
      { type: "prompt_result", id: "p1", agent_invoked: true, status: "error", error: "provider mati" },
    ]);
    expect(state.busy).toBe(false);
    expect(state.error).toBe("provider mati");
  });

  it("model hanya berubah lewat session_info_update", () => {
    const state = apply(initialState, [
      { type: "session_info_update", session_id: "sess-1", model: "model-baru", title: "Judul" },
    ]);
    expect(state.model).toBe("model-baru");
    expect(state.sessionId).toBe("sess-1");
    expect(state.sessionTitle).toBe("Judul");
  });
});
