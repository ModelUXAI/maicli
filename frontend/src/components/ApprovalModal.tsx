// Modal approval (referensi 11). Fail-safe: fokus awal di tombol Tolak,
// Esc = tolak, Y/N/S hanya aktif selama modal terbuka. Pilihan `always`
// sengaja tidak ditampilkan.

import { useEffect, useRef } from "react";
import { useStore } from "../state/store";

export function ApprovalModal() {
  const { state, dispatch, client } = useStore();
  const denyRef = useRef<HTMLButtonElement | null>(null);
  const approval = state.approval;

  useEffect(() => {
    if (!approval) return;
    denyRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.isContentEditable;
      if (event.key === "Escape") {
        event.preventDefault();
        client.approve(approval.id, "deny");
        dispatch({ type: "approval_closed" });
        return;
      }
      if (typing) return;
      const key = event.key.toLowerCase();
      if (key === "y" || key === "n" || key === "s") {
        event.preventDefault();
        const decision = key === "y" ? "once" : key === "s" ? "session" : "deny";
        client.approve(approval.id, decision);
        dispatch({ type: "approval_closed" });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [approval, client, dispatch]);

  if (!approval) return null;

  const decide = (decision: "once" | "session" | "deny") => {
    client.approve(approval.id, decision);
    dispatch({ type: "approval_closed" });
  };

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="Persetujuan tool">
      <div className="modal-card overflow-hidden">
        <div
          className="flex items-center justify-between px-md py-xs font-mono text-label-md font-bold tracking-wider"
          style={{ background: "var(--color-error-container)", color: "#ffdad6" }}
        >
          <span className="flex items-center gap-sm">
            <span className="material-symbols-outlined animate-state-pulse text-[16px]">
              crisis_alert
            </span>
            ╔══ SECURITY &amp; PERMISSION GUARDRAIL ══╗
          </span>
          <span className="flex items-center gap-1">
            <span className="text-[10px] opacity-80">AUTH_GATE:</span>
            <span
              className="rounded bg-surface-container-lowest px-1.5 py-0.5 font-mono text-label-sm font-bold"
              style={{ color: "var(--color-error)" }}
            >
              STRICT ENFORCEMENT
            </span>
          </span>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-sm bg-surface-container-low px-md py-sm">
          <div className="flex items-center gap-sm">
            <div
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded"
              style={{ background: "color-mix(in srgb, var(--color-error) 15%, transparent)" }}
            >
              <span className="material-symbols-outlined text-[20px]" style={{ color: "var(--color-error)" }}>
                shield_lock
              </span>
            </div>
            <div>
              <div className="flex items-center gap-xs">
                <span className="text-headline-sm font-bold text-on-surface">Persetujuan Tool</span>
                <span
                  className="rounded px-1 py-0.5 font-mono text-label-sm font-bold"
                  style={{ background: "var(--color-error)", color: "var(--color-surface)" }}
                >
                  RISK: {approval.tier}
                </span>
              </div>
              <p className="text-body-sm text-outline">
                Agen meminta izin menjalankan tool sebelum melanjutkan turn.
              </p>
            </div>
          </div>
          <button
            type="button"
            className="btn btn-ghost"
            aria-label="Tutup dan tolak"
            onClick={() => decide("deny")}
          >
            ✕
          </button>
        </div>

        <div className="flex flex-col gap-md bg-surface-container-lowest p-md">
          <div className="grid grid-cols-1 gap-sm md:grid-cols-3">
            <InfoCard label="TOOL" value={approval.tool} tone="secondary" icon="handyman" />
            <InfoCard label="TIER" value={approval.tier} tone="error" icon="local_fire_department" />
            <InfoCard label="SUBJECT" value={approval.subject} tone="primary" icon="hub" />
          </div>

          <div className="overflow-hidden rounded bg-surface-container-low">
            <div className="flex items-center justify-between bg-surface-container px-sm py-0.5 font-mono text-label-sm text-outline">
              <span className="flex items-center gap-xs">
                <span className="text-tertiary">╭─</span>
                <span className="font-bold text-on-surface">INSPECTOR: SUBJECT</span>
              </span>
              <span>sumber: frame approval_request</span>
            </div>
            <pre className="mono-pre overflow-x-auto bg-surface-container-lowest p-sm text-on-surface">
              {approval.subject}
            </pre>
          </div>

          <div className="flex items-center justify-between rounded bg-surface-container px-sm py-1 text-label-sm text-outline">
            <span>Permintaan akan kedaluwarsa sesuai batas server.</span>
            <span className="font-mono text-outline-variant">id: {approval.id}</span>
          </div>

          <div className="flex items-center justify-between px-xs text-label-sm text-outline">
            <span>AVAILABLE RESOLUTION ACTIONS</span>
            <span>TEKAN TOMBOL UNTUK DISPATCH</span>
          </div>

          <div className="grid grid-cols-1 gap-xs md:grid-cols-3">
            <button
              type="button"
              onClick={() => decide("once")}
              className="flex items-center justify-between rounded p-sm text-left"
              style={{
                background: "var(--color-tertiary-container)",
                color: "var(--color-surface)",
              }}
            >
              <span className="flex min-w-0 items-center gap-sm">
                <span
                  className="shrink-0 rounded bg-surface-container-lowest px-1.5 py-0.5 font-mono text-label-sm font-bold"
                  style={{ color: "var(--color-tertiary)" }}
                >
                  Y
                </span>
                <span className="truncate">
                  <span className="block text-label-lg font-bold">Setuju Sekali</span>
                  <span className="block text-body-sm opacity-90">Jalankan sekali ini saja</span>
                </span>
              </span>
              <span className="shrink-0 font-mono text-[11px] font-bold opacity-80">[Y]</span>
            </button>

            <button
              ref={denyRef}
              type="button"
              onClick={() => decide("deny")}
              className="flex items-center justify-between rounded p-sm text-left"
              style={{ background: "var(--color-error-container)", color: "#ffdad6" }}
            >
              <span className="flex min-w-0 items-center gap-sm">
                <span
                  className="shrink-0 rounded bg-surface-container-lowest px-1.5 py-0.5 font-mono text-label-sm font-bold"
                  style={{ color: "var(--color-error)" }}
                >
                  N
                </span>
                <span className="truncate">
                  <span className="block text-label-lg font-bold">Tolak</span>
                  <span className="block text-body-sm opacity-90">Hentikan permintaan tool</span>
                </span>
              </span>
              <span className="shrink-0 font-mono text-[11px] font-bold opacity-80">[Esc / N]</span>
            </button>

            <button
              type="button"
              onClick={() => decide("session")}
              className="flex items-center justify-between rounded bg-surface-container-high p-sm text-left hover:bg-surface-container-highest"
            >
              <span className="flex min-w-0 items-center gap-sm">
                <span
                  className="shrink-0 rounded px-1.5 py-0.5 font-mono text-label-sm font-bold"
                  style={{
                    background: "var(--color-secondary-container)",
                    color: "var(--color-secondary)",
                  }}
                >
                  S
                </span>
                <span className="truncate">
                  <span className="block text-label-lg font-bold text-on-surface">Setuju Sesi</span>
                  <span className="block text-body-sm text-outline">Izinkan selama sesi ini</span>
                </span>
              </span>
              <span className="shrink-0 font-mono text-[11px] text-outline">[S]</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function InfoCard({
  label,
  value,
  tone,
  icon,
}: {
  label: string;
  value: string;
  tone: "secondary" | "error" | "primary";
  icon: string;
}) {
  const color =
    tone === "secondary"
      ? "var(--color-secondary)"
      : tone === "error"
        ? "var(--color-error)"
        : "var(--color-primary)";
  return (
    <div className="flex flex-col justify-between rounded bg-surface-container-low p-sm">
      <span className="flex items-center gap-1 font-mono text-label-sm uppercase tracking-[0.06em] text-outline">
        <span className="material-symbols-outlined text-[14px]">{icon}</span> {label}
      </span>
      <div className="mt-xs">
        <div className="truncate text-body-lg font-bold" style={{ color }} title={value}>
          {value}
        </div>
      </div>
    </div>
  );
}
