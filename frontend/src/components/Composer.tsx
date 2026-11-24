// Composer tiga band, replika referensi 02: band ASCII atas, body prompt,
// band bawah berisi keybinding. Tidak ada lampiran berkas karena RPC tidak
// punya request attach.

import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useStore } from "../state/store";
import { COMPOSER_SET_EVENT } from "./composerBridge";

export interface ComposerProps {
  onSlash: (query: string) => void;
  onOpenModel: () => void;
  onOpenPalette: () => void;
}

function estimateTokens(text: string): number {
  const words = text.trim().length === 0 ? 0 : text.trim().split(/\s+/).length;
  return Math.round(words * 1.3);
}

export function Composer({ onSlash, onOpenModel, onOpenPalette }: ComposerProps) {
  const { state, dispatch, client } = useStore();
  const [input, setInput] = useState("");
  const ref = useRef<HTMLTextAreaElement | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        document.querySelector<HTMLTextAreaElement>("[data-composer]")?.focus();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  // Palette/modal bisa mengganti isi composer (mis. setelah memilih /model
  // yang langsung membuka modal, isi composer harus dikosongkan).
  useEffect(() => {
    const onSet = (event: Event) => {
      const detail = (event as CustomEvent<{ text?: string }>).detail;
      setInput(detail?.text ?? "");
    };
    window.addEventListener(COMPOSER_SET_EVENT, onSet);
    return () => window.removeEventListener(COMPOSER_SET_EVENT, onSet);
  }, []);

  const send = () => {
    const text = input.trim();
    if (text.length === 0) return;
    if (state.busy) {
      setNotice("turn berjalan; ^C untuk membatalkan");
      return;
    }
    // Hasil perintah ditampilkan apa adanya dari `command_output` server.
    if (text.startsWith("/")) {
      void client
        .command(text)
        .then((frame) => {
          const output = typeof frame.text === "string" ? frame.text : "";
          dispatch({
            type: "notice",
            text: output.length > 0 ? output : `${text}: tidak ada output dari core`,
          });
        })
        .catch((error: unknown) => {
          dispatch({ type: "error", message: String(error) });
        });
      setInput("");
      return;
    }
    setInput("");
    setNotice(null);
    dispatch({ type: "prompt_sent", text });
    void client.prompt(text).catch((error: unknown) => {
      dispatch({ type: "prompt_result", status: "error", error: String(error) });
    });
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
      return;
    }
    if (event.key === "Enter" && event.shiftKey) {
      return;
    }
    if (event.ctrlKey && event.key.toLowerCase() === "c") {
      const target = event.currentTarget;
      const hasSelection =
        target.selectionStart !== target.selectionEnd ||
        (typeof window.getSelection === "function" && (window.getSelection()?.toString().length ?? 0) > 0);
      if (hasSelection) return;
      if (!state.busy) return;
      event.preventDefault();
      client.abort();
      dispatch({ type: "prompt_result", status: "aborted", error: null });
      return;
    }
    if (event.key === "/" && input.length === 0) {
      onSlash("");
      return;
    }
    if (event.ctrlKey && event.key.toLowerCase() === "p") {
      event.preventDefault();
      onOpenModel();
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <div className="term-frame flex flex-col">
        <div className="term-band term-band-top">
          <span className="flex min-w-0 items-center gap-1">
            <span className="text-primary">╭─</span>
            <span className="font-semibold text-on-surface">Prompt</span>
            <span className="truncate text-outline-variant">
              (Markdown &amp; Multi-line enabled · Enter kirim, Shift+Enter baris baru)
            </span>
            <span className="hidden text-outline-variant lg:inline">────────────────</span>
          </span>
          <span className="flex shrink-0 items-center gap-1">
            <span className="rounded bg-surface-container px-1 text-label-sm text-on-surface-variant">
              AUTONOMOUS
            </span>
            <span className="text-outline">─╮</span>
          </span>
        </div>

        <div className="flex min-h-[160px] flex-col justify-between bg-surface-container-lowest p-md">
          <div className="flex w-full items-start gap-sm">
            <span className="select-none pt-0.5 text-headline-sm font-bold leading-none text-primary">
              ❯
            </span>
            <div className="relative flex-1">
              <textarea
                ref={ref}
                data-composer
                aria-label="Prompt"
                value={input}
                onChange={(event) => {
                  setInput(event.target.value);
                  const atStart = event.target.value.startsWith("/") && !event.target.value.includes(" ");
                  if (atStart) onSlash(event.target.value.slice(1));
                }}
                onKeyDown={onKeyDown}
                rows={3}
                spellCheck={false}
                aria-placeholder="Tulis prompt untuk agen…"
                className="w-full resize-none bg-transparent font-mono text-body-lg leading-relaxed text-on-surface caret-primary outline-none"
              />
              {input.length === 0 ? (
                <span
                  aria-hidden
                  className="pointer-events-none absolute left-0 top-0 flex items-center gap-1 text-body-lg text-outline-variant"
                >
                  <span className="animate-caret-pulse inline-block h-5 w-2.5 bg-primary" />
                  Tulis prompt untuk agen…
                </span>
              ) : null}
            </div>
          </div>

          <div className="mt-md flex flex-wrap items-center justify-between gap-sm text-label-sm text-on-surface-variant">
            <span className="text-outline">
              Estimasi: ~{estimateTokens(input)} token{" "}
              <span className="text-outline-variant">(perkiraan)</span>
            </span>
            <div className="flex items-center gap-sm">
              {notice ? <span className="text-error">{notice}</span> : null}
              <button
                type="button"
                className="btn btn-primary"
                onClick={send}
                disabled={state.busy || input.trim().length === 0}
              >
                Kirim <span className="text-[10px] opacity-75">[⏎]</span>
              </button>
            </div>
          </div>
        </div>

        <div className="term-band">
          <div className="flex flex-wrap items-center gap-sm">
            <span className="text-outline">╰─</span>
            <span className="flex items-center gap-1">
              <kbd className="keycap">[Tab]</kbd> Lengkapi
            </span>
            <button
              type="button"
              className="flex items-center gap-1 text-on-surface hover:text-primary"
              onClick={onOpenPalette}
            >
              <kbd className="keycap">[/]</kbd> Slash commands
            </button>
            <button
              type="button"
              className="flex items-center gap-1 text-on-surface hover:text-primary"
              onClick={onOpenModel}
            >
              <kbd className="keycap">[^P]</kbd> Ganti model
            </button>
            <span className="flex items-center gap-1">
              <kbd className="keycap">[^C]</kbd> Batalkan
            </span>
          </div>
          <span className="text-outline">─╯</span>
        </div>
      </div>
    </div>
  );
}
