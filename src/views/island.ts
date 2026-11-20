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

/** "2h14", "45min", "3d4h": curto para caber na pílula (mesmo formato do círculo) */
function remaining(win: LimitWindow): string | null {
  if (!win.resetsAt) return null;
  const m = Math.max(0, Math.round((win.resetsAt.getTime() - Date.now()) / 60000));
  const d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), mm = m % 60;
  if (d > 0) return `${d}d${h ? `${h}h` : ""}`;
  if (h > 0) return `${h}h${String(mm).padStart(2, "0")}`;
  return `${mm}min`;
}

export function mountIsland(root: HTMLElement, initial: Settings) {
  root.innerHTML = `
    <div class="isl" data-level="error" role="group" aria-label="Uso do Claude">
      <div class="isl-compact">
        <span class="isl-mark"></span>
        <span class="isl-val">
          <span class="isl-num">
            <span class="isl-n isl-n-main"><b>–</b><i>%</i></span>
            <span class="isl-n isl-n-alt"><b>–</b><i>%</i></span>
            <span class="isl-n isl-n-time">–</span>
          </span>
          <span class="isl-lbl"><span class="isl-l-main">sessão</span><span class="isl-l-alt">semana</span><span class="isl-l-time">restam</span></span>
        </span>
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
  const nMain = root.querySelector<HTMLElement>(".isl-n-main b")!;
  const nAlt = root.querySelector<HTMLElement>(".isl-n-alt b")!;
  const nTime = root.querySelector<HTMLElement>(".isl-n-time")!;
  const lMain = root.querySelector<HTMLElement>(".isl-l-main")!;
  const lAlt = root.querySelector<HTMLElement>(".isl-l-alt")!;
  const arc = root.querySelector<SVGCircleElement>(".isl-ring .arc")!;
  const rows = root.querySelector<HTMLElement>(".isl-rows")!;
  const name = root.querySelector<HTMLElement>(".isl-name")!;
  const sub = root.querySelector<HTMLElement>(".isl-sub")!;
  const flag = root.querySelector<HTMLElement>(".isl-flag")!;

  let settings = initial;
  let last: Snapshot | null = null;
  // janela principal (a da configuração "Mostra") e a outra (sessão ⇄ semana)
  let main: LimitWindow | undefined;
  let other: LimitWindow | undefined;

  function setEdge(edge: Settings["islandEdge"]) {
    isl.dataset.edge = edge;
  }

  /** Aponta o anel (arco e cor) para a janela exibida. */
  function aim(win: LimitWindow | undefined) {
    const u = clamp(win?.utilization ?? 0);
    isl.dataset.level = win ? (u > CRIT ? "crit" : "ok") : "error";
    arc.style.strokeDashoffset = String(C * (1 - u / 100));
  }

  // ---------- rotação, como no círculo: % principal → tempo restante → % da outra janela ----------
  type Face = "main" | "time" | "alt";
  const FACES: Face[] = ["main", "time", "alt"];
  const FACE_MS: Record<Face, number> = { main: 7000, time: 3500, alt: 3500 };
  let face: Face = "main";
  let rotTimer = 0;

  const available = (f: Face) => f === "main" || (f === "time" ? !!main && !!remaining(main) : !!main && !!other);

  function showFace(f: Face) {
    face = available(f) ? f : "main";
    if (face === "time" && main) nTime.textContent = remaining(main) ?? "–";
    isl.classList.toggle("time", face === "time");
    isl.classList.toggle("alt", face === "alt");
    aim(face === "alt" ? other : main);
  }

  function rotate() {
    clearTimeout(rotTimer);
    const at = FACES.indexOf(face);
    const next = [1, 2, 3].map((k) => FACES[(at + k) % FACES.length]).find(available) ?? "main";
    showFace(next);
    rotTimer = window.setTimeout(rotate, FACE_MS[next]);
  }

  /** Volta ao % principal e recomeça o ciclo (ao fechar a ilha ou trocar de IA/métrica). */
  function restartRotation() {
    clearTimeout(rotTimer);
    showFace("main");
    rotTimer = window.setTimeout(rotate, FACE_MS.main);
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
    main = settings.widgetMetric === "seven_day" ? week ?? five : five ?? week;
    other = main === five ? week : five;

    if (!main) {
      main = other = undefined;
      nMain.textContent = nAlt.textContent = "–";
      showFace("main");
      rows.innerHTML = `<p class="isl-msg">${esc(s.error?.message ?? "Sem dados de uso ainda. Abra o painel para ver os detalhes.")}</p>`;
      return;
    }
    const winLabel = (w: LimitWindow | undefined) => (w?.key === "seven_day" ? "semana" : "sessão");
    nMain.textContent = String(Math.floor(clamp(main.utilization)));
    nAlt.textContent = other ? String(Math.floor(clamp(other.utilization))) : "–";
    lMain.textContent = winLabel(main);
    lAlt.textContent = winLabel(other);
    showFace(face); // mantém o que está na tela, com os números novos
    rows.innerHTML = row(five, "Sessão") + row(week, "Semana");
  }

  function setOpen(open: boolean) {
    if (open && last) render(last); // contagens regressivas em dia ao abrir
    isl.classList.toggle("open", open);
    full.setAttribute("aria-hidden", String(!open));
    // aberta, a pílula compacta some: pausa o ciclo; ao fechar, recomeça do % principal
    if (open) clearTimeout(rotTimer);
    else restartRotation();
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
    const switched = st.provider !== settings.provider || st.widgetMetric !== settings.widgetMetric;
    settings = st;
    setEdge(st.islandEdge);
    if (last) render(last);
    // nova IA/métrica começa no %, com um ciclo próprio (como no círculo)
    if (switched) restartRotation();
  });
  restartRotation();
  setInterval(() => {
    if (last && isl.classList.contains("open")) render(last);
  }, 30_000);
}
