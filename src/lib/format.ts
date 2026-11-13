const nf0 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });
const nf1 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 1 });
const usd = new Intl.NumberFormat("pt-BR", { style: "currency", currency: "USD", maximumFractionDigits: 2 });
const usd0 = new Intl.NumberFormat("pt-BR", { style: "currency", currency: "USD", maximumFractionDigits: 0 });

export const int = (n: number) => nf0.format(n);
export const pct = (n: number) => `${nf0.format(Math.floor(n))}%`;
export const money = (n: number) => (Math.abs(n) >= 1000 ? usd0.format(n) : usd.format(n));

/** 1234567 → "1,2 mi"; 12345 → "12,3 mil" */
export function compact(n: number): string {
  const a = Math.abs(n);
  if (a >= 1e9) return `${nf1.format(n / 1e9)} bi`;
  if (a >= 1e6) return `${nf1.format(n / 1e6)} mi`;
  if (a >= 1e3) return `${nf1.format(n / 1e3)} mil`;
  return nf0.format(n);
}

/** duração em ms → "2h 14min", "3d 4h", "45s" */
export function duration(ms: number): string {
  if (!isFinite(ms)) return "—";
  const s = Math.max(0, Math.round(ms / 1000));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return h ? `${d}d ${h}h` : `${d}d`;
  if (h > 0) return m ? `${h}h ${m}min` : `${h}h`;
  if (m > 0) return `${m}min`;
  return `${s}s`;
}

export function ago(ts: number | null | undefined, now = Date.now()): string {
  if (!ts) return "nunca";
  const diff = now - ts;
  if (diff < 10_000) return "agora";
  return `há ${duration(diff)}`;
}

const timeFmt = new Intl.DateTimeFormat("pt-BR", { hour: "2-digit", minute: "2-digit" });
const dayTimeFmt = new Intl.DateTimeFormat("pt-BR", { weekday: "short", hour: "2-digit", minute: "2-digit" });
const dateFmt = new Intl.DateTimeFormat("pt-BR", { day: "2-digit", month: "2-digit" });
const weekdayFmt = new Intl.DateTimeFormat("pt-BR", { weekday: "short" });

/** "às 16:00" se hoje, senão "sex 21:00" */
export function when(d: Date, now = new Date()): string {
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay ? `às ${timeFmt.format(d)}` : dayTimeFmt.format(d).replace(".", "").replace(",", "");
}

export const clock = (d: Date) => timeFmt.format(d);
export const shortDate = (d: Date) => dateFmt.format(d);
export const weekday = (d: Date) => weekdayFmt.format(d).replace(".", "");

export function esc(s: unknown): string {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

export type Level = "ok" | "warn" | "high" | "crit";

export function level(util: number): Level {
  if (util >= 95) return "crit";
  if (util >= 80) return "high";
  if (util >= 60) return "warn";
  return "ok";
}

export const levelLabel: Record<Level, string> = {
  ok: "Tranquilo",
  warn: "Atenção",
  high: "Alto",
  crit: "Crítico",
};

/** Ícones (SVG inline) para acompanhar a cor de status — nunca só a cor. */
export const levelIcon: Record<Level, string> = {
  ok: `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  warn: `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="M8 5v3.4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/><circle cx="8" cy="11" r="1" fill="currentColor"/></svg>`,
  high: `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 2.2l6 10.6H2z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/><path d="M8 6.4v3" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/><circle cx="8" cy="11.2" r=".95" fill="currentColor"/></svg>`,
  crit: `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M5.3 1.8h5.4l3.5 3.5v5.4l-3.5 3.5H5.3l-3.5-3.5V5.3z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M6 6l4 4M10 6l-4 4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>`,
};
