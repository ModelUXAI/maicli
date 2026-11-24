import { describe, expect, it } from "vitest";
import {
  ChunkError,
  MAX_REASSEMBLED_FRAME_BYTES,
  decodeChunk,
  isApprovalRequest,
  isReady,
  isState,
  parseFrame,
  type ChunkAccumulator,
} from "./frames";

function base64(text: string): string {
  return btoa(text);
}

describe("parseFrame", () => {
  it("menerima frame ready", () => {
    const frame = parseFrame(
      '{"type":"ready","protocol":"maicli-rpc/1","model":"m","supported_protocol_versions":[1,2],"max_reassembled_frame_bytes":67108864}',
    );
    expect(frame).not.toBeNull();
    expect(frame && isReady(frame)).toBe(true);
  });

  it("menerima frame state", () => {
    const frame = parseFrame(
      '{"type":"state","id":"s1","model":"m","messages":2,"busy":false,"queued":0,"input_tokens":10,"output_tokens":20}',
    );
    expect(frame).not.toBeNull();
    expect(frame && isState(frame)).toBe(true);
  });

  it("menerima frame approval_request", () => {
    const frame = parseFrame(
      '{"type":"approval_request","id":"ap0","tool":"bash","tier":"exec","subject":"ls"}',
    );
    expect(frame).not.toBeNull();
    expect(frame && isApprovalRequest(frame)).toBe(true);
  });

  it("mengembalikan null untuk JSON rusak, baris kosong, dan non-objek", () => {
    expect(parseFrame("{bukan json")).toBeNull();
    expect(parseFrame("   ")).toBeNull();
    expect(parseFrame("[1,2,3]")).toBeNull();
    expect(parseFrame('"teks"')).toBeNull();
    expect(parseFrame('{"id":"x"}')).toBeNull();
  });
});

describe("decodeChunk", () => {
  const payload = '{"type":"error","code":"E_X","message":"halo dunia"}';
  const bytes = new TextEncoder().encode(payload);
  const third = Math.ceil(bytes.length / 3);
  const parts = [
    bytes.slice(0, third),
    bytes.slice(third, third * 2),
    bytes.slice(third * 2),
  ];

  function chunkFrame(index: number, data: string, count = 3) {
    return {
      type: "rpc_chunk",
      chunk_id: "c1",
      index,
      count,
      byte_length: bytes.length,
      data,
    };
  }

  it("menyatukan tiga chunk berurutan menjadi frame logis", () => {
    const acc: ChunkAccumulator = new Map();
    expect(decodeChunk(acc, chunkFrame(0, base64(String.fromCharCode(...parts[0]))))).toBeNull();
    expect(decodeChunk(acc, chunkFrame(1, base64(String.fromCharCode(...parts[1]))))).toBeNull();
    const done = decodeChunk(acc, chunkFrame(2, base64(String.fromCharCode(...parts[2]))));
    expect(done).toBe(payload);
    expect(acc.size).toBe(0);
  });

  it("menerima chunk tidak berurutan", () => {
    const acc: ChunkAccumulator = new Map();
    expect(decodeChunk(acc, chunkFrame(2, base64(String.fromCharCode(...parts[2]))))).toBeNull();
    expect(decodeChunk(acc, chunkFrame(0, base64(String.fromCharCode(...parts[0]))))).toBeNull();
    expect(decodeChunk(acc, chunkFrame(1, base64(String.fromCharCode(...parts[1]))))).toBe(payload);
  });

  it("menolak index duplikat", () => {
    const acc: ChunkAccumulator = new Map();
    decodeChunk(acc, chunkFrame(0, base64(String.fromCharCode(...parts[0]))));
    expect(() =>
      decodeChunk(acc, chunkFrame(0, base64(String.fromCharCode(...parts[0])))),
    ).toThrowError(ChunkError);
    expect(acc.size).toBe(0);
  });

  it("menolak count yang berubah di tengah reassembly", () => {
    const acc: ChunkAccumulator = new Map();
    decodeChunk(acc, chunkFrame(0, base64(String.fromCharCode(...parts[0]))));
    expect(() =>
      decodeChunk(acc, chunkFrame(1, base64(String.fromCharCode(...parts[1])), 4)),
    ).toThrowError(/metadata berubah/);
  });

  it("menolak frame di atas batas ukuran", () => {
    const acc: ChunkAccumulator = new Map();
    expect(() =>
      decodeChunk(acc, {
        type: "rpc_chunk",
        chunk_id: "big",
        index: 0,
        count: 1,
        byte_length: MAX_REASSEMBLED_FRAME_BYTES + 1,
        data: "",
      }),
    ).toThrowError(/melebihi batas/);
  });

  it("menolak hasil yang bukan objek JSON", () => {
    const acc: ChunkAccumulator = new Map();
    const raw = btoa("[1,2,3]");
    expect(() =>
      decodeChunk(acc, {
        type: "rpc_chunk",
        chunk_id: "bad",
        index: 0,
        count: 1,
        byte_length: 7,
        data: raw,
      }),
    ).toThrowError(/bukan objek/);
  });
});
