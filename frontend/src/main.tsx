import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./app.css";
import { RpcClient, type Frame, type RpcTransport, type SessionItem } from "./rpc";

const DEMO_SESSIONS: SessionItem[] = [
  { title: "Resume Session (current folder)", messages: 12, path: "." },
  { title: "HDD: Gabung disk HDD baru ke SSD", messages: 8 },
  { title: "ENGR: Debug failing engraph MCP search", messages: 21 },
  { title: "Skill modeluxai-design-system terpasang?", messages: 4 },
];

type Msg = { role: "user" | "assistant"; text: string };
type Tool = { name: string; preview: string; ok: boolean };

/** Transport nyata: kirim JSONL ke `maicli --mode rpc` lewat command Tauri. */
class TauriTransport implements RpcTransport {
  private listeners = new Set<(line: string) => void>();
  send(line: string): void {
    void invoke("rpc_send", { line }).catch((err: unknown) => {
      this.emit({ type: "error", code: "E_RPC", message: String(err) });
    });
  }
  onLine(cb: (line: string) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }
  emitRaw(line: string): void {
    this.listeners.forEach((cb) => cb(line));
  }
  emit(frame: Frame): void {
    this.emitRaw(JSON.stringify(frame));
  }
}

function App() {
  const [sessions] = useState(DEMO_SESSIONS);
  const [active, setActive] = useState(0);
  const [messages, setMessages] = useState<Msg[]>([]);
  const [tools, setTools] = useState<Tool[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [model] = useState("deepseek-v4.1-flash");
  const transportRef = useRef<TauriTransport | null>(null);
  const clientRef = useRef<RpcClient | null>(null);
  const logRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const transport = new TauriTransport();
    transportRef.current = transport;
    const client = new RpcClient(transport);
    clientRef.current = client;
    // Dengar balasan RPC dari child process lewat event Tauri.
    void listen<string>("rpc-line", (event) => transport.emitRaw(event.payload));
    client.onFrame = (frame) => {
      const event = frame.event as Frame | undefined;
      if (!event) return;
      if (event.event === "text_delta" && typeof event.text === "string") {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last?.role === "assistant") {
            return [...prev.slice(0, -1), { ...last, text: last.text + event.text as string }];
          }
          return [...prev, { role: "assistant", text: event.text as string }];
        });
      } else if (event.event === "tool_start") {
        setTools((prev) => [
          ...prev,
          { name: String(event.tool_name ?? "tool"), preview: String(event.preview ?? ""), ok: true },
        ]);
      }
    };
    void client.listSessions();
    return () => client.dispose();
  }, []);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [messages, tools]);

  const submit = () => {
    const text = input.trim();
    if (!text || busy) return;
    setInput("");
    setBusy(true);
    setMessages((prev) => [...prev, { role: "user", text }]);
    const transport = transportRef.current;
    if (!transport) return;
    transport.emit({ type: "event", event: { event: "text_delta", text: "maicli GUI siap. Hubungkan core Rust lewat bridge Tauri untuk menjalankan turn.\n" } });
    transport.emit({ type: "event", event: { event: "text_delta", text: `Prompt diterima: ${text}\n` } });
    transport.emit({ type: "result", id: "local", stop_reason: "stop", turns: 1 });
    setBusy(false);
  };

  return (
    <div className="app">
      <header className="topbar">
        <span className="brand">maicli</span>
        <span className="chip">model <b>{model}</b></span>
        <span className="chip">cwd <b>/tmp</b></span>
        <span className="chip"><span className="dot" /> siap</span>
        <span className="spacer" />
        <span className="chip">v0.1.0</span>
      </header>

      <div className="main">
        <aside className="side">
          <div className="section-title">Sesi</div>
          {sessions.map((session, i) => (
            <button
              key={`${session.title}-${i}`}
              className={`session ${i === active ? "active" : ""}`}
              onClick={() => setActive(i)}
            >
              <span className="title">{session.title}</span>
              <span className="meta">
                {session.messages ?? 0} pesan · {session.path ?? "tersimpan"}
              </span>
            </button>
          ))}
          <div className="section-title" style={{ marginTop: 24 }}>Bantuan</div>
          <div className="empty">
            Enter mengirim · Shift+Enter baris baru · klik sesi untuk berpindah.
          </div>
        </aside>

        <section className="center">
          <div className="transcript" ref={logRef}>
            {messages.length === 0 && (
              <div className="welcome">
                <div className="logo">maicli</div>
                <div className="logo-sub">ModeluX — CLI agentic ModeluXAI</div>
                <div className="tip">
                  Tip: pilih sesi di sebelah kiri atau tulis prompt di bawah untuk memulai.
                  Tekan <b>Enter</b> untuk mengirim.
                </div>
                <div className="empty">Versi 0.1.0 · dibangun di atas inti Rust maicli</div>
              </div>
            )}
            {messages.map((msg, i) => (
              <div className={`msg ${msg.role}`} key={i}>
                <span className="who">{msg.role === "user" ? "Anda" : "maicli"}</span>
                {msg.text}
              </div>
            ))}
          </div>
          <div className="composer">
            <textarea
              value={input}
              placeholder="Tulis prompt…"
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  submit();
                }
              }}
            />
            <button className="send" onClick={submit} disabled={busy || !input.trim()}>
              Kirim
            </button>
          </div>
        </section>

        <aside className="right">
          <div className="section-title">Aktivitas tool</div>
          {tools.length === 0 ? (
            <div className="empty">Belum ada tool call pada sesi ini.</div>
          ) : (
            tools.map((tool, i) => (
              <div className="tool" key={i}>
                <span className="name">{tool.name}</span>{" "}
                <span className={tool.ok ? "ok" : "err"}>{tool.ok ? "✓" : "✗"}</span>
                <div>{tool.preview}</div>
              </div>
            ))
          )}
          <div className="section-title" style={{ marginTop: 24 }}>Status</div>
          <div className="empty">
            {busy ? "Turn berjalan…" : "Siap menerima prompt."}
          </div>
        </aside>
      </div>

      <footer className="footer">
        <span><kbd>Enter</kbd> kirim</span>
        <span><kbd>Shift+Enter</kbd> baris baru</span>
        <span><kbd>PgUp/PgDn</kbd> gulir</span>
        <span className="spacer" />
        <span>maicli GUI · Tauri + React + TypeScript</span>
      </footer>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
