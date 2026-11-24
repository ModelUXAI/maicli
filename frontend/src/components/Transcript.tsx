// Transkrip: user = blok berisi, assistant = teks polos, notice = baris kecil.
// Auto-scroll hanya bila pembaca berada dekat dasar (<=40px), supaya riwayat
// yang sedang dibaca tidak terlempar.

import { useEffect, useRef } from "react";
import { useStore } from "../state/store";
import { ToolCard } from "./ToolCard";
import { WelcomeView } from "./WelcomeView";

export function Transcript({ onQuickstart }: { onQuickstart: (index: number) => void }) {
  const { state } = useStore();
  const ref = useRef<HTMLDivElement | null>(null);
  const pinned = useRef(true);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const onScroll = () => {
      const distance = node.scrollHeight - node.scrollTop - node.clientHeight;
      pinned.current = distance <= 40;
    };
    node.addEventListener("scroll", onScroll, { passive: true });
    return () => node.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    const node = ref.current;
    if (!node || !pinned.current) return;
    node.scrollTop = node.scrollHeight;
  }, [state.turn, state.tools]);

  const empty = state.turn.length === 0 && state.tools.length === 0;

  return (
    <div
      ref={ref}
      data-transcript
      className="flex min-h-0 flex-1 flex-col gap-sm overflow-y-auto px-md py-sm"
    >
      {empty ? <WelcomeView onQuickstart={onQuickstart} /> : null}
      {state.turn.map((entry, index) => {
        if (entry.kind === "user") {
          return (
            <div key={`turn-${index}`} className="flex flex-col gap-0.5">
              <span className="text-label-sm uppercase tracking-[0.06em] text-outline">Anda</span>
              <div className="flex items-start gap-sm rounded-lg bg-surface-container-high px-sm py-1">
                <span className="select-none text-primary">❯</span>
                <span className="whitespace-pre-wrap break-words text-body-md text-on-surface">
                  {entry.text}
                </span>
              </div>
            </div>
          );
        }
        if (entry.kind === "notice") {
          return (
            <div key={`turn-${index}`} className="text-label-sm text-outline">
              {entry.text}
            </div>
          );
        }
        return (
          <div key={`turn-${index}`} className="flex flex-col gap-0.5">
            <span className="text-label-sm uppercase tracking-[0.06em] text-outline">Agen</span>
            <div className="whitespace-pre-wrap break-words text-body-md text-on-surface">
              {entry.text}
            </div>
          </div>
        );
      })}

      {state.tools.length > 0 ? (
        <div className="flex flex-col gap-sm pt-sm">
          <span className="text-label-sm uppercase tracking-[0.06em] text-outline">
            Tool call
          </span>
          {state.tools.map((tool) => (
            <ToolCard key={tool.id} tool={tool} />
          ))}
        </div>
      ) : null}
    </div>
  );
}
