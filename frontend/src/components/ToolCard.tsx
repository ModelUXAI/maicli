// Kartu tool bergaya sub-shell referensi 08: band `╭─ TOOL │ STATE │ elapsed`,
// body preview dengan gutter, footer exit/elapsed. Diff unified dirender
// lewat DiffView.

import { useStore } from "../state/store";
import type { ToolEntry } from "../state/store";
import { DiffView, isUnifiedDiff } from "./DiffView";

const COLLAPSE_LINES = 12;

function elapsed(tool: ToolEntry): string {
  const end = tool.endedAt ?? Date.now();
  const ms = end - tool.startedAt;
  return ms < 1000 ? `${Math.max(0, ms)}ms` : `${(ms / 1000).toFixed(2)}s`;
}

function gutter(lines: string[], width = 6): string {
  return lines
    .map((line, index) => `${String(index + 1).padStart(width, " ")} │ ${line}`)
    .join("\n");
}

export function ToolCard({ tool }: { tool: ToolEntry }) {
  const { dispatch } = useStore();
  const state = tool.isError ? "ERROR" : tool.endedAt ? "DONE" : "RUNNING";
  const stateColor = tool.isError
    ? "var(--color-error)"
    : tool.endedAt
      ? "var(--color-tertiary)"
      : "var(--color-secondary)";

  // Diff unified bisa datang dari argumen (`preview`) maupun hasil (`resultPreview`).
  const diffSource = [tool.preview, tool.resultPreview].find(
    (candidate) => candidate && isUnifiedDiff(candidate),
  );
  const body = diffSource ?? tool.resultPreview ?? tool.preview;
  const lines = body.split("\n");
  const folded = !tool.expanded && lines.length > COLLAPSE_LINES;
  const shown = folded ? lines.slice(0, COLLAPSE_LINES) : lines;

  return (
    <article data-tool-card className="term-frame flex flex-col" aria-label={`tool ${tool.name}`}>
      <div className="term-band term-band-top">
        <span className="flex min-w-0 items-center gap-1">
          <span className="text-tertiary">╭─</span>
          <span className="truncate font-semibold text-on-surface">{tool.name}</span>
          <span className="text-outline-variant">│</span>
          <span className="flex items-center gap-1" style={{ color: stateColor }}>
            <span
              aria-hidden
              className={`inline-block h-1.5 w-1.5 rounded-full ${state === "RUNNING" ? "animate-state-pulse" : ""}`}
              style={{ background: stateColor }}
            />
            {state}
          </span>
          <span className="text-outline-variant">│</span>
          <span className="text-outline">{elapsed(tool)}</span>
        </span>
        <button
          type="button"
          className="btn btn-ghost px-1 py-0 text-label-sm"
          onClick={() => dispatch({ type: "tool_toggle", id: tool.id })}
          aria-expanded={tool.expanded}
        >
          [{tool.expanded ? "-" : "+"}]
        </button>
      </div>

      <div className="bg-surface-container-lowest">
        {diffSource ? (
          <DiffView text={diffSource} />
        ) : (
          <pre className="mono-pre px-sm py-1">{folded ? gutter(shown) : gutter(lines)}</pre>
        )}
      </div>

      <div className="term-band justify-between">
        <span className="flex items-center gap-sm">
          <span>
            exit:{" "}
            <span style={{ color: stateColor }}>{tool.endedAt ? (tool.isError ? "1" : "0") : "—"}</span>
          </span>
          <span className="text-outline-variant">·</span>
          <span>{elapsed(tool)}</span>
          <span className="text-outline-variant">·</span>
          <span>bytes: —</span>
        </span>
        {folded ? (
          <button
            type="button"
            className="text-primary hover:underline"
            onClick={() => dispatch({ type: "tool_toggle", id: tool.id })}
          >
            Showing {shown.length} of {lines.length} · [^O] Expand
          </button>
        ) : (
          <span className="text-outline-variant">{lines.length} baris</span>
        )}
      </div>
    </article>
  );
}
