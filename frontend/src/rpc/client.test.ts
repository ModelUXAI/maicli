import { describe, expect, it } from "vitest";
import { RpcClient } from "./client";
import type { RpcFrame } from "./frames";
import type { RpcTransport } from "./transport";

class MemoryTransport implements RpcTransport {
  readonly sent: RpcFrame[] = [];
  private listeners = new Set<(line: string) => void>();

  send(line: string): void {
    this.sent.push(JSON.parse(line) as RpcFrame);
  }

  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  deliver(frame: RpcFrame): void {
    const line = JSON.stringify(frame);
    this.listeners.forEach((cb) => cb(line));
  }

  last(): RpcFrame | undefined {
    return this.sent[this.sent.length - 1];
  }
}

describe("RpcClient", () => {
  it("mengorelasikan pong lewat id", async () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const promise = client.ping();
    const sent = transport.last();
    expect(sent?.type).toBe("ping");
    transport.deliver({ type: "pong", id: sent?.id as string });
    await expect(promise).resolves.toMatchObject({ type: "pong" });
  });

  it("menolak request dengan error ber-id", async () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const promise = client.setModel("tidak-ada");
    const sent = transport.last();
    transport.deliver({
      type: "error",
      id: sent?.id as string,
      code: "E_MODEL_UNKNOWN",
      message: "model tidak dikenal",
    });
    await expect(promise).rejects.toMatchObject({ code: "E_MODEL_UNKNOWN" });
  });

  it("mengumpulkan command_output tanpa id lalu menyelesaikan pada ack", async () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const promise = client.command("/model");
    const sent = transport.last();
    // Server mengirim tepat satu `command_output` (tanpa id) lalu `ack`.
    transport.deliver({ type: "command_output", text: "model aktif: m1" });
    transport.deliver({ type: "ack", id: sent?.id as string });
    const frame = await promise;
    expect(frame.type).toBe("command_output");
    expect(frame.text).toBe("model aktif: m1");
  });

  it("dua command bersamaan tidak saling menukar output", async () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);

    const first = client.command("/model");
    const firstId = transport.last()?.id as string;
    const second = client.command("/settings");
    const secondId = transport.last()?.id as string;

    transport.deliver({ type: "command_output", text: "output model" });
    transport.deliver({ type: "ack", id: firstId });
    transport.deliver({ type: "command_output", text: "output settings" });
    transport.deliver({ type: "ack", id: secondId });

    await expect(first).resolves.toMatchObject({ text: "output model" });
    await expect(second).resolves.toMatchObject({ text: "output settings" });
  });

  it("negosiasi v2 sekali saat ready mendukung versi 2", () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const frame: RpcFrame = {
      type: "ready",
      protocol: "maicli-rpc/1",
      model: "m",
      supported_protocol_versions: [1, 2],
      max_reassembled_frame_bytes: 67108864,
    };
    transport.deliver(frame);
    transport.deliver(frame);
    const negotiations = transport.sent.filter((item) => item.type === "negotiate_protocol");
    expect(negotiations).toHaveLength(1);
    expect(negotiations[0]).toMatchObject({ protocol_version: 2 });
    expect(client).toBeDefined();
  });

  it("tidak bernegosiasi bila versi 2 tidak didukung", () => {
    const transport = new MemoryTransport();
    new RpcClient(transport);
    transport.deliver({
      type: "ready",
      protocol: "maicli-rpc/1",
      model: "m",
      supported_protocol_versions: [1],
      max_reassembled_frame_bytes: 67108864,
    });
    expect(transport.sent.filter((item) => item.type === "negotiate_protocol")).toHaveLength(0);
  });

  it("menyusun ulang frame yang dipecah rpc_chunk sebelum diparse", () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const frames: RpcFrame[] = [];
    client.onFrame = (frame) => frames.push(frame);

    const inner = JSON.stringify({ type: "usage", id: "u9", input_tokens: 1, output_tokens: 2, cached_tokens: 3 });
    const bytes = new TextEncoder().encode(inner);
    const half = Math.ceil(bytes.length / 2);
    const part = (slice: Uint8Array) => btoa(String.fromCharCode(...slice));

    transport.deliver({
      type: "rpc_chunk",
      chunk_id: "x",
      index: 0,
      count: 2,
      byte_length: bytes.length,
      data: part(bytes.slice(0, half)),
    });
    expect(frames).toHaveLength(0);
    transport.deliver({
      type: "rpc_chunk",
      chunk_id: "x",
      index: 1,
      count: 2,
      byte_length: bytes.length,
      data: part(bytes.slice(half)),
    });
    expect(frames).toHaveLength(1);
    expect(frames[0]).toMatchObject({ type: "usage", input_tokens: 1 });
  });

  it("chunk cacat menjadi frame error, bukan melempar", () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const frames: RpcFrame[] = [];
    client.onFrame = (frame) => frames.push(frame);
    transport.deliver({
      type: "rpc_chunk",
      chunk_id: "y",
      index: 1,
      count: 1,
      byte_length: 3,
      data: "e30=",
    });
    expect(frames[0]).toMatchObject({ type: "error" });
  });

  it("frame rusak diabaikan tanpa menjatuhkan klien", () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const frames: RpcFrame[] = [];
    client.onFrame = (frame) => frames.push(frame);
    transport.deliver({ type: "pong", id: "x" });
    const listeners = (transport as unknown as { listeners: Set<(line: string) => void> }).listeners;
    listeners.forEach((cb) => cb("{bukan json"));
    expect(frames).toHaveLength(1);
    expect(client).toBeDefined();
  });

  it("dispose menolak pending dengan E_DISPOSED", async () => {
    const transport = new MemoryTransport();
    const client = new RpcClient(transport);
    const promise = client.getState();
    client.dispose();
    await expect(promise).rejects.toMatchObject({ code: "E_DISPOSED" });
  });
});
