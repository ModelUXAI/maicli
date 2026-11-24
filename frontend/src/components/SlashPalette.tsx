// Slash palette (referensi 06). Daftar perintah berasal dari registry Rust
// (src/state/commands.ts), bukan daftar hardcode di komponen.

import { useEffect, useMemo, useRef, useState } from "react";
import { filterCommands, type KnownCommand } from "../state/commands";

export interface SlashPaletteProps {
  open: boolean;
  query: string;
  onClose: () => void;
  onPick: (command: KnownCommand) => void;
}

export function SlashPalette({ open, query, onClose, onPick }: SlashPaletteProps) {
  const items = useMemo(() => filterCommands(query).slice(0, 40), [query]);
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => setIndex(0), [query, open]);

  useEffect(() => {
    if (!open) return;
    // Capture + stopPropagation: palette adalah lapisan teratas, jadi Enter
    // tidak boleh ikut terkirim composer di baliknya.
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onClose();
        return;
      }
      const isNavigation =
        event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Tab" || event.key === "Enter";
      if (!isNavigation) return;
      event.stopPropagation();
      if (event.key === "ArrowDown" || (event.key === "Tab" && !event.shiftKey)) {
        event.preventDefault();
        setIndex((prev) => (items.length === 0 ? 0 : (prev + 1) % items.length));
        return;
      }
      if (event.key === "ArrowUp" || (event.key === "Tab" && event.shiftKey)) {
        event.preventDefault();
        setIndex((prev) => (items.length === 0 ? 0 : (prev - 1 + items.length) % items.length));
        return;
      }
      if (event.key === "Enter") {
        const chosen = items[index];
        if (chosen) {
          event.preventDefault();
          onPick(chosen);
        }
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [open, items, index, onPick, onClose]);

  useEffect(() => {
    const node = listRef.current?.querySelector<HTMLElement>(`[data-index="${index}"]`);
    node?.scrollIntoView({ block: "nearest" });
  }, [index]);

  if (!open) return null;

  return (
    <div className="absolute bottom-sm left-md right-md z-40">
      <div className="term-frame max-h-[280px] overflow-hidden">
        <div className="term-band term-band-top">
          <span className="flex items-center gap-xs">
            <span className="text-primary">╭─</span>
            <span className="font-semibold text-on-surface">Slash commands</span>
            <span className="text-outline-variant">
              {query.length > 0 ? `/${query}` : "semua perintah"}
            </span>
          </span>
          <span className="text-outline">
            {items.length} hasil · <kbd className="keycap">[↑/↓]</kbd> pindah ·{" "}
            <kbd className="keycap">[Enter]</kbd> pilih · <kbd className="keycap">[Esc]</kbd> tutup
          </span>
        </div>
        <div ref={listRef} className="max-h-[230px] overflow-y-auto bg-surface-container-lowest">
          {items.length === 0 ? (
            <div className="px-sm py-1 text-label-sm text-outline">
              Tidak ada perintah yang cocok dengan “{query}”.
            </div>
          ) : (
            items.map((command, i) => (
              <button
                key={command.name}
                type="button"
                data-index={i}
                onMouseEnter={() => setIndex(i)}
                onClick={() => onPick(command)}
                className={`flex w-full items-center justify-between gap-sm px-sm py-0.5 text-left font-mono text-body-sm ${
                  i === index
                    ? "bg-surface-container-high text-on-surface"
                    : "text-on-surface-variant"
                }`}
              >
                <span className="flex min-w-0 items-center gap-sm">
                  <span className="text-primary">/{command.name}</span>
                  {command.usage ? (
                    <span className="text-outline-variant">{command.usage}</span>
                  ) : null}
                </span>
                <span className="shrink-0 truncate text-label-sm text-outline">
                  {command.description}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
