// Komposisi layar: rail + header/footer dari Shell, panel transkrip, HUD,
// composer, dan semua modal. Tidak ada data demo di jalur produksi.

import { useCallback, useEffect, useState } from "react";
import { ApprovalModal } from "./ApprovalModal";
import { Composer } from "./Composer";
import { Hud } from "./Hud";
import { AskDialogModal, ModelModal, SettingsModal, ShortcutsModal } from "./Modals";
import { Panel } from "./Panel";
import { Shell } from "./Shell";
import { SlashPalette } from "./SlashPalette";
import { Transcript } from "./Transcript";
import { useStore } from "../state/store";
import type { KnownCommand } from "../state/commands";
import { setComposerText } from "./composerBridge";

type ModalId = "model" | "settings" | "shortcuts" | "palette" | null;

export function App() {
  const { state, dispatch, client } = useStore();
  const [modal, setModal] = useState<ModalId>(null);
  const [slashQuery, setSlashQuery] = useState("");

  useEffect(() => {
    const openShortcuts = () => setModal("shortcuts");
    window.addEventListener("maicli:open-shortcuts", openShortcuts);
    return () => window.removeEventListener("maicli:open-shortcuts", openShortcuts);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.isContentEditable;
      if (event.ctrlKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        setModal("model");
        return;
      }
      if (event.ctrlKey && event.key.toLowerCase() === "b") {
        event.preventDefault();
        const cards = document.querySelectorAll("[data-tool-card]");
        cards[cards.length - 1]?.scrollIntoView({ block: "center" });
        return;
      }
      if (event.key === "?" && !typing) {
        event.preventDefault();
        setModal("shortcuts");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const pickCommand = useCallback(
    (command: KnownCommand) => {
      setModal(null);
      setComposerText("");
      if (command.name === "model") {
        setModal("model");
        return;
      }
      if (command.name === "settings") {
        setModal("settings");
        return;
      }
      if (command.name === "usage") {
        void client
          .command("/usage")
          .then((frame) => {
            const text = typeof frame.text === "string" ? frame.text : "";
            dispatch({ type: "notice", text: text.length > 0 ? text : "output /usage kosong" });
          })
          .catch((error: unknown) => dispatch({ type: "error", message: String(error) }));
        return;
      }
      setComposerText(`/${command.name} `);
      document.querySelector<HTMLTextAreaElement>("[data-composer]")?.focus();
    },
    [client, dispatch],
  );

  const quickstart = useCallback(
    (index: number) => {
      if (index === 1) {
        document.querySelector<HTMLTextAreaElement>("[data-composer]")?.focus();
        return;
      }
      if (index === 2) {
        const cards = document.querySelectorAll("[data-tool-card]");
        cards[cards.length - 1]?.scrollIntoView({ block: "center" });
        return;
      }
      if (index === 3) {
        setModal("model");
        return;
      }
      setModal("shortcuts");
    },
    [],
  );

  return (
    <Shell>
      <div className="flex h-full min-h-0 flex-col">
        <Panel
          index="0x1"
          label="TRANSCRIPT"
          state={state.busy ? "RUNNING" : "IDLE"}
          active
          hint="Alt+1"
          bodyClassName="flex min-h-0 flex-col"
          className={`mx-sm mt-sm min-h-0 flex-1 transition-opacity ${
            modal === "palette" ? "opacity-35" : ""
          }`}
        >
          <Transcript onQuickstart={quickstart} />
        </Panel>

        <div className="relative flex shrink-0 flex-col gap-sm px-sm pb-1 pt-sm">
          <Hud />
          <Composer
            onSlash={(query) => {
              setSlashQuery(query);
              setModal("palette");
            }}
            onOpenModel={() => setModal("model")}
            onOpenPalette={() => {
              setSlashQuery("");
              setModal("palette");
            }}
          />
          <SlashPalette
            open={modal === "palette"}
            query={slashQuery}
            onClose={() => setModal(null)}
            onPick={pickCommand}
          />
        </div>
      </div>

      <ApprovalModal />
      <AskDialogModal />
      {modal === "model" ? <ModelModal onClose={() => setModal(null)} /> : null}
      {modal === "settings" ? <SettingsModal onClose={() => setModal(null)} /> : null}
      {modal === "shortcuts" ? <ShortcutsModal onClose={() => setModal(null)} /> : null}
    </Shell>
  );
}
