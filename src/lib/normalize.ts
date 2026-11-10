import type { HistPoint, LimitWindow, Snapshot, UsageRaw } from "./types";

const H = 3600_000;
const KNOWN: Record<string, { label: string; short: string; period: number; order: number }> = {
  five_hour: { label: "Sessão atual", short: "5h", period: 5 * H, order: 0 },
  seven_day: { label: "Semanal · todos os modelos", short: "7d", period: 168 * H, order: 1 },
  seven_day_opus: { label: "Semanal · Opus", short: "Opus", period: 168 * H, order: 2 },
  seven_day_sonnet: { label: "Semanal · Sonnet", short: "Sonnet", period: 168 * H, order: 3 },
  seven_day_oauth_apps: { label: "Semanal · apps conectados", short: "Apps", period: 168 * H, order: 4 },
  seven_day_cowork: { label: "Semanal · Cowork", short: "Cowork", period: 168 * H, order: 5 },
  seven_day_routines: { label: "Semanal · rotinas", short: "Rotinas", period: 168 * H, order: 6 },
};

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

function humanize(key: string): { label: string; short: string; period: number | null } {
  if (key.startsWith("seven_day_")) {
    const rest = key.slice("seven_day_".length).replace(/_/g, " ");
    return { label: `Semanal · ${cap(rest)}`, short: cap(rest), period: 168 * H };
  }
  if (key.startsWith("five_hour_")) {
    const rest = key.slice("five_hour_".length).replace(/_/g, " ");
    return { label: `Sessão · ${cap(rest)}`, short: cap(rest), period: 5 * H };
  }
  const nice = cap(key.replace(/_/g, " "));
  return { label: nice, short: nice, period: null };
}

/** Converte o payload de /api/oauth/usage numa lista ordenada de janelas de limite. */
export function limitWindows(usage: UsageRaw | null | undefined): LimitWindow[] {
  if (!usage) return [];
  const out: (LimitWindow & { order: number })[] = [];
  for (const [key, val] of Object.entries(usage)) {
    if (!val || typeof val !== "object" || Array.isArray(val)) continue;
    const v = val as Record<string, unknown>;
    if (typeof v.utilization !== "number") continue;
    if (key === "extra_usage") continue;
    const meta = KNOWN[key] ?? { ...humanize(key), order: 50 };
    out.push({
      key,
      label: meta.label,
      short: meta.short,
      utilization: Math.max(0, v.utilization),
      resetsAt: typeof v.resets_at === "string" ? new Date(v.resets_at) : null,
      periodMs: meta.period,
      order: meta.order,
      scoped: key.startsWith("seven_day_") || key.startsWith("five_hour_"),
    });
  }
  // Formato novo: { limits: [{ kind, percent, resets_at, scope: { model: { display_name } } }] }
  //  · kind "session" / "weekly_all" repetem five_hour / seven_day (só usados se faltarem no topo)
  //  · kind "weekly_scoped" = limite semanal próprio de um modelo/superfície (ex.: Fable)
  //  · weekly_scoped_shares[] diz quanto do limite semanal geral esse modelo pode consumir
  const limits = (usage as Record<string, unknown>).limits;
  const shares = (usage as Record<string, unknown>).weekly_scoped_shares;
  if (Array.isArray(limits)) {
    limits.forEach((l: any, i) => {
      if (!l || typeof l !== "object") return;
      const util = typeof l.percent === "number" ? l.percent : typeof l.utilization === "number" ? l.utilization : null;
      if (util == null) return;
      const resetsAt = typeof l.resets_at === "string" ? new Date(l.resets_at) : null;
      if (l.kind === "session" || l.kind === "weekly_all") {
        const key = l.kind === "session" ? "five_hour" : "seven_day";
        if (out.some((o) => o.key === key)) return;
        const meta = KNOWN[key];
        out.push({ key, label: meta.label, short: meta.short, utilization: util, resetsAt, periodMs: meta.period, order: meta.order });
        return;
      }
      const scopeName =
        l.scope?.model?.display_name ?? l.scope?.surface?.display_name ?? l.scope?.model?.id ?? l.display_name ?? l.name ?? null;
      const name = String(scopeName ?? `limite ${i + 1}`).replace(/_/g, " ");
      // já veio como seven_day_<nome> no topo?
      if (out.some((o) => o.short.toLowerCase() === name.toLowerCase())) return;
      let note: string | undefined;
      if (Array.isArray(shares)) {
        const sh = (shares as any[]).find((x) => x?.limit_index === i);
        if (sh && typeof sh.allowance_percent_of_weekly === "number") {
          note = `pode usar até ${sh.allowance_percent_of_weekly}% do semanal` +
            (typeof sh.used_percent_of_weekly === "number" ? ` · usou ${sh.used_percent_of_weekly}% dele` : "");
        }
      }
      const weekly = l.group === "weekly" || String(l.kind ?? "").startsWith("weekly");
      out.push({
        key: `scoped:${name.toLowerCase()}`,
        label: `${weekly ? "Semanal" : "Limite"} · ${cap(name)}`,
        short: cap(name),
        utilization: util,
        resetsAt,
        periodMs: weekly ? 168 * H : null,
        order: 10 + i,
        scoped: true,
        note,
      });
    });
  }
  return out.sort((a, b) => a.order - b.order);
}

export interface ExtraUsage {
  enabled: boolean;
  limit: number | null;
  used: number | null;
  utilization: number | null;
}

/**
 * Uso extra (créditos pagos além do limite do plano).
 *  · `extra_usage`: valores em centavos (formato antigo)
 *  · `spend`: { enabled, used: { amount_minor, exponent }, limit, percent } (formato novo)
 */
export function extraUsage(usage: UsageRaw | null | undefined): ExtraUsage | null {
  const e = usage?.extra_usage;
  const sp = (usage as any)?.spend;
  if (e && typeof e === "object" && (e.is_enabled || typeof e.used_credits === "number")) {
    const limit = typeof e.monthly_limit === "number" ? e.monthly_limit / 100 : null;
    const used = typeof e.used_credits === "number" ? e.used_credits / 100 : null;
    let utilization = typeof e.utilization === "number" ? e.utilization : null;
    if (utilization == null && limit && used != null) utilization = (used / limit) * 100;
    return { enabled: !!e.is_enabled, limit, used, utilization };
  }
  if (sp && typeof sp === "object") {
    const money = (m: any): number | null =>
      m && typeof m === "object" && typeof m.amount_minor === "number"
        ? m.amount_minor / Math.pow(10, typeof m.exponent === "number" ? m.exponent : 2)
        : typeof m === "number" ? m : null;
    const used = money(sp.used);
    const limit = money(sp.limit) ?? money(sp.cap);
    const utilization = typeof sp.percent === "number" ? sp.percent : limit && used != null ? (used / limit) * 100 : null;
    return { enabled: !!sp.enabled, limit, used, utilization };
  }
  return null;
}

/** Fração do período já decorrida (0..1) — usada para o marcador de "ritmo linear". */
export function elapsedFraction(w: LimitWindow, now = Date.now()): number | null {
  if (!w.resetsAt || !w.periodMs) return null;
  const remaining = w.resetsAt.getTime() - now;
  return Math.min(1, Math.max(0, 1 - remaining / w.periodMs));
}

const DAY = 24 * H;
const isWeekday = (t: number) => {
  const d = new Date(t).getDay();
  return d !== 0 && d !== 6;
};
const startOfDay = (t: number) => {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
};
const nextDay = (t: number) => {
  const d = new Date(startOfDay(t));
  d.setDate(d.getDate() + 1); // respeita horário de verão
  return d.getTime();
};

/** Tempo em dias úteis (seg–sex, hora local) entre `a` e `b`; fim de semana não conta. */
export function businessDaysBetween(a: number, b: number): number {
  let total = 0;
  for (let t = a; t < b; ) {
    const end = Math.min(b, nextDay(t));
    if (isWeekday(t)) total += (end - t) / DAY;
    t = end;
  }
  return total;
}

export interface WeeklyPace {
  /** quanto do limite ainda dá para usar por dia útil, dividindo igualmente até o reset */
  perDay: number | null;
  /** dias úteis (inteiros, contando hoje) com tempo antes do reset */
  daysLeft: number;
  /** % que deveria ter sido usado até agora num ritmo constante por dia útil */
  expected: number;
  /** fração do período (em dias úteis) que já passou — posição do marcador na barra */
  fraction: number;
  over: boolean;
}

/**
 * Ritmo da janela semanal contando só dias úteis: no fim de semana o "relógio" do ritmo
 * para, então usar menos no sábado/domingo não deixa você "adiantado" à toa.
 */
export function weeklyPace(w: LimitWindow, now = Date.now()): WeeklyPace | null {
  if (!w.resetsAt || !w.periodMs || w.periodMs < 2 * DAY) return null;
  const end = w.resetsAt.getTime();
  if (end <= now) return null;
  const start = end - w.periodMs;
  const total = businessDaysBetween(start, end);
  if (total <= 0) return null;
  const fraction = Math.min(1, Math.max(0, businessDaysBetween(start, Math.min(now, end)) / total));
  let daysLeft = 0;
  // hoje sempre conta; um dia futuro só conta se tiver ao menos 8h antes do reset
  // (reset na segunda 02:00 não dá um "dia útil" na segunda)
  for (let t = startOfDay(now); t < end; t = nextDay(t)) {
    if (!isWeekday(t)) continue;
    if (t <= now || Math.min(end, nextDay(t)) - t >= 8 * H) daysLeft++;
  }
  const remaining = Math.max(0, 100 - w.utilization);
  const expected = fraction * 100;
  return {
    perDay: daysLeft > 0 ? remaining / daysLeft : null,
    daysLeft,
    expected,
    fraction,
    over: fraction > 0.03 && w.utilization > expected + 5,
  };
}

export interface Projection {
  ratePerHour: number; // pontos percentuais por hora
  hitsAt: Date | null; // quando chega a 100% no ritmo atual (se antes do reset)
}

/** Projeção da janela de 5h a partir do histórico de amostras. */
export function projectSession(history: HistPoint[], w: LimitWindow | undefined, now = Date.now()): Projection | null {
  if (!w || !w.resetsAt) return null;
  const start = w.resetsAt.getTime() - 5 * H;
  const pts = history.filter((p) => p.ts >= Math.max(start, now - 90 * 60_000) && typeof p.fiveHour === "number");
  let rate: number;
  if (pts.length >= 2) {
    const a = pts[0], b = pts[pts.length - 1];
    const dtH = (b.ts - a.ts) / H;
    if (dtH < 0.08) return null;
    rate = ((b.fiveHour as number) - (a.fiveHour as number)) / dtH;
  } else {
    const elapsedH = (now - start) / H;
    if (elapsedH < 0.25) return null;
    rate = w.utilization / elapsedH;
  }
  if (!isFinite(rate)) return null;
  let hitsAt: Date | null = null;
  if (rate > 0.5 && w.utilization < 100) {
    const t = now + ((100 - w.utilization) / rate) * H;
    if (t < w.resetsAt.getTime()) hitsAt = new Date(t);
  }
  return { ratePerHour: rate, hitsAt };
}

export function planLabel(s: Snapshot): { plan: string; tier: string | null } {
  const org = (s.profile?.organization ?? {}) as Record<string, any>;
  const raw = String(s.creds?.subscriptionType ?? org.organization_type ?? org.billing_type ?? "").toLowerCase();
  const tierRaw = String(s.creds?.rateLimitTier ?? org.rate_limit_tier ?? "").toLowerCase();
  let plan = "Claude";
  if (raw.includes("enterprise")) plan = "Enterprise";
  else if (raw.includes("team")) plan = "Team";
  else if (raw.includes("max")) plan = "Max";
  else if (raw.includes("pro")) plan = "Pro";
  else if (raw) plan = cap(raw.replace(/^claude_/, "").replace(/_/g, " "));
  let tier: string | null = null;
  const isOrg = plan === "Team" || plan === "Enterprise";
  if (tierRaw.includes("20x")) tier = isOrg ? "seat 20×" : "20×";
  else if (tierRaw.includes("5x")) tier = isOrg ? "seat Premium" : "5×";
  else if (tierRaw && isOrg) tier = "seat Standard";
  return { plan, tier };
}

export function accountInfo(s: Snapshot): { name: string; email: string; org: string } {
  const acc = (s.profile?.account ?? {}) as Record<string, any>;
  const org = (s.profile?.organization ?? {}) as Record<string, any>;
  return {
    name: String(acc.full_name ?? acc.display_name ?? acc.name ?? ""),
    email: String(acc.email ?? acc.email_address ?? ""),
    org: String(org.name ?? ""),
  };
}

// ------------------------------------------------------------ status.claude.com
const INCIDENT_STATUS: Record<string, string> = {
  investigating: "investigando",
  identified: "causa identificada",
  monitoring: "corrigido · monitorando",
  in_progress: "em andamento",
  scheduled: "agendado",
  verifying: "verificando",
};
const COMPONENT_STATUS: Record<string, string> = {
  degraded_performance: "lento",
  partial_outage: "falha parcial",
  major_outage: "fora do ar",
  under_maintenance: "em manutenção",
};
export const incidentStatus = (s: string) => INCIDENT_STATUS[s] ?? s.replace(/_/g, " ");
export const componentStatus = (s: string) => COMPONENT_STATUS[s] ?? s.replace(/_/g, " ");

/** Situação do Claude para a interface: null = tudo normal (ou consulta desligada). */
export function serviceIssue(s: Snapshot): { severity: "minor" | "major" | "maintenance"; title: string } | null {
  const v = s.service;
  if (!v) return null;
  const degraded = v.incidents.length > 0 || v.components.length > 0 || !["", "none"].includes(v.indicator);
  if (!degraded) return null;
  const worst = [v.indicator, ...v.incidents.map((i) => i.impact)];
  const outage = v.components.some((c) => c.status === "major_outage" || c.status === "partial_outage");
  if (worst.some((x) => x === "major" || x === "critical") || outage) return { severity: "major", title: "Instabilidade no Claude" };
  if (v.indicator === "maintenance" || (v.incidents.length === 0 && v.components.every((c) => c.status === "under_maintenance")))
    return { severity: "maintenance", title: "Manutenção no Claude" };
  return { severity: "minor", title: "Lentidão ou erros no Claude" };
}
