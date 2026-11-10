import "./styles/tokens.css";
import "./styles/widget.css";
import "./styles/panel.css";
import { invoke, listen } from "./lib/ipc";
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
