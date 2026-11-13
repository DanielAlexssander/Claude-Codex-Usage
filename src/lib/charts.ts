// Gráficos leves em SVG/HTML puro (sem bibliotecas).
import type { DayPoint, HistPoint, ModelRow } from "./types";
import { compact, money, int, shortDate, weekday, clock, esc } from "./format";

const lvlVar: Record<string, string> = {
  ok: "var(--accent)",
  warn: "var(--warning)",
  high: "var(--serious)",
  crit: "var(--critical)",
};

/** Medidor circular grande (270°). `pace` = fração do tempo decorrido (0..1). */
export function gauge(util: number, lvl: string, pace: number | null, label: string): string {
  const size = 128, cx = 64, cy = 64, r = 54, sw = 11;
  const arcLen = 2 * Math.PI * r * 0.75;
  const full = 2 * Math.PI * r;
  const u = Math.max(0, Math.min(100, util)) / 100;
  const off = arcLen * (1 - u);
  let paceMark = "";
  if (pace != null) {
    const ang = (135 + 270 * pace) * (Math.PI / 180);
    const r1 = r - sw / 2 - 3, r2 = r + sw / 2 + 3;
    paceMark = `<line class="g-pace" x1="${cx + r1 * Math.cos(ang)}" y1="${cy + r1 * Math.sin(ang)}" x2="${cx + r2 * Math.cos(ang)}" y2="${cy + r2 * Math.sin(ang)}"><title>Ritmo linear: ${Math.floor(pace * 100)}% do tempo da janela já passou</title></line>`;
  }
  return `
  <div class="gauge">
    <svg viewBox="0 0 ${size} ${size}" role="img" aria-label="${esc(label)}: ${Math.floor(util)}%">
      <defs>
        <linearGradient id="g-grad" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0" style="stop-color:${lvl === "ok" ? "var(--accent-2)" : lvlVar[lvl]}"/>
          <stop offset="1" style="stop-color:${lvlVar[lvl] ?? "var(--accent)"}"/>
        </linearGradient>
      </defs>
      <circle class="g-track" cx="${cx}" cy="${cy}" r="${r}" stroke-width="${sw}"
        stroke-dasharray="${arcLen} ${full}" transform="rotate(135 ${cx} ${cy})" stroke-linecap="round"/>
      <circle class="g-arc" cx="${cx}" cy="${cy}" r="${r}" stroke-width="${sw}" stroke="url(#g-grad)"
        stroke-dasharray="${arcLen} ${full}" stroke-dashoffset="${off}" style="--full:${arcLen}"
        transform="rotate(135 ${cx} ${cy})"/>
      ${paceMark}
    </svg>
    <div class="g-center"><b>${Math.floor(util)}<small>%</small></b><span>${esc(label)}</span></div>
  </div>`;
}

/** Linha do % da sessão ao longo da janela atual. */
export function sparkline(points: HistPoint[], start: number, end: number): string {
  const pts = points.filter((p) => p.ts >= start && typeof p.fiveHour === "number");
  if (pts.length < 2) return "";
  const W = 200, H = 30;
  const x = (t: number) => ((t - start) / (end - start)) * W;
  const y = (v: number) => H - (Math.min(100, v) / 100) * H;
  const d = pts.map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)},${y(p.fiveHour as number).toFixed(1)}`).join(" ");
  const last = pts[pts.length - 1];
  const area = `${d} L${x(last.ts).toFixed(1)},${H} L${x(pts[0].ts).toFixed(1)},${H} Z`;
  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" aria-hidden="true">
    <defs><linearGradient id="spark-fill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--accent)" stop-opacity=".22"/><stop offset="1" style="stop-color:var(--accent)" stop-opacity="0"/>
    </linearGradient></defs>
    <path class="sa" d="${area}"/>
    <path class="sl" d="${d}" vector-effect="non-scaling-stroke"/>
    <circle class="sd" cx="${x(last.ts)}" cy="${y(last.fiveHour as number)}" r="3.5" vector-effect="non-scaling-stroke"/>
  </svg>`;
}

/** Barras diárias (tokens). */
export function dailyBars(days: DayPoint[]): string {
  if (!days.length) return "";
  const max = Math.max(1, ...days.map((d) => d.total));
  const today = days[days.length - 1].date;
  const bars = days
    .map((d, k) => {
      const dt = new Date(`${d.date}T12:00:00`);
      const h = d.total > 0 ? Math.max(3, (d.total / max) * 100) : 2;
      const tip = `<b>${weekday(dt)} ${shortDate(dt)}</b><br>${compact(d.total)} tokens · ${int(d.messages)} req.<br>${money(d.cost)} eq. API`;
      return `<div class="b ${d.date === today ? "today" : ""} ${d.total === 0 ? "zero" : ""}" style="--k:${k}" data-tip="${esc(tip)}"><i style="height:${h}%"></i></div>`;
    })
    .join("");
  const first = new Date(`${days[0].date}T12:00:00`);
  const mid = new Date(`${days[Math.floor(days.length / 2)].date}T12:00:00`);
  return `<div class="bars">${bars}</div>
    <div class="axis"><span>${shortDate(first)}</span><span>${shortDate(mid)}</span><span>hoje</span></div>`;
}

/** Mapa de calor dia-da-semana × hora (nº de requisições). */
export function heatmap(grid: number[][]): string {
  if (!grid?.length) return "";
  const max = Math.max(1, ...grid.flat());
  const days = ["Seg", "Ter", "Qua", "Qui", "Sex", "Sáb", "Dom"];
  let html = `<div class="heat">`;
  grid.forEach((row, wd) => {
    html += `<span class="hl">${days[wd]}</span>`;
    row.forEach((v, h) => {
      const k = v === 0 ? 0 : 12 + Math.round((v / max) * 88);
      const tip = `<b>${days[wd]} ${String(h).padStart(2, "0")}h</b><br>${int(v)} requisições`;
      html += `<i class="c" style="background:color-mix(in oklab, var(--seq-hi) ${k}%, var(--seq-lo))" data-tip="${esc(tip)}"></i>`;
    });
  });
  html += `<span></span>`;
  for (let h = 0; h < 24; h++) html += `<span class="hx">${h % 6 === 0 ? h : ""}</span>`;
  html += `</div><div class="heat-legend">menos <i></i> mais</div>`;
  return html;
}

export const TOKEN_KINDS = [
  { key: "input", label: "Entrada", cls: "s1" },
  { key: "output", label: "Saída", cls: "s2" },
  { key: "cacheWrite", label: "Cache (escrita)", cls: "s3" },
  { key: "cacheRead", label: "Cache (leitura)", cls: "s4" },
] as const;

export function tokenLegend(): string {
  return `<div class="legend">${TOKEN_KINDS.map((k) => `<span><i class="${k.cls}"></i>${k.label}</span>`).join("")}</div>`;
}

/** Barra empilhada por tipo de token; largura relativa ao maior modelo. */
export function stackBar(m: ModelRow, maxTotal: number): string {
  const width = Math.max(4, (m.total / Math.max(1, maxTotal)) * 100);
  const segs = TOKEN_KINDS.map((k) => {
    const v = m[k.key];
    if (!v) return "";
    const tip = `<b>${esc(m.display)} · ${k.label}</b><br>${compact(v)} tokens (${((v / m.total) * 100).toFixed(1)}%)`;
    return `<i class="${k.cls}" style="flex:${v}" data-tip="${esc(tip)}"></i>`;
  }).join("");
  return `<div class="stack" style="width:${width}%">${segs}</div>`;
}

export { clock };
