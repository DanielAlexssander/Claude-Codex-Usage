// Ilha: o widget em forma de pílula centralizada numa borda da tela.
// Fechada mostra a logo e o % da janela principal; ao parar o mouse em cima ela se expande
// (o Rust acompanha o cursor e manda "island-open") com os limites e as ações rápidas.
// Medidas iguais às de src-tauri/src/island.rs e src/styles/island.css.
import { providerSnapshot } from "../lib/provider";
import { invoke, isTauri, listen } from "../lib/ipc";
import { limitWindows, planLabel, serviceIssue } from "../lib/normalize";
import { ago, duration, esc } from "../lib/format";
import { providerMark } from "../lib/brand";
import type { LimitWindow, Settings, Snapshot } from "../lib/types";

// anel do estado fechado (viewBox 18×18)
const R = 7;
const C = 2 * Math.PI * R;
const CRIT = 90;

const svg = (body: string, box = 16) =>
  `<svg viewBox="0 0 ${box} ${box}" aria-hidden="true">${body}</svg>`;
const ICON = {
  swap: svg(`<path d="M3 5.5h9.5M10 3l2.5 2.5L10 8M13 10.5H3.5M6 8l-2.5 2.5L6 13" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>`),
  refresh: svg(`<path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>`),
  panel: svg(`<rect x="2.5" y="2.5" width="11" height="11" rx="2.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M2.5 6h11" stroke="currentColor" stroke-width="1.5"/>`),
  gear: svg(`<circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M3.6 12.4l1.1-1.1M11.3 4.7l1.1-1.1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>`),
};

const clamp = (n: number) => Math.min(100, Math.max(0, n));

export function mountIsland(root: HTMLElement, initial: Settings) {
  root.innerHTML = `
    <div class="isl" data-level="error" role="group" aria-label="Uso do Claude">
      <div class="isl-compact">
        <span class="isl-mark"></span>
        <span class="isl-val"><b>–</b><i>%</i></span>
        <svg class="isl-ring" viewBox="0 0 18 18" aria-hidden="true">
          <circle class="trk" cx="9" cy="9" r="${R}"/>
          <circle class="arc" cx="9" cy="9" r="${R}" stroke-dasharray="${C}" stroke-dashoffset="${C}"/>
        </svg>
        <span class="isl-dot" aria-hidden="true"></span>
      </div>
      <div class="isl-full" aria-hidden="true">
        <button class="isl-hd" data-act="panel" title="Abrir o painel">
          <span class="isl-mark isl-mark-lg"></span>
          <span class="isl-title"><b class="isl-name">Claude</b><small class="isl-sub"></small></span>
          <span class="isl-flag" hidden>Instável</span>
        </button>
        <div class="isl-rows"></div>
        <div class="isl-acts">
          <button data-act="provider" title="Alternar Claude / Codex">${ICON.swap}<span>Alternar</span></button>
          <button data-act="refresh" title="Atualizar agora">${ICON.refresh}<span>Atualizar</span></button>
          <button data-act="panel" title="Abrir o painel">${ICON.panel}<span>Painel</span></button>
          <button data-act="settings" title="Configurações">${ICON.gear}<span>Ajustes</span></button>
        </div>
      </div>
    </div>`;

  const isl = root.querySelector<HTMLElement>(".isl")!;
  const full = root.querySelector<HTMLElement>(".isl-full")!;
  const val = root.querySelector<HTMLElement>(".isl-val b")!;
  const arc = root.querySelector<SVGCircleElement>(".isl-ring .arc")!;
  const rows = root.querySelector<HTMLElement>(".isl-rows")!;
  const name = root.querySelector<HTMLElement>(".isl-name")!;
  const sub = root.querySelector<HTMLElement>(".isl-sub")!;
  const flag = root.querySelector<HTMLElement>(".isl-flag")!;

  let settings = initial;
  let last: Snapshot | null = null;

  function setEdge(edge: Settings["islandEdge"]) {
    isl.dataset.edge = edge;
  }

  function row(w: LimitWindow | undefined, label: string): string {
    if (!w) return "";
    const u = clamp(w.utilization);
    const left = w.resetsAt ? `em ${duration(w.resetsAt.getTime() - Date.now())}` : "sem reset";
    return `
      <div class="isl-row${u > CRIT ? " crit" : ""}">
        <div class="isl-row-hd"><span>${label}<em>${esc(left)}</em></span><b>${Math.floor(u)}%</b></div>
        <div class="isl-bar"><i style="width:${u}%"></i></div>
      </div>`;
  }

  function subtitle(s: Snapshot, codex: boolean): string {
    const plan = codex ? String(s.profile?.plan ?? "") : planLabel(s).plan;
    return [plan, s.updatedAt ? `atualizado ${ago(s.updatedAt)}` : ""].filter(Boolean).join(" · ");
  }

  function render(raw: Snapshot) {
    last = raw;
    const codex = settings.provider === "codex";
    const s = providerSnapshot(raw, settings.provider);
    const label = codex ? "Codex" : "Claude";
    root.querySelectorAll(".isl-mark").forEach((el) => (el.innerHTML = providerMark(settings.provider)));
    isl.setAttribute("aria-label", `Uso do ${label}`);
    name.textContent = label;
    sub.textContent = subtitle(s, codex);

    const issue = serviceIssue(s);
    isl.dataset.svc = issue?.severity ?? "";
    flag.hidden = !issue;
    flag.title = issue?.title ?? "";

    const wins = limitWindows(s.usage);
    const five = wins.find((x) => x.key === "five_hour");
    const week = wins.find((x) => x.key === "seven_day");
    const main = settings.widgetMetric === "seven_day" ? week ?? five : five ?? week;

    if (!main) {
      isl.dataset.level = "error";
      val.textContent = "–";
      arc.style.strokeDashoffset = String(C);
      rows.innerHTML = `<p class="isl-msg">${esc(s.error?.message ?? "Sem dados de uso ainda. Abra o painel para ver os detalhes.")}</p>`;
      return;
    }
    const u = clamp(main.utilization);
    isl.dataset.level = u > CRIT ? "crit" : "ok";
    val.textContent = String(Math.floor(u));
    arc.style.strokeDashoffset = String(C * (1 - u / 100));
    rows.innerHTML = row(five, "Sessão") + row(week, "Semana");
  }

  function setOpen(open: boolean) {
    if (open && last) render(last); // contagens regressivas em dia ao abrir
    isl.classList.toggle("open", open);
    full.setAttribute("aria-hidden", String(!open));
  }

  // ---------- abrir/fechar ----------
  listen<boolean>("island-open", setOpen);
  if (!isTauri) {
    // no navegador (dados de demonstração) o próprio hover abre a ilha
    isl.addEventListener("pointerenter", () => setOpen(true));
    isl.addEventListener("pointerleave", () => setOpen(false));
    if (new URLSearchParams(location.search).get("open") === "1") setOpen(true);
  }

  // ---------- ações ----------
  isl.addEventListener("click", (e) => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]")?.dataset.act;
    if (act === "provider") invoke("toggle_provider");
    else if (act === "refresh") invoke("refresh_now");
    else if (act === "panel") invoke("toggle_panel");
    else if (act === "settings") invoke("open_settings");
  });

  // ---------- dados ----------
  setEdge(settings.islandEdge);
  invoke<Snapshot>("get_snapshot").then(render);
  listen<Snapshot>("snapshot", render);
  listen<boolean>("refreshing", (b) => isl.classList.toggle("busy", b));
  listen<Settings>("settings", (st) => {
    settings = st;
    setEdge(st.islandEdge);
    if (last) render(last);
  });
  setInterval(() => {
    if (last && isl.classList.contains("open")) render(last);
  }, 30_000);
}
