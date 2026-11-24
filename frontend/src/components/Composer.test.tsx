import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { Composer } from "./Composer";
import { StoreContext, initialState, reducer, type AppState } from "../state/store";
import { RpcClient } from "../rpc/client";
import type { RpcTransport } from "../rpc/transport";

class RecordingTransport implements RpcTransport {
  readonly sent: unknown[] = [];
  private listeners = new Set<(line: string) => void>();
  send(line: string): void {
    this.sent.push(JSON.parse(line) as unknown);
  }
  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }
  deliver(frame: unknown): void {
    const line = JSON.stringify(frame);
    this.listeners.forEach((cb) => cb(line));
  }
}

function setup(state: AppState = initialState) {
  const transport = new RecordingTransport();
  const client = new RpcClient(transport);
  const onSlash = vi.fn();
  const onOpenModel = vi.fn();
  const onOpenPalette = vi.fn();

  render(
    <StoreContext.Provider value={{ state, dispatch: () => undefined, client }}>
      <Composer onSlash={onSlash} onOpenModel={onOpenModel} onOpenPalette={onOpenPalette} />
    </StoreContext.Provider>,
  );

  const textarea = screen.getByLabelText("Prompt");
  return { transport, client, textarea, onSlash, onOpenModel, onOpenPalette };
}

describe("Composer", () => {
  it("Enter mengirim prompt sekali dengan teks trim", () => {
    const { transport, textarea } = setup();
    fireEvent.change(textarea, { target: { value: "  halo dunia  " } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: false });

    const prompts = transport.sent.filter(
      (frame) => (frame as { type: string }).type === "prompt",
    );
    expect(prompts).toHaveLength(1);
    expect(prompts[0]).toMatchObject({ type: "prompt", message: "halo dunia" });
  });

  it("Shift+Enter tidak mengirim prompt", () => {
    const { transport, textarea } = setup();
    fireEvent.change(textarea, { target: { value: "baris satu" } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });
    expect(
      transport.sent.filter((frame) => (frame as { type: string }).type === "prompt"),
    ).toHaveLength(0);
  });

  it("Enter saat busy tidak mengirim prompt", () => {
    const busyState: AppState = reducer(initialState, { type: "prompt_sent", text: "sedang" });
    const { transport, textarea } = setup(busyState);
    fireEvent.change(textarea, { target: { value: "halo" } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: false });
    expect(
      transport.sent.filter((frame) => (frame as { type: string }).type === "prompt"),
    ).toHaveLength(0);
    expect(screen.getByText(/turn berjalan/)).toBeInTheDocument();
  });

  it("tombol Kirim nonaktif saat input kosong", () => {
    setup();
    expect(screen.getByRole("button", { name: /Kirim/ })).toBeDisabled();
  });
});
