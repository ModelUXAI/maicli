// HUD status sesi. Hanya data yang benar-benar dikirim RPC; sisanya "—" dengan
// alasan yang jujur, bukan angka rekaan.

import { useState } from "react";
import { useStore } from "../state/store";
import { asciiMeter } from "./Shell";
import { useCompactHeight } from "./useCompact";

function format(value: number): string {
  return value.toLocaleString("id-ID");
}

export function Hud() {
  const { state } = useStore();
  const compact = useCompactHeight();
  const [dismissed, setDismissed] = useState<{ error: string | null; notice: string | null }>({
    error: null,
    notice: null,
  });

  const total = state.inputTokens + state.outputTokens;
  const percent =
    state.contextWindow && state.contextWindow > 0
      ? Math.min(100, Math.round((total / state.contextWindow) * 100))
      : null;

  const toolNames = Array.from(new Set(state.tools.map((tool) => tool.name)));

  const showError = state.error !== null && state.error !== dismissed.error;
  const showNotice = state.notice !== null && state.notice !== dismissed.notice;

  return (
    <section className="flex flex-col gap-1 px-sm py-1" aria-label="Status sesi">
      {(showError || showNotice) && (
        <div className="flex flex-col gap-1">
          {showError ? (
            <div className="banner">
              <span className="material-symbols-outlined text-[14px]">error</span>
              <span className="flex-1 whitespace-pre-wrap break-words">{state.error}</span>
              <button
                type="button"
                className="btn btn-ghost px-1 py-0"
                onClick={() => setDismissed((prev) => ({ ...prev, error: state.error }))}
                aria-label="Tutup galat"
              >
                ✕
              </button>
            </div>
          ) : null}
          {showNotice ? (
            <div className="banner banner-neutral">
              <span className="material-symbols-outlined text-[14px]">info</span>
              <span className="flex-1 whitespace-pre-line break-words">{state.notice}</span>
              <button
                type="button"
                className="btn btn-ghost px-1 py-0"
                onClick={() => setDismissed((prev) => ({ ...prev, notice: state.notice }))}
                aria-label="Tutup catatan"
              >
                ✕
              </button>
            </div>
          ) : null}
        </div>
      )}

      <div className="flex flex-wrap items-center justify-between gap-sm text-label-md">
        <div className="flex flex-wrap items-center gap-sm">
          <span className="flex items-center gap-1.5">
            <span
              aria-hidden
              className="inline-block h-2.5 w-2.5 animate-state-pulse rounded-full"
              style={{ background: "var(--color-secondary)" }}
            />
            <span className="text-headline-sm text-on-surface">{state.model}</span>
          </span>
          <span className="state-pill">SESSION</span>
          <span className="flex items-center gap-1 rounded bg-surface-container-lowest px-sm py-xs">
            <span className="text-outline">Session Cost:</span>
            <span className="font-bold text-outline-variant">—</span>
          </span>
          <span className="flex items-center gap-1 rounded bg-surface-container-lowest px-sm py-xs">
            <span className="text-outline">Token:</span>
            <span className="text-on-surface">{format(total)}</span>
          </span>
          <span className="flex items-center gap-1 rounded bg-surface-container-lowest px-sm py-xs">
            <span className="text-outline">tok/s:</span>
            <span className="text-outline-variant">—</span>
          </span>
        </div>
        <span className="flex items-center gap-sm text-label-sm text-outline-variant">
          {compact ? (
            <span className="flex items-center gap-1">
              <span className="ascii-meter text-[11px]">
                {percent === null ? "[░░░░░░░░]" : asciiMeter(percent)}
              </span>
              <span className="text-on-surface-variant">{percent === null ? "—" : `${percent}%`}</span>
            </span>
          ) : null}
          <span>
            {state.busy ? "RUNNING" : "IDLE"} · antrean {state.queued}
          </span>
        </span>
      </div>

      <div className={`flex-col gap-xs rounded-lg bg-surface-container-lowest p-sm ${compact ? "hidden" : "flex"}`}>
        <div className="flex flex-wrap items-center justify-between gap-sm text-label-sm">
          <span className="flex flex-wrap items-center gap-xs text-on-surface-variant">
            <span className="material-symbols-outlined text-[14px] text-primary">data_array</span>
            Context Window Utilization:
            <span className="font-bold text-on-surface">
              {format(total)} / {state.contextWindow === null ? "—" : format(state.contextWindow)} token
            </span>
            <span className="rounded bg-surface-container-high px-1 text-primary">
              {percent === null ? "—" : `${percent}% consumed`}
            </span>
          </span>
          <span className="ascii-meter">{percent === null ? "[░░░░░░░░]" : asciiMeter(percent)}</span>
        </div>
        <div className="flex h-2.5 w-full overflow-hidden rounded-full bg-surface-container-highest">
          <div
            className="h-full bg-primary-container transition-all duration-500 ease-out"
            style={{ width: `${percent ?? 0}%` }}
          />
        </div>
        <div className="flex flex-wrap items-center justify-between gap-sm text-label-sm text-outline-variant">
          <div className="flex flex-wrap items-center gap-sm">
            <span className="flex items-center gap-1">
              <span className="h-2 w-2 rounded-full bg-primary-container" /> Input{" "}
              {format(state.inputTokens)}
            </span>
            <span className="flex items-center gap-1">
              <span className="h-2 w-2 rounded-full bg-secondary/70" /> Output{" "}
              {format(state.outputTokens)}
            </span>
            <span className="flex items-center gap-1">
              <span className="h-2 w-2 rounded-full bg-surface-variant" /> Cache{" "}
              {format(state.cachedTokens)}
            </span>
          </div>
          <span>
            {state.contextWindow === null
              ? "Konteks maksimum belum tersedia lewat RPC"
              : `Sisa: ${format(Math.max(0, state.contextWindow - total))} token`}
          </span>
        </div>
      </div>

      <div className={`flex-wrap items-center justify-between gap-sm font-code-inline text-label-sm ${compact ? "hidden" : "flex"}`}>
        <div className="flex flex-wrap items-center gap-xs">
          <span className="mr-1 text-label-sm uppercase tracking-wider text-outline">
            Tools Armed:
          </span>
          {toolNames.length === 0 ? (
            <span className="text-outline-variant">—</span>
          ) : (
            toolNames.map((name) => (
              <span
                key={name}
                className="flex items-center gap-1 rounded bg-surface-container-high px-xs py-0.5 text-primary"
              >
                <span className="material-symbols-outlined text-[13px]">handyman</span>
                {name}
              </span>
            ))
          )}
        </div>
        <span className="flex items-center gap-1 text-tertiary">
          <span className="material-symbols-outlined text-[14px]">shield</span>
          <span>Auto-Execution: mengikuti config approval</span>
        </span>
      </div>
    </section>
  );
}
