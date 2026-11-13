import "./styles/tokens.css";
import "./styles/widget.css";
import "./styles/panel.css";
import { invoke, listen, isTauri } from "./lib/ipc";
import { applyPrefs } from "./lib/prefs";
import type { Settings } from "./lib/types";

const view = new URLSearchParams(location.search).get("view") ?? "panel";
const root = document.getElementById("app")!;
document.body.classList.add(`view-${view}`);

// Sem menu de contexto do navegador nem zoom acidental.
window.addEventListener("contextmenu", (e) => e.preventDefault());
window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });

invoke<Settings>("get_settings").then(applyPrefs).catch(() => {});
listen<Settings>("settings", applyPrefs);

if (view === "widget") {
  import("./views/widget").then((m) => m.mountWidget(root));
} else {
  import("./views/panel").then((m) => m.mountPanel(root));
}

// O app nativo registra o atalho global; no navegador a prévia usa o mesmo atalho local.
if (!isTauri) {
  let keys = "PageUp";
  invoke<Settings>("get_settings").then(st => { keys = st.providerShortcut; });
  listen<Settings>("settings", st => { keys = st.providerShortcut; });
  window.addEventListener("keydown", e => {
    if (e.repeat || (e.target as HTMLElement).closest("input, textarea")) return;
    const key = e.code.startsWith("Key") ? e.code.slice(3) : e.code.startsWith("Digit") ? e.code.slice(5) : e.code;
    const combo = [...(e.ctrlKey ? ["Control"] : []), ...(e.altKey ? ["Alt"] : []), ...(e.shiftKey ? ["Shift"] : []), ...(e.metaKey ? ["Super"] : []), key].join("+");
    if (combo === keys) { e.preventDefault(); invoke("toggle_provider"); }
  });
}
