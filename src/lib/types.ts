export interface UsageWindowRaw {
  utilization?: number | null;
  resets_at?: string | null;
  [k: string]: unknown;
}

export interface ExtraUsageRaw {
  is_enabled?: boolean | null;
  monthly_limit?: number | null;
  used_credits?: number | null;
  utilization?: number | null;
  currency?: string | null;
}

export type UsageRaw = Record<string, unknown> & {
  five_hour?: UsageWindowRaw | null;
  seven_day?: UsageWindowRaw | null;
  extra_usage?: ExtraUsageRaw | null;
};

export interface Creds {
  expiresAt?: number | null;
  scopes: string[];
  subscriptionType?: string | null;
  rateLimitTier?: string | null;
  source: string;
}

export interface Totals {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  total: number;
  cost: number;
  messages: number;
  sessions: number;
}

export interface DayPoint {
  date: string;
  total: number;
  output: number;
  cost: number;
  messages: number;
}

export interface ModelRow {
  model: string;
  display: string;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  total: number;
  cost: number;
  messages: number;
  priced?: boolean;
}

export interface ProjectRow {
  name: string;
  total: number;
  cost: number;
  messages: number;
  sessions: number;
  lastTs: number;
}

export interface LocalStats {
  today: Totals;
  block: Totals;
  blockStart: number;
  week: Totals;
  month: Totals;
  daily: DayPoint[];
  heatmap: number[][];
  models: ModelRow[];
  projects: ProjectRow[];
  lastActivity?: number | null;
  activeSessions: number;
  burnRateTpm: number;
  costPerHour: number;
  files: number;
  records: number;
  scanMs: number;
  roots: string[];
  sources?: {
    desktopBases: [string, boolean][];
    desktopDirs: string[];
    desktopDirCount: number;
    desktopFileCount: number;
    walkedEntries: number;
    discoveryMs: number;
    cliRoots: string[];
  };
}

export interface HistPoint {
  ts: number;
  fiveHour?: number | null;
  sevenDay?: number | null;
}

export interface ServiceIncident {
  name: string;
  status: string;
  impact: string;
  url?: string | null;
  updatedAt?: string | null;
}

export interface ServiceStatus {
  indicator: string;
  description: string;
  incidents: ServiceIncident[];
  components: { name: string; status: string }[];
  url: string;
  checkedAt: number;
}

export interface Snapshot {
  usage?: UsageRaw | null;
  profile?: Record<string, any> | null;
  creds?: Creds | null;
  local?: LocalStats | null;
  history: HistPoint[];
  updatedAt?: number | null;
  localUpdatedAt?: number | null;
  nextRefreshAt?: number | null;
  error?: { kind: string; message: string } | null;
  refreshing: boolean;
  renewing?: boolean;
  renewFailed?: boolean;
  stale: boolean;
  service?: ServiceStatus | null;
  claudeVersion: string;
  appVersion: string;
}

export interface Settings {
  refreshSecs: number;
  widgetVisible: boolean;
  widgetPos?: [number, number] | null;
  theme: "system" | "light" | "dark";
  notifications: boolean;
  thresholds: number[];
  widgetMetric: "five_hour" | "seven_day";
  reduceMotion: boolean;
  initialized: boolean;
  autoRenew: boolean;
  notifyReset: boolean;
  shortcut: boolean;
  hideFullscreen: boolean;
  hideOnShare: boolean;
  serviceStatus: boolean;
  shortcutKeys?: string;
  shortcutFailed?: boolean;
  autostart: boolean;
  pinned: boolean;
  platform: string;
}

export interface LimitWindow {
  key: string;
  label: string;
  short: string;
  utilization: number;
  resetsAt: Date | null;
  periodMs: number | null;
  /** limite específico de um modelo/superfície (ex.: Semanal · Fable) */
  scoped?: boolean;
  /** detalhe extra (ex.: cota do limite semanal que o modelo pode usar) */
  note?: string;
}
