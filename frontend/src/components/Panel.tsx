// Header pane bergaya terminal: `[0x1] LABEL │ ● STATE` + pintasan kanan.
// Aturan referensi 16: hanya pane fokus yang memakai header terisi.

import type { ReactNode } from "react";

export interface PanelProps {
  index: string;
  label: string;
  state?: string;
  active?: boolean;
  hint?: string;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
  id?: string;
}

export function Panel({
  index,
  label,
  state,
  active = false,
  hint,
  children,
  className = "",
  bodyClassName = "",
  id,
}: PanelProps) {
  return (
    <section
      id={id}
      className={`term-frame flex min-h-0 flex-col ${className}`}
      aria-label={label}
    >
      <header className={`pane-header shrink-0 ${active ? "pane-header-active" : ""}`}>
        <span className="flex min-w-0 items-center gap-sm">
          <span className="opacity-70">[{index}]</span>
          <span className="truncate font-semibold tracking-[0.06em]">{label}</span>
          <span className="opacity-40">│</span>
          <span className="pane-state flex items-center gap-1 whitespace-nowrap">
            <span
              aria-hidden
              className="inline-block h-1.5 w-1.5 rounded-full"
              style={{
                background: active ? "var(--color-on-primary)" : "var(--color-outline)",
              }}
            />
            {state ?? "—"}
          </span>
        </span>
        {hint ? <span className="shrink-0 opacity-70">{hint}</span> : null}
      </header>
      <div className={`min-h-0 flex-1 ${bodyClassName}`}>{children}</div>
    </section>
  );
}
