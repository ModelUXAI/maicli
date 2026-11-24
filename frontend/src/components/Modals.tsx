// Modal bersama untuk Model, Settings, Shortcuts, dan pertanyaan agen.
// Semua membaca data dari RPC; tidak ada daftar model yang di-hardcode.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { useStore } from "../state/store";

function ModalShell({
  title,
  subtitle,
  onClose,
  children,
  footer,
}: {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label={title}>
      <div className="modal-card">
        <div className="modal-head">
          <div className="flex min-w-0 flex-col">
            <span className="text-headline-sm font-semibold text-on-surface">{title}</span>
            {subtitle ? <span className="text-label-sm text-outline">{subtitle}</span> : null}
          </div>
          <button type="button" className="btn btn-ghost" onClick={onClose} aria-label="Tutup">
            ✕
          </button>
        </div>
        <div className="modal-body flex flex-col gap-md">{children}</div>
        {footer ? (
          <div className="flex items-center justify-between border-t border-outline-variant px-md py-sm text-label-sm text-outline">
            {footer}
          </div>
        ) : null}
      </div>
    </div>
  );
}

export function ModelModal({ onClose }: { onClose: () => void }) {
  const { state, client } = useStore();
  const [output, setOutput] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [modelId, setModelId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const load = () => {
    setLoading(true);
    void client
      .command("/model")
      .then((frame) => {
        const text = typeof frame.text === "string" ? frame.text : "";
        setOutput(text.length > 0 ? text : "Daftar model belum tersedia dari core.");
      })
      .catch((err: unknown) => {
        setOutput("Daftar model belum tersedia dari core.");
        setError(String(err));
      })
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    load();
    inputRef.current?.focus();
  }, []);

  const apply = () => {
    const id = modelId.trim();
    if (id.length === 0) return;
    setPending(true);
    setError(null);
    void client
      .setModel(id)
      .then(() => setModelId(""))
      .catch((err: unknown) => setError(String(err)))
      .finally(() => setPending(false));
  };

  return (
    <ModalShell
      title="Model Provider"
      subtitle={`model aktif: ${state.model}`}
      onClose={onClose}
      footer={
        <>
          <span>Daftar berasal dari command_output /model</span>
          <button type="button" className="btn" onClick={load} disabled={loading}>
            {loading ? "memuat…" : "Muat ulang"}
          </button>
        </>
      }
    >
      <div className="term-frame">
        <div className="term-band term-band-top">
          <span className="flex items-center gap-xs">
            <span className="text-primary">╭─</span>
            <span className="font-semibold text-on-surface">OUTPUT /model</span>
          </span>
          <span>{loading ? "menunggu…" : "selesai"}</span>
        </div>
        <pre className="mono-pre max-h-[240px] overflow-auto p-sm text-on-surface">
          {output ?? "memuat daftar model…"}
        </pre>
      </div>

      <div className="flex flex-wrap items-end gap-sm">
        <label className="flex flex-1 flex-col gap-0.5">
          <span className="text-label-sm uppercase tracking-[0.06em] text-outline">ID model</span>
          <input
            ref={inputRef}
            className="field"
            value={modelId}
            onChange={(event) => setModelId(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") apply();
            }}
            placeholder="mis. deepseek-v4.1-flash"
          />
        </label>
        <button type="button" className="btn btn-primary" onClick={apply} disabled={pending}>
          {pending ? "mengirim…" : "Ganti"}
        </button>
      </div>

      {error ? <div className="banner">{error}</div> : null}
      <p className="text-label-sm text-outline">
        Model di UI hanya berubah setelah frame <code>session_info_update</code> diterima.
      </p>
    </ModalShell>
  );
}

export function SettingsModal({ onClose }: { onClose: () => void }) {
  const { client } = useStore();
  const [output, setOutput] = useState<string | null>(null);

  useEffect(() => {
    void client
      .command("/settings")
      .then((frame) => {
        const text = typeof frame.text === "string" ? frame.text : "";
        setOutput(text.length > 0 ? text : "Output /settings kosong dari core.");
      })
      .catch((err: unknown) => setOutput(`Gagal memuat settings: ${String(err)}`));
  }, [client]);

  return (
    <ModalShell
      title="Settings"
      subtitle="read-only: config ditulis lewat core, bukan frontend"
      onClose={onClose}
      footer={<span>Output command_output /settings</span>}
    >
      <pre className="mono-pre max-h-[420px] overflow-auto rounded bg-surface-container-lowest p-sm text-on-surface">
        {output ?? "memuat settings…"}
      </pre>
    </ModalShell>
  );
}

export function ShortcutsModal({ onClose }: { onClose: () => void }) {
  const groups: { title: string; rows: [string, string][] }[] = [
    {
      title: "COMPOSER",
      rows: [
        ["Enter", "Kirim prompt"],
        ["Shift+Enter", "Baris baru"],
        ["/", "Buka slash palette"],
        ["Tab", "Lengkapi / pindah item palette"],
        ["↑ / ↓", "Pindah item palette"],
        ["Esc", "Tutup modal / fokus composer"],
        ["Ctrl+C", "Batalkan turn saat berjalan"],
      ],
    },
    {
      title: "NAVIGASI",
      rows: [
        ["Ctrl+P", "Buka modal model"],
        ["Ctrl+B", "Lompat ke Tool Inspector"],
        ["[?]", "Buka daftar shortcut ini"],
        ["[/]", "Buka slash palette"],
        ["[1-4]", "Quickstart dispatcher"],
        ["Klik rail", "Pindah panel / buka modal"],
        ["Klik [+/−]", "Lipat atau buka tool card"],
      ],
    },
    {
      title: "APPROVAL",
      rows: [
        ["Y", "Setuju sekali"],
        ["N", "Tolak"],
        ["S", "Setuju sesi"],
        ["Esc", "Tolak (fail-safe)"],
        ["—", "Pilihan always tidak ditampilkan"],
        ["—", "Fokus awal selalu di Tolak"],
        ["—", "Modal tunggal: approval baru menimpa"],
      ],
    },
  ];

  return (
    <ModalShell title="Keyboard Shortcuts" subtitle="hanya binding yang benar-benar aktif" onClose={onClose}>
      <div className="grid grid-cols-1 gap-md md:grid-cols-3">
        {groups.map((group) => (
          <div key={group.title} className="flex flex-col gap-0.5">
            <span className="mb-1 text-label-sm uppercase tracking-[0.06em] text-outline">
              {group.title}
            </span>
            {group.rows.map(([keys, description]) => (
              <div
                key={`${group.title}-${keys}-${description}`}
                className="flex items-center justify-between gap-sm rounded bg-surface-container-low px-sm py-0.5"
              >
                <kbd className="keycap">{keys}</kbd>
                <span className="truncate text-label-sm text-on-surface-variant">{description}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </ModalShell>
  );
}

export function AskDialogModal() {
  const { state, dispatch, client } = useStore();
  const ask = state.ask;
  const question = state.question;
  const [text, setText] = useState("");

  useEffect(() => {
    if (!ask) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      client.askAnswer(ask.id, null, "");
      dispatch({ type: "ask_closed" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ask, client, dispatch]);

  useEffect(() => {
    if (!question) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      client.answer("");
      dispatch({ type: "ask_closed" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [question, client, dispatch]);

  if (ask) {
    return (
      <ModalShell
        title="Pertanyaan agen"
        subtitle={ask.timeoutSecs > 0 ? `batas waktu ${ask.timeoutSecs}s` : "tanpa batas waktu"}
        onClose={() => {
          client.askAnswer(ask.id, null, "");
          dispatch({ type: "ask_closed" });
        }}
      >
        <p className="whitespace-pre-wrap text-body-md text-on-surface">{ask.question}</p>
        <div className="flex flex-col gap-1">
          {ask.options.map((option, index) => (
            <button
              key={`${option}-${index}`}
              type="button"
              className={`flex items-center justify-between rounded bg-surface-container-low px-sm py-1 text-left text-body-sm hover:bg-surface-container-high ${
                ask.recommended === index ? "outline outline-1 outline-primary" : ""
              }`}
              onClick={() => {
                client.askAnswer(ask.id, index, "");
                dispatch({ type: "ask_closed" });
              }}
            >
              <span className="text-on-surface">
                <span className="mr-1 text-outline">[{index + 1}]</span>
                {option}
              </span>
              {ask.recommended === index ? (
                <span className="text-label-sm text-primary">disarankan</span>
              ) : null}
            </button>
          ))}
        </div>
        <div className="flex items-end gap-sm">
          <label className="flex flex-1 flex-col gap-0.5">
            <span className="text-label-sm uppercase tracking-[0.06em] text-outline">
              Jawaban teks
            </span>
            <input
              className="field"
              value={text}
              onChange={(event) => setText(event.target.value)}
              onKeyDown={(event) => {
                if (event.key !== "Enter") return;
                client.askAnswer(ask.id, null, text);
                dispatch({ type: "ask_closed" });
              }}
            />
          </label>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => {
              client.askAnswer(ask.id, null, text);
              dispatch({ type: "ask_closed" });
            }}
          >
            Kirim
          </button>
        </div>
      </ModalShell>
    );
  }

  if (question) {
    return (
      <ModalShell
        title="Pertanyaan agen"
        subtitle="dari frame question"
        onClose={() => {
          client.answer("");
          dispatch({ type: "ask_closed" });
        }}
      >
        <p className="whitespace-pre-wrap text-body-md text-on-surface">{question.question}</p>
        <div className="flex items-end gap-sm">
          <label className="flex flex-1 flex-col gap-0.5">
            <span className="text-label-sm uppercase tracking-[0.06em] text-outline">Jawaban</span>
            <input className="field" value={text} onChange={(event) => setText(event.target.value)} />
          </label>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => {
              client.answer(text);
              dispatch({ type: "ask_closed" });
            }}
          >
            Kirim
          </button>
        </div>
      </ModalShell>
    );
  }

  return null;
}

export { ModalShell };
