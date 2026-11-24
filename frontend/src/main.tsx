// Titik masuk aplikasi. Renderer sengaja tipis: state, transport, dan seluruh
// perilaku hidup di modul masing-masing.

import { createRoot } from "react-dom/client";
import "./app.css";
import { App } from "./components/App";
import { AppStateProvider } from "./state/store";
import { RpcClient } from "./rpc/client";

const container = document.getElementById("root");
if (!container) throw new Error("elemen #root tidak ditemukan");

async function bootstrap() {
  const params = new URLSearchParams(location.search);
  let client: RpcClient | undefined;
  // Preview hanya ada di build dev; dynamic import menjaga bundle produksi bersih.
  if (import.meta.env.DEV && params.has("preview")) {
    const { createPreviewClient } = await import("./preview");
    client = createPreviewClient(params);
  }

  createRoot(container as HTMLElement).render(
    <AppStateProvider client={client}>
      <App />
    </AppStateProvider>,
  );
}

void bootstrap();
