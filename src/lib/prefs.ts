import type { Settings } from "./types";

/** Aplica tema e preferência de movimento no documento atual. */
export function applyPrefs(st: Pick<Settings, "theme" | "reduceMotion">) {
  const root = document.documentElement;
  if (st.theme === "light" || st.theme === "dark") root.dataset.theme = st.theme;
  else delete root.dataset.theme;
  if (st.reduceMotion) root.dataset.motion = "reduced";
  else delete root.dataset.motion;
}
