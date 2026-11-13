import { providerSnapshot } from "../lib/provider";
import { invoke, listen, startDragging } from "../lib/ipc";
import { limitWindows, serviceIssue } from "../lib/normalize";
import { when, pct } from "../lib/format";
import { claudeMark, providerMark } from "../lib/brand";
import type { LimitWindow, Settings, Snapshot } from "../lib/types";

// Geometria do anel (viewBox 72×72)
const R = 33;
const C = 2 * Math.PI * R;

export function mountWidget(root: HTMLElement) {
  root.innerHTML = `
    <div class="w" data-level="ok" role="button" aria-label="Uso do Claude — clique para abrir">
      <div class="w-halo"></div>
      <div class="w-busy"></div>
      <svg class="w-ring" viewBox="0 0 72 72" aria-hidden="true">
        <circle class="w-track" cx="36" cy="36" r="${R}"/>
        <circle class="w-arc" cx="36" cy="36" r="${R}" stroke-dasharray="${C}" stroke-dashoffset="${C}"/>
        <g class="w-spark"><circle cx="${36 + R}" cy="36" r="2.4"/></g>
      </svg>
      <div class="w-core">
        <div class="w-stack">
          <div class="w-brand"><span class="w-cc">${claudeMark()}</span></div>
          <div class="w-num"><span class="w-n w-n-main"><span class="v">–</span><i>%</i></span><span class="w-n w-n-alt"><span class="v">–</span><i>%</i></span><span class="w-n w-n-time">–</span></div>
          <div class="w-lbl"><span class="w-l-main">sessão</span><span class="w-l-alt">semana</span><span class="w-l-time">restam</span></div>
        </div>
        <div class="w-err">!</div>
      </div>
      <span class="w-svc" aria-hidden="true"></span>
    </div>`;

  const w = root.querySelector<HTMLElement>(".w")!;
  const arc = root.querySelector<SVGCircleElement>(".w-arc")!;
  const spark = root.querySelector<SVGGElement>(".w-spark")!;
  const nMain = root.querySelector<HTMLElement>(".w-n-main .v")!;
  const nAlt = root.querySelector<HTMLElement>(".w-n-alt .v")!;
  const lMain = root.querySelector<HTMLElement>(".w-l-main")!;
  const lAlt = root.querySelector<HTMLElement>(".w-l-alt")!;
  const nTime = root.querySelector<HTMLElement>(".w-n-time")!;

  let provider: Settings["provider"] = "claude";
  let metric: Settings["widgetMetric"] = "five_hour";
  let current: number | null = null;
  let animation = 0;
  let last: Snapshot | null = null;
  let main: LimitWindow | undefined;
  let other: LimitWindow | undefined;
  let hovering = false;

  function countTo(el: HTMLElement, from: number, to: number) {
    cancelAnimationFrame(animation);
    const t0 = performance.now();
    const dur = 900;
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / dur);
      const e = 1 - Math.pow(1 - k, 3);
      el.textContent = String(Math.round(from + (to - from) * e));
      if (k < 1) animation = requestAnimationFrame(step);
    };
    animation = requestAnimationFrame(step);
  }

  /** Aponta o anel (arco, cor e ponto) para uma janela de limite. */
  function aimRing(win: LimitWindow | undefined) {
    const u = Math.min(100, Math.max(0, win?.utilization ?? 0));
    w.dataset.level = win ? (u > 90 ? "crit" : "ok") : "error";
    arc.style.strokeDashoffset = String(C * (1 - u / 100));
    spark.style.transform = `rotate(${(u / 100) * 360}deg)`;
  }

  function render(s: Snapshot) {
    last = s;
    s = providerSnapshot(s, provider);
    root.querySelector(".w-cc")!.innerHTML = providerMark(provider);
    w.setAttribute("aria-label", `Uso do ${provider === "codex" ? "Codex" : "Claude"} — clique para abrir`);
    // ponto de aviso quando o próprio Claude está instável (status.claude.com)
    const issue = serviceIssue(s);
    w.dataset.svc = issue?.severity ?? "";
    const wins = limitWindows(s.usage);
    const five = wins.find((x) => x.key === "five_hour");
    const week = wins.find((x) => x.key === "seven_day");
    main = metric === "seven_day" ? week ?? five : five ?? week;
    other = main === five ? week : five;

    if (!main) {
      main = other = undefined;
      cancelAnimationFrame(animation);
      current = null;
      nMain.textContent = nAlt.textContent = "–";
      w.classList.remove("has-alt", "alt", "time");
      aimRing(undefined);
      w.title = s.error?.message ?? "Sem dados de uso ainda — abra o painel.";
      return;
    }
    const prevLevel = w.dataset.level;
    aimRing(hovering && other ? other : main);

    lMain.textContent = main.key === "five_hour" ? "sessão" : "semana";
    lAlt.textContent = other?.key === "five_hour" ? "sessão" : "semana";
    nAlt.textContent = other ? String(Math.floor(other.utilization)) : "–";
    w.classList.toggle("has-alt", !!other);

    const displayed = Math.floor(main.utilization);
    if (current === null || displayed === 0) {
      // Uma nova fonte ou janela zerada deve substituir o texto anterior imediatamente.
      cancelAnimationFrame(animation);
      nMain.textContent = String(displayed);
      current = displayed;
    } else if (displayed !== current) {
      countTo(nMain, current, displayed);
      if (nMain.textContent !== "–" && prevLevel !== "error") {
        w.classList.remove("bump");
        void w.offsetWidth;
        w.classList.add("bump");
      }
      current = displayed;
    }

    const lines = [`${main.label}: ${pct(main.utilization)}` + (main.resetsAt ? ` · reinicia ${when(main.resetsAt)}` : "")];
    if (other) lines.push(`${other.label}: ${pct(other.utilization)}` + (other.resetsAt ? ` · reinicia ${when(other.resetsAt)}` : ""));
    if (s.error) lines.push(`⚠ ${s.error.message}`);
    if (issue) lines.push(`⚠ ${issue.title}${s.service?.incidents[0] ? `: ${s.service.incidents[0].name}` : ""}`);
    lines.push("Clique para detalhes · arraste para mover");
    w.title = lines.join("\n");
  }

  // ---------- rotação: alterna "% sessão" ⇄ "tempo restante da janela" ----------
  const SHOW_PCT_MS = 7000;
  const SHOW_TIME_MS = 3500;
  let rotTimer = 0;

  /** "2h14", "45min", "3d4h" — curto para caber no disco */
  function remaining(win: LimitWindow): string | null {
    if (!win.resetsAt) return null;
    const m = Math.max(0, Math.round((win.resetsAt.getTime() - Date.now()) / 60000));
    const d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), mm = m % 60;
    if (d > 0) return `${d}d${h ? `${h}h` : ""}`;
    if (h > 0) return `${h}h${String(mm).padStart(2, "0")}`;
    return `${mm}min`;
  }

  function rotate(showTime: boolean) {
    clearTimeout(rotTimer);
    const txt = main ? remaining(main) : null;
    const canShow = showTime && !!txt && !hovering && w.dataset.level !== "error";
    if (canShow) nTime.textContent = txt!;
    w.classList.toggle("time", canShow);
    rotTimer = window.setTimeout(() => rotate(!canShow), canShow ? SHOW_TIME_MS : SHOW_PCT_MS);
  }
  rotTimer = window.setTimeout(() => rotate(true), SHOW_PCT_MS);

  // ---------- hover: mostra a outra janela (anel + número deslizam) ----------
  w.addEventListener("pointerenter", () => {
    hovering = true;
    w.classList.remove("time");
    if (other) {
      w.classList.add("alt");
      aimRing(other);
    }
  });
  w.addEventListener("pointerleave", () => {
    hovering = false;
    w.classList.remove("alt");
    if (main) aimRing(main);
  });

  // ---------- clique × arrasto ----------
  let press: { x: number; y: number } | null = null;
  let dragging = false;
  w.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    press = { x: e.screenX, y: e.screenY };
    dragging = false;
  });
  w.addEventListener("pointermove", (e) => {
    if (!press || dragging) return;
    if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 4) {
      dragging = true;
      invoke("hide_panel");
      startDragging();
    }
  });
  w.addEventListener("pointerup", () => {
    if (press && !dragging) invoke("toggle_panel");
    press = null;
    dragging = false;
  });
  window.addEventListener("blur", () => {
    press = null;
    dragging = false;
  });
  w.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    invoke("toggle_panel");
  });

  // ---------- dados ----------
  invoke<Settings>("get_settings").then((st) => {
    provider = st.provider;
    metric = st.widgetMetric;
    invoke<Snapshot>("get_snapshot").then(render);
  });
  listen<Snapshot>("snapshot", render);
  listen<boolean>("refreshing", (b) => w.classList.toggle("busy", b));
  listen<boolean>("panel-visibility", (open) => w.classList.toggle("open", open));
  listen<Settings>("settings", (st) => {
    if (st.widgetMetric !== metric || st.provider !== provider) {
      provider = st.provider;
      metric = st.widgetMetric;
      cancelAnimationFrame(animation);
      current = null;
      if (last) render(last);
      // A nova IA/métrica começa no percentual, com um ciclo de tempo próprio.
      nTime.textContent = "–";
      rotate(false);
    }
  });
}
