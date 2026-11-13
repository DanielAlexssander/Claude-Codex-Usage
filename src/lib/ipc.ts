// Ponte com o back-end Rust. Fora do Tauri (ex.: `npm run dev` no navegador)
// usa dados de demonstração, o que facilita ajustar o visual.
import type { Settings, Snapshot } from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Unlisten = () => void;

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockInvoke(cmd, args) as Promise<T>;
}

export async function listen<T>(event: string, cb: (payload: T) => void): Promise<Unlisten> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, (e) => cb(e.payload));
  }
  const handler = (e: Event) => cb((e as CustomEvent).detail as T);
  window.addEventListener(`mock:${event}`, handler);
  return () => window.removeEventListener(`mock:${event}`, handler);
}

export async function startDragging() {
  if (!isTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().startDragging();
}

export async function openUrl(url: string) {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank");
  }
}

// ---------------------------------------------------------------- mock
let mockSettings: Settings = {
  provider: "claude",
  providerShortcut: "PageUp",
  refreshSecs: 180,
  widgetVisible: true,
  widgetPos: null,
  theme: "system",
  notifications: true,
  thresholds: [75, 90],
  widgetMetric: "five_hour",
  reduceMotion: false,
  initialized: true,
  autoRenew: true,
  notifyReset: true,
  shortcut: true,
  hideFullscreen: true,
  hideOnShare: true,
  serviceStatus: true,
  shortcutKeys: "Control+Alt+U",
  shortcutFailed: new URLSearchParams(location.search).get("shortcutfail") === "1",
  autostart: true,
  pinned: false,
  platform: "windows",
};

function mockClaudeSnapshot(): Snapshot {
  const now = Date.now();
  const h = 3600_000;
  const q = new URLSearchParams(location.search);
  const five = Number(q.get("five") ?? 62);
  const week = Number(q.get("week") ?? 38);
  const daily = Array.from({ length: 14 }, (_, i) => {
    const d = new Date(now - (13 - i) * 86400_000);
    const weekend = d.getDay() === 0 || d.getDay() === 6;
    const base = weekend ? 0.15 : 1;
    const total = Math.round((8 + 30 * Math.abs(Math.sin(i * 1.7))) * 1e6 * base);
    return {
      date: d.toISOString().slice(0, 10),
      total,
      output: Math.round(total * 0.012),
      cost: total * 0.0000009,
      messages: Math.round(total / 60000),
    };
  });
  const heat = Array.from({ length: 7 }, (_, wd) =>
    Array.from({ length: 24 }, (_, hr) => {
      if (wd >= 5) return hr > 14 && hr < 17 ? 3 : 0;
      if (hr < 8 || hr > 19) return hr === 21 ? 4 : 0;
      return Math.round(20 + 40 * Math.sin(((hr - 8) / 11) * Math.PI) * (hr === 12 ? 0.3 : 1));
    }),
  );
  const t = (total: number, msgs: number, sess: number) => ({
    input: Math.round(total * 0.004),
    output: Math.round(total * 0.012),
    cacheRead: Math.round(total * 0.9),
    cacheWrite: Math.round(total * 0.084),
    total,
    cost: total * 0.0000009,
    messages: msgs,
    sessions: sess,
  });
  return {
    usage: {
      five_hour: { utilization: five, resets_at: new Date(now + 2.3 * h).toISOString() },
      seven_day: { utilization: week, resets_at: new Date(now + 3.6 * 24 * h).toISOString() },
      seven_day_opus: null,
      seven_day_sonnet: null,
      // formato real observado na API (out/2026): limites por modelo em `limits[]`
      limits: [
        { group: "session", kind: "session", percent: five, resets_at: new Date(now + 2.3 * h).toISOString(), scope: null, severity: "normal" },
        { group: "weekly", kind: "weekly_all", percent: week, resets_at: new Date(now + 3.6 * 24 * h).toISOString(), scope: null, severity: "normal" },
        { group: "weekly", kind: "weekly_scoped", percent: 21, resets_at: new Date(now + 3.6 * 24 * h).toISOString(), scope: { model: { display_name: "Fable", id: null }, surface: null }, severity: "normal" },
      ],
      weekly_scoped_shares: [{ allowance_percent_of_weekly: 50, limit_index: 2, used_percent_of_weekly: 10 }],
      extra_usage: { is_enabled: false, monthly_limit: null, used_credits: null, utilization: null },
      spend: { enabled: true, used: { amount_minor: 1240, currency: "USD", exponent: 2 }, limit: { amount_minor: 5000, currency: "USD", exponent: 2 }, percent: 24.8 },
    },
    profile: {
      account: { full_name: q.get("name") ?? "Ana Souza", email: q.get("email") ?? "ana.souza@example.com" },
      organization: q.get("plan") === "team"
        ? { name: "Minha Empresa", organization_type: "claude_team", rate_limit_tier: "default_claude_max_5x" }
        : { name: null, organization_type: "claude_max", rate_limit_tier: "default_claude_max_5x" },
    },
    creds: {
      expiresAt: now + 0.8 * h,
      scopes: ["user:inference", "user:profile"],
      subscriptionType: q.get("plan") === "team" ? "team" : "max",
      rateLimitTier: "default_claude_max_5x",
      source: "C:\\Users\\ana\\.claude\\.credentials.json",
    },
    local: {
      today: t(41_200_000, 612, 5),
      block: t(23_800_000, 344, 3),
      blockStart: Math.floor((now - 2.7 * h) / 1000),
      week: t(182_000_000, 2810, 19),
      month: t(604_000_000, 9120, 61),
      daily,
      heatmap: heat,
      models: [
        ...(q.get("nofable") ? [] : [{ model: "claude-fable-5-1", display: "Fable 5.1", input: 300000, output: 1100000, cacheRead: 64000000, cacheWrite: 4200000, total: 69600000, cost: 123.5, messages: 940, priced: true }]),
        { model: "claude-opus-4-5-20251101", display: "Opus 4.5", input: 410000, output: 1620000, cacheRead: 118000000, cacheWrite: 9100000, total: 129130000, cost: 112.4, messages: 1680 },
        { model: "claude-sonnet-4-5-20250929", display: "Sonnet 4.5", input: 220000, output: 690000, cacheRead: 47000000, cacheWrite: 3900000, total: 51810000, cost: 38.7, messages: 1010 },
        { model: "claude-haiku-4-5-20251001", display: "Haiku 4.5", input: 80000, output: 52000, cacheRead: 900000, cacheWrite: 30000, total: 1062000, cost: 0.6, messages: 120 },
      ],
      projects: [
        { name: "api-backend", total: 88_000_000, cost: 71.2, messages: 1300, sessions: 7, lastTs: now / 1000 - 300 },
        { name: "app-web", total: 51_000_000, cost: 40.1, messages: 820, sessions: 5, lastTs: now / 1000 - 7200 },
        { name: "claude-usage", total: 27_000_000, cost: 22.9, messages: 410, sessions: 4, lastTs: now / 1000 - 60 },
        { name: "scripts", total: 11_000_000, cost: 9.3, messages: 190, sessions: 2, lastTs: now / 1000 - 86400 * 2 },
        { name: "docs", total: 5_000_000, cost: 3.1, messages: 90, sessions: 1, lastTs: now / 1000 - 86400 * 4 },
      ],
      lastActivity: Math.floor(now / 1000) - 45,
      activeSessions: 2,
      burnRateTpm: 41250,
      costPerHour: 6.8,
      files: 214,
      records: 12930,
      scanMs: 38,
      roots: ["C:\\Users\\ana\\.claude\\projects"],
      sources: {
        desktopBases: [["C:\\Users\\ana\\AppData\\Roaming\\Claude", true]],
        desktopDirs: [],
        desktopDirCount: q.get("nodesk") ? 0 : 2,
        desktopFileCount: q.get("nodesk") ? 0 : 7,
        walkedEntries: 1200,
        discoveryMs: 40,
        cliRoots: ["C:\\Users\\ana\\.claude\\projects"],
      },
    },
    history: Array.from({ length: 30 }, (_, i) => ({
      ts: now - (29 - i) * 5 * 60_000,
      fiveHour: Math.max(0, five - (29 - i) * 1.4),
      sevenDay: week - (29 - i) * 0.2,
    })),
    updatedAt: now - 42_000,
    localUpdatedAt: now - 5_000,
    nextRefreshAt: now + 138_000,
    error: q.get("error") ? { kind: q.get("error")!, message: "Sessão do Claude Code expirada. Abra o Claude Code (rode `claude`) para renovar o login." } : null,
    refreshing: false,
    renewing: q.get("renewing") === "1",
    renewFailed: q.get("renewfail") === "1",
    stale: false,
    service: mockService(q.get("svc"), now),
    claudeVersion: "2.1.4",
    appVersion: "1.0.0",
  };
}

function mockSnapshot(): Snapshot {
  const claude = mockClaudeSnapshot();
  const codex = mockClaudeSnapshot();
  codex.usage = { five_hour: { utilization: 47, resets_at: new Date(Date.now() + 7200000).toISOString() }, seven_day: { utilization: 23, resets_at: new Date(Date.now() + 3 * 86400000).toISOString() } };
  codex.profile = { plan: "Plus · demonstração", account: { name: "Pessoa Exemplo", email: "pessoa@example.com" } };
  codex.creds = null;
  codex.service = null;
  codex.error = null;
  codex.renewing = false;
  if (codex.local) codex.local.models = codex.local.models.map((m, i) => ({ ...m, model: `codex-demo-${i}`, display: `Codex · modelo ${i + 1}`, priced: false }));
  return { ...claude, codex };
}

function mockService(kind: string | null, now: number): Snapshot["service"] {
  const base = { url: "https://status.claude.com", checkedAt: now - 90_000 };
  if (kind === "minor")
    return {
      ...base,
      indicator: "minor",
      description: "Partially Degraded Service",
      incidents: [{ name: "Elevated errors on Claude Opus 4.5", status: "investigating", impact: "minor", url: "https://status.claude.com/incidents/x" }],
      components: [{ name: "Claude Code", status: "degraded_performance" }],
    };
  if (kind === "major")
    return {
      ...base,
      indicator: "major",
      description: "Partial System Outage",
      incidents: [
        { name: "claude.ai and Claude Code unavailable for some users", status: "identified", impact: "major", url: "https://status.claude.com/incidents/y" },
        { name: "Slow responses on the API", status: "monitoring", impact: "minor", url: "https://status.claude.com/incidents/z" },
      ],
      components: [{ name: "claude.ai", status: "partial_outage" }, { name: "Claude Code", status: "partial_outage" }, { name: "Claude API", status: "degraded_performance" }],
    };
  return { ...base, indicator: "none", description: "All Systems Operational", incidents: [], components: [] };
}

async function mockInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "toggle_provider":
      mockSettings.provider = mockSettings.provider === "codex" ? "claude" : "codex";
      window.dispatchEvent(new CustomEvent("mock:settings", { detail: mockSettings }));
      return null;
    case "get_snapshot":
      return mockSnapshot();
    case "get_settings":
      return mockSettings;
    case "update_settings":
      mockSettings = { ...mockSettings, ...(args?.settings as Partial<Settings>) };
      window.dispatchEvent(new CustomEvent("mock:settings", { detail: mockSettings }));
      return mockSettings;
    case "set_autostart":
      mockSettings = { ...mockSettings, autostart: !!args?.enabled };
      return mockSettings;
    case "set_pinned":
      mockSettings = { ...mockSettings, pinned: !!args?.pinned };
      return null;
    case "refresh_now":
      window.dispatchEvent(new CustomEvent("mock:refreshing", { detail: true }));
      setTimeout(() => {
        window.dispatchEvent(new CustomEvent("mock:snapshot", { detail: mockSnapshot() }));
        window.dispatchEvent(new CustomEvent("mock:refreshing", { detail: false }));
      }, 900);
      return null;
    default:
      return null;
  }
}
