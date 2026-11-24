// Shell tunggal untuk semua layar (keluarga B referensi: rail 256, header 48,
// footer 40). Satu-satunya tempat grid aplikasi didefinisikan.

import { useEffect, useState, type ReactNode } from "react";
import { useStore } from "../state/store";

export interface ShellProps {
  children: ReactNode;
}

const NAV = [
  { id: "repl", label: "REPL Session", icon: "terminal" },
  { id: "tools", label: "Tool Inspector", icon: "handyman" },
  { id: "mcp", label: "MCP (belum tersedia lewat RPC)", icon: "cable" },
  { id: "settings", label: "Settings & Shortcuts", icon: "settings" },
] as const;

export type NavId = (typeof NAV)[number]["id"];

function Clock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 15_000);
    return () => clearInterval(timer);
  }, []);
  const hh = String(now.getHours()).padStart(2, "0");
  const mm = String(now.getMinutes()).padStart(2, "0");
  return (
    <span className="tabular-nums">
      {hh}:{mm}
    </span>
  );
}

function formatTokens(value: number): string {
  return value.toLocaleString("id-ID");
}

export function Shell({ children }: ShellProps) {
  const { state } = useStore();
  const [active, setActive] = useState<NavId>("repl");
  const [mcpNotice, setMcpNotice] = useState(false);

  const title = state.sessionTitle ?? "sesi saat ini";
  const totalTokens = state.inputTokens + state.outputTokens;
  const percent =
    state.contextWindow && state.contextWindow > 0
      ? Math.min(100, Math.round((totalTokens / state.contextWindow) * 100))
      : null;

  const onNav = (id: NavId) => {
    if (id === "mcp") {
      setMcpNotice(true);
      return;
    }
    setActive(id);
    if (id === "tools") {
      const cards = document.querySelectorAll("[data-tool-card]");
      cards[cards.length - 1]?.scrollIntoView({ block: "center" });
    }
    if (id === "settings") {
      window.dispatchEvent(new CustomEvent("maicli:open-shortcuts"));
    }
    if (id === "repl") {
      document.querySelector<HTMLTextAreaElement>("[data-composer]")?.focus();
    }
  };

  return (
    <div className="shell-grid" style={{ gridTemplateAreas: '"rail header" "rail body" "rail footer"' }}>
      <header
        style={{ gridArea: "header" }}
        className="flex min-w-0 items-center gap-md overflow-hidden border-b border-outline-variant bg-surface-container-low/95 px-md backdrop-blur-md"
      >
        <span className="flex shrink-0 items-center gap-1.5" aria-hidden>
          <span className="h-3 w-3 rounded-full" style={{ background: "#ff5f56" }} />
          <span className="h-3 w-3 rounded-full" style={{ background: "#ffbd2e" }} />
          <span className="h-3 w-3 rounded-full" style={{ background: "#27c93f" }} />
        </span>
        <span className="shrink-0 text-label-sm uppercase tracking-[0.06em] text-outline">
          maicli ::
        </span>
        <span className="min-w-0 flex-1 truncate text-body-md text-on-surface" title={title}>
          {title}
        </span>
        <span className="hidden shrink-0 items-center gap-1 xl:flex">
          <span className="text-label-sm uppercase tracking-[0.06em] text-outline">model:</span>
          <span className="max-w-[16rem] truncate text-body-md text-on-surface">{state.model}</span>
        </span>
        <span className="flex shrink-0 items-center gap-md">
          <span
            className="whitespace-nowrap text-label-sm tracking-[0.06em]"
            style={{ color: state.connected ? "var(--color-tertiary)" : "var(--color-outline)" }}
          >
            {state.connected ? "● CONNECTED" : "○ OFFLINE"}
          </span>
          <span
            className="hidden whitespace-nowrap text-label-sm tracking-[0.06em] md:inline"
            style={{
              color: state.busy ? "var(--color-secondary)" : "var(--color-on-surface-variant)",
            }}
          >
            {state.busy ? "● AGENT_RUNNING" : "○ AGENT_IDLE"}
          </span>
        </span>
      </header>

      <aside
        style={{ gridArea: "rail" }}
        className="flex min-h-0 flex-col overflow-y-auto border-r border-outline-variant bg-surface-container-low"
      >
        <div className="flex flex-col gap-0.5 py-sm">
          <div className="px-md pb-1 text-label-sm uppercase tracking-[0.06em] text-outline">
            NAVIGASI
          </div>
          {NAV.map((item) => {
            const muted = item.id === "mcp";
            const isActive = active === item.id && !muted;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => onNav(item.id)}
                aria-current={isActive ? "page" : undefined}
                className={`rail-item ${isActive ? "rail-item-active" : ""} ${muted ? "rail-item-muted" : ""}`}
              >
                <span className="material-symbols-outlined text-[16px]">{item.icon}</span>
                <span className="truncate">{item.label}</span>
              </button>
            );
          })}
        </div>

        {mcpNotice ? (
          <div className="mx-sm mt-sm">
            <div className="banner banner-neutral items-start">
              <span className="material-symbols-outlined text-[14px]">info</span>
              <span className="flex-1">
                MCP belum tersedia lewat RPC. Status server MCP hanya dikirim sebagai catatan
                (`notice`) oleh inti.
              </span>
              <button
                type="button"
                className="btn btn-ghost px-1 py-0"
                onClick={() => setMcpNotice(false)}
                aria-label="Tutup catatan MCP"
              >
                ✕
              </button>
            </div>
          </div>
        ) : null}

        <div className="mt-auto flex flex-col gap-1 px-md pb-sm text-label-sm text-outline-variant">
          <span className="flex items-center justify-between">
            <span>PROTOKOL</span>
            <span className="text-on-surface-variant">{state.protocol}</span>
          </span>
          <span className="flex items-center justify-between">
            <span>ANTREAN</span>
            <span className="text-on-surface-variant">{state.queued}</span>
          </span>
          <span className="flex items-center justify-between">
            <span>PESAN</span>
            <span className="text-on-surface-variant">{state.messages}</span>
          </span>
        </div>
      </aside>

      <main style={{ gridArea: "body" }} className="min-h-0 overflow-hidden">
        {children}
      </main>

      <footer
        style={{ gridArea: "footer" }}
        className="flex min-w-0 items-center justify-between gap-md overflow-hidden border-t border-outline-variant bg-surface-container-lowest px-md text-label-sm text-on-surface-variant"
      >
        <div className="flex min-w-0 items-center gap-md overflow-hidden">
          <span className="flex shrink-0 items-center gap-1">
            <kbd className="keycap">[?]</kbd> Help
          </span>
          <span className="flex shrink-0 items-center gap-1">
            <kbd className="keycap">[/]</kbd> Commands
          </span>
          <span className="hidden shrink-0 items-center gap-1 lg:flex">
            <kbd className="keycap">[^P]</kbd> Model
          </span>
          <span className="hidden shrink-0 items-center gap-1 lg:flex">
            <kbd className="keycap">[^B]</kbd> Tool Inspector
          </span>
          <span className="hidden shrink-0 items-center gap-1 xl:flex">
            <kbd className="keycap">[Esc]</kbd> Tutup
          </span>
        </div>
        <div className="flex min-w-0 items-center gap-md overflow-hidden">
          <span className="flex shrink-0 items-center gap-1 whitespace-nowrap">
            <span className="text-outline">CTX:</span>
            <span className="ascii-meter hidden text-[11px] sm:inline">
              {percent === null ? "[░░░░░░░░]" : asciiMeter(percent)}
            </span>
            <span className="text-on-surface-variant">{percent === null ? "—" : `${percent}%`}</span>
            <span className="hidden text-outline-variant sm:inline">
              ({formatTokens(totalTokens)}/
              {state.contextWindow === null ? "—" : formatTokens(state.contextWindow)})
            </span>
          </span>
          <span className="hidden min-w-0 truncate text-on-surface-variant lg:inline">
            {state.model}
          </span>
          <Clock />
        </div>
      </footer>
    </div>
  );
}

export function asciiMeter(percent: number, width = 8): string {
  const clamped = Math.max(0, Math.min(100, percent));
  const filled = Math.round((clamped / 100) * width);
  return `[${"█".repeat(filled)}${"░".repeat(width - filled)}]`;
}
