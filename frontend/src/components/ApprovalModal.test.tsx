import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ApprovalModal } from "./ApprovalModal";
import { StoreContext, initialState, type AppState } from "../state/store";
import { RpcClient } from "../rpc/client";
import type { RpcTransport } from "../rpc/transport";
import type { RpcFrame } from "../rpc/frames";

class RecordingTransport implements RpcTransport {
  readonly sent: RpcFrame[] = [];
  private listeners = new Set<(line: string) => void>();
  send(line: string): void {
    this.sent.push(JSON.parse(line) as RpcFrame);
  }
  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }
}

function setup(overrides: Partial<AppState> = {}) {
  const transport = new RecordingTransport();
  const client = new RpcClient(transport);
  const dispatch = vi.fn();
  const state: AppState = {
    ...initialState,
    ...overrides,
    approval: {
      id: "ap0",
      tool: "bash",
      tier: "exec",
      subject: "sudo rm -rf /tmp/build-cache",
    },
  };

  render(
    <StoreContext.Provider value={{ state, dispatch, client }}>
      <ApprovalModal />
    </StoreContext.Provider>,
  );

  const approvalFrames = () => transport.sent.filter((frame) => frame.type === "approval");
  return { transport, dispatch, approvalFrames };
}

describe("ApprovalModal", () => {
  it("klik Y mengirim approve sekali", () => {
    const { approvalFrames } = setup();
    fireEvent.click(screen.getByRole("button", { name: /Setuju Sekali/ }));
    expect(approvalFrames()).toEqual([
      expect.objectContaining({ type: "approval", id: "ap0", decision: "once" }),
    ]);
  });

  it("klik N mengirim approve deny", () => {
    const { approvalFrames } = setup();
    fireEvent.click(screen.getByRole("button", { name: /Tolak/ }));
    expect(approvalFrames()).toEqual([
      expect.objectContaining({ type: "approval", id: "ap0", decision: "deny" }),
    ]);
  });

  it("klik S mengirim approve session", () => {
    const { approvalFrames } = setup();
    fireEvent.click(screen.getByRole("button", { name: /Setuju Sesi/ }));
    expect(approvalFrames()).toEqual([
      expect.objectContaining({ type: "approval", id: "ap0", decision: "session" }),
    ]);
  });

  it("tombol Esc mengirim deny", () => {
    const { approvalFrames } = setup();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(approvalFrames()).toEqual([
      expect.objectContaining({ type: "approval", id: "ap0", decision: "deny" }),
    ]);
  });

  it("fokus awal berada di tombol Tolak", () => {
    setup();
    expect(document.activeElement?.textContent ?? "").toContain("Tolak");
  });

  it("tidak menampilkan pilihan always", () => {
    setup();
    expect(screen.queryByText(/always/i)).toBeNull();
    expect(screen.queryByText(/selalu/i)).toBeNull();
  });

  it("tombol tutup X mengirim deny, bukan diam", () => {
    const { approvalFrames } = setup();
    fireEvent.click(screen.getByRole("button", { name: /Tutup dan tolak/ }));
    expect(approvalFrames()[0]).toMatchObject({ decision: "deny" });
  });
});
