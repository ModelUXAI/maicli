// Layar kosong (referensi 01) tanpa data rekaan: hanya field yang datang dari
// frame RPC, sisanya labelnya tidak ditampilkan sama sekali.

import pkg from "../../package.json";
import { useStore } from "../state/store";
import { asciiMeter } from "./Shell";

export interface WelcomeViewProps {
  onQuickstart: (index: number) => void;
}

function logoLines(): { text: string; color: string }[] {
  return [
    { text: "███╗   ███╗ █████╗ ██╗ ██████╗██╗     ██╗", color: "var(--color-secondary)" },
    { text: "████╗ ████║██╔══██╗██║██╔════╝██║     ██║", color: "var(--color-primary)" },
    { text: "██╔████╔██║███████║██║██║     ██║     ██║", color: "var(--color-primary)" },
    { text: "██║╚██╔╝██║██╔══██║██║██║     ██║     ██║", color: "var(--color-tertiary)" },
    { text: "██║ ╚═╝ ██║██║  ██║██║╚██████╗███████╗██║", color: "var(--color-tertiary)" },
    { text: "╚═╝     ╚═╝╚═╝  ╚═╝╚═╝ ╚═════╝╚══════╝╚═╝", color: "var(--color-tertiary)" },
  ];
}

export function WelcomeView({ onQuickstart }: WelcomeViewProps) {
  const { state } = useStore();
  const total = state.inputTokens + state.outputTokens;
  const percent =
    state.contextWindow && state.contextWindow > 0
      ? Math.min(100, Math.round((total / state.contextWindow) * 100))
      : null;

  return (
    <div className="flex w-full select-none flex-col gap-sm">
      <div className="flex flex-wrap items-center justify-between gap-sm rounded-t bg-surface-container-lowest px-md py-xs text-label-sm text-on-surface-variant">
        <div className="flex flex-wrap items-center gap-sm">
          <span className="flex items-center gap-xs" style={{ color: state.connected ? "var(--color-tertiary)" : "var(--color-outline)" }}>
            <span
              className="inline-block h-1.5 w-1.5 rounded-full"
              style={{ background: state.connected ? "var(--color-tertiary)" : "var(--color-outline)" }}
            />
            {state.connected ? "RUNTIME_ONLINE" : "OFFLINE"}
          </span>
          <span className="text-outline-variant">::</span>
          <span className="text-primary">PROTOCOL: {state.protocol}</span>
          <span className="text-outline-variant">::</span>
          <span className="text-secondary">MODEL: {state.model}</span>
          <span className="text-outline-variant">::</span>
          <span>SESSION: {state.sessionId ?? "—"}</span>
          <span className="text-outline-variant">::</span>
          <span>MESSAGES: {state.messages}</span>
        </div>
      </div>

      <div className="flex flex-col justify-between gap-md rounded bg-surface-container-low p-lg lg:flex-row lg:items-center">
        <div className="flex min-w-0 flex-col">
          <pre className="select-none overflow-x-auto font-mono text-[11px] font-bold leading-[1.05] tracking-tighter sm:text-[13px]">
            {logoLines().map((line) => (
              <div key={line.text} style={{ color: line.color }}>
                {line.text}
              </div>
            ))}
          </pre>
          <div className="mt-sm flex flex-wrap items-center gap-sm">
            <span className="rounded bg-secondary-container px-xs py-0.5 text-label-sm font-semibold tracking-wider text-on-secondary-container">
              maicli v{pkg.version}
            </span>
            <span className="text-headline-sm font-semibold text-on-surface">
              Antarmuka desktop agentic ModeluXAI
            </span>
          </div>
          <p className="mt-1 text-body-sm text-on-surface-variant">
            Frontend Tauri 2 di atas inti Rust yang sama; semua status berasal dari frame RPC.
          </p>
        </div>

        <div className="flex min-w-[260px] flex-col gap-xs rounded bg-surface-container p-md">
          <div className="flex items-center justify-between text-label-sm">
            <span className="flex items-center gap-1 text-on-surface-variant">
              <span className="material-symbols-outlined text-[14px] text-primary">memory</span>
              CONTEXT
            </span>
            <span className="text-outline">{percent === null ? "—" : `${percent}%`}</span>
          </div>
          <div className="h-2 w-full overflow-hidden rounded bg-surface-container-highest">
            <div className="h-full bg-primary-container" style={{ width: `${percent ?? 0}%` }} />
          </div>
          <div className="flex items-center justify-between font-mono text-label-sm text-on-surface-variant">
            <span>{total.toLocaleString("id-ID")}</span>
            <span className="text-outline">
              Max: {state.contextWindow === null ? "—" : state.contextWindow.toLocaleString("id-ID")}
            </span>
          </div>
          <div className="ascii-meter text-[11px]">
            {percent === null ? "[░░░░░░░░]" : asciiMeter(percent)}
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-md md:grid-cols-3">
        <div className="flex flex-col justify-between rounded bg-surface-container-low p-md">
          <div className="mb-sm flex items-center justify-between">
            <span className="flex items-center gap-xs text-headline-sm font-semibold text-primary">
              <span className="material-symbols-outlined text-[18px]">dns</span>
              RPC
            </span>
            <span
              className="rounded px-1.5 py-0.5 font-mono text-label-sm font-bold"
              style={{
                background: state.connected ? "var(--color-tertiary-container)" : "var(--color-surface-container-high)",
                color: state.connected ? "var(--color-surface)" : "var(--color-outline)",
              }}
            >
              {state.connected ? "CONNECTED" : "OFFLINE"}
            </span>
          </div>
          <div className="flex flex-col gap-0.5 font-mono text-body-sm">
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">protokol</span>
              <span className="text-on-surface-variant">{state.protocol}</span>
            </div>
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">antrean</span>
              <span className="text-on-surface-variant">{state.queued}</span>
            </div>
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">token</span>
              <span className="text-on-surface-variant">{total.toLocaleString("id-ID")}</span>
            </div>
          </div>
        </div>

        <div className="flex flex-col justify-between rounded bg-surface-container-low p-md">
          <div className="mb-sm flex items-center justify-between">
            <span className="flex items-center gap-xs text-headline-sm font-semibold text-secondary">
              <span className="material-symbols-outlined text-[18px]">psychology</span>
              Model
            </span>
            <span className="rounded bg-surface-container-high px-1.5 py-0.5 font-mono text-label-sm text-on-surface-variant">
              aktif
            </span>
          </div>
          <div className="flex flex-col gap-0.5 font-mono text-body-sm">
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">id</span>
              <span className="truncate text-on-surface-variant">{state.model}</span>
            </div>
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">input</span>
              <span className="text-on-surface-variant">{state.inputTokens.toLocaleString("id-ID")}</span>
            </div>
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">output</span>
              <span className="text-on-surface-variant">{state.outputTokens.toLocaleString("id-ID")}</span>
            </div>
          </div>
        </div>

        <div className="flex flex-col justify-between rounded bg-surface-container-low p-md">
          <div className="mb-sm flex items-center justify-between">
            <span className="flex items-center gap-xs text-headline-sm font-semibold text-tertiary">
              <span className="material-symbols-outlined text-[18px]">description</span>
              Sesi
            </span>
            <span className="rounded bg-surface-container-high px-1.5 py-0.5 font-mono text-label-sm text-on-surface-variant">
              {state.messages} pesan
            </span>
          </div>
          <div className="flex flex-col gap-0.5 font-mono text-body-sm">
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">judul</span>
              <span className="truncate text-on-surface-variant">{state.sessionTitle ?? "—"}</span>
            </div>
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">id</span>
              <span className="truncate text-on-surface-variant">{state.sessionId ?? "—"}</span>
            </div>
            <div className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5">
              <span className="text-on-surface">status</span>
              <span className="text-on-surface-variant">{state.busy ? "RUNNING" : "IDLE"}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="rounded bg-surface-container-low p-md">
        <div className="mb-sm flex items-center justify-between">
          <span className="flex items-center gap-xs text-headline-sm font-semibold text-on-surface">
            <span className="material-symbols-outlined text-[18px] text-primary">keyboard_command_key</span>
            Quickstart Dispatcher
          </span>
          <span className="text-label-sm text-outline">Gunakan [1-4] atau klik</span>
        </div>
        <div className="flex flex-col gap-0.5 font-mono text-body-sm">
          {[
            { label: "Prompt baru", hint: "Fokus composer" },
            { label: "Riwayat tool call", hint: "Tool Inspector" },
            { label: "Ganti model", hint: "Ctrl+P" },
            { label: "Shortcuts", hint: "Daftar keybinding" },
          ].map((item, index) => (
            <button
              key={item.label}
              type="button"
              onClick={() => onQuickstart(index + 1)}
              className="flex items-center justify-between rounded bg-surface-container px-xs py-0.5 text-left hover:bg-surface-container-high"
            >
              <span className="flex items-center gap-sm">
                <span className="text-outline">[{index + 1}]</span>
                <span className="text-on-surface">{item.label}</span>
              </span>
              <span className="flex items-center gap-1 text-label-sm text-outline">
                {item.hint}
                <span className="material-symbols-outlined text-[13px]">arrow_forward</span>
              </span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
