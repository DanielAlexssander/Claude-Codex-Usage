import type { Settings, Snapshot } from "./types";
export function providerSnapshot(s: Snapshot, provider: Settings["provider"]): Snapshot {
  if (provider !== "codex") return s;
  return s.codex ?? { history: [], refreshing: s.refreshing, stale: false, claudeVersion: "", appVersion: s.appVersion,
    error: { kind: "codex_no_data", message: "Use o Codex neste computador para registrar os limites do plano." } };
}
