// Klien RPC maicli v1 — JavaScript ESM + deklarasi TypeScript di client.d.ts.
// Tanpa dependensi: cukup Node >= 18.

/** Iterasi baris balasan; `write` mengirim satu baris permintaan. */
export class Client {
  constructor(lines, write, options = {}) {
    this.reader = lines[Symbol.asyncIterator]
      ? lines[Symbol.asyncIterator]()
      : lines[Symbol.iterator]();
    this.write = write;
    this.options = options;
    this.next = 0;
  }

  id(prefix) {
    this.next += 1;
    return `${prefix}-${this.next}`;
  }

  send(value) {
    this.write(JSON.stringify(value));
  }

  async recv() {
    for (;;) {
      const { value, done } = await this.reader.next();
      if (done) throw new Error("maicli menutup kanal");
      const baris = String(value).trim();
      if (baris) return JSON.parse(baris);
    }
  }

  async prompt(message) {
    const ident = this.id("ts");
    this.send({ type: "prompt", id: ident, message });
    for (;;) {
      const frame = await this.recv();
      if (frame.type === "question") {
        const jawaban = this.options.onQuestion?.(frame.id, frame.question ?? "") ?? "";
        this.send({ type: "answer", id: frame.id, text: jawaban });
      } else if (frame.type === "approval_request") {
        const keputusan =
          this.options.onApproval?.(frame.id, frame.tool ?? "", frame.tier ?? "", frame.subject ?? "") ?? "deny";
        this.send({ type: "approval", id: frame.id, decision: keputusan });
      } else if (frame.type === "event") {
        this.options.onEvent?.(frame);
      } else if (frame.type === "result" && frame.id === ident) {
        return frame;
      } else if (frame.type === "error" && frame.id === ident) {
        throw new Error(frame.message ?? "galat maicli");
      }
    }
  }

  async ping() {
    const ident = this.id("ts");
    this.send({ type: "ping", id: ident });
    for (;;) {
      const frame = await this.recv();
      if (frame.type === "pong" && frame.id === ident) return frame;
    }
  }

  shutdown() {
    this.send({ type: "shutdown" });
  }
}
