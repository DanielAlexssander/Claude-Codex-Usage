use crate::{local_stats::Scanner, state::{Snapshot, ErrInfo}};
use chrono::{DateTime, Utc};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use std::{collections::HashMap, io::{Read, Seek, SeekFrom}, path::PathBuf};

#[derive(Default)]
struct FileState {
    offset: u64,
    cwd: String,
    model: String,
    totals: [u64; 3],
}

#[derive(Default)]
pub struct CodexScanner {
    stats: Scanner,
    files: HashMap<PathBuf, FileState>,
    limits: Option<Value>,
    updated: Option<i64>,
    plan: Option<String>,
}

// Decode local identity claims for display only, never for authorization.
// Only these profile fields can leave the backend; credentials are not returned.
fn account_profile(auth: &Value, fallback_plan: Option<&str>) -> Value {
    let mut profile = json!({"plan": fallback_plan});
    if auth["auth_mode"].as_str().is_some_and(|mode| !matches!(mode, "chatgpt" | "chatgptAuthTokens")) {
        return profile;
    }
    let claims = auth.pointer("/tokens/id_token").and_then(Value::as_str)
        .and_then(|token| token.split('.').nth(1))
        .and_then(|payload| URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if let Some(claims) = claims {
        let text = |value: &Value| value.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        profile["account"] = json!({"name": text(&claims["name"]), "email": text(&claims["email"])});
        if let Some(plan) = text(&claims["https://api.openai.com/auth"]["chatgpt_plan_type"]) {
            profile["plan"] = Value::String(plan);
        }
    }
    profile
}

fn usage_windows(limits: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for slot in ["primary", "secondary"] {
        let w = &limits[slot];
        let Some(percent) = w["used_percent"].as_f64().filter(|p| p.is_finite()) else { continue };
        let key = match w["window_minutes"].as_u64() {
            Some(300) => "five_hour",
            Some(10080) => "seven_day",
            _ => continue,
        };
        let reset = w["resets_at"].as_i64().and_then(|t| DateTime::from_timestamp(t, 0)).map(|t| t.to_rfc3339());
        out.insert(key.into(), json!({"utilization": percent.clamp(0.0, 100.0), "resets_at": reset}));
    }
    Value::Object(out)
}

// Codex limits only come from local logs, so an idle account keeps its last snapshot.
// Once a window's reset time has passed it is empty again, and the next window
// starts only on the next use, so its reset time is unknown until then.
fn expire_windows(limits: &Value, now: DateTime<Utc>) -> Value {
    let Some(windows) = limits.as_object() else { return limits.clone() };
    let elapsed = |w: &Value| w["resets_at"].as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t <= now);
    Value::Object(windows.iter().map(|(key, w)| {
        let shown = if elapsed(w) { json!({"utilization": 0.0, "resets_at": null}) } else { w.clone() };
        (key.clone(), shown)
    }).collect())
}

impl CodexScanner {
    pub fn scan(&mut self) -> Snapshot {
        let base = std::env::var_os("CODEX_HOME").map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|p| p.join(".codex"))).unwrap_or_default();
        let mut snapshot = self.scan_root(base.join("sessions"));
        // Read again each cycle so logout or a changed login cannot retain an old name/email.
        let auth = std::fs::read(base.join("auth.json")).ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).unwrap_or(Value::Null);
        snapshot.profile = Some(account_profile(&auth, self.plan.as_deref()));
        snapshot
    }

    fn scan_root(&mut self, root: PathBuf) -> Snapshot {
        for entry in walkdir::WalkDir::new(&root).follow_links(false).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !entry.file_type().is_file() || path.extension().is_none_or(|e| e != "jsonl") { continue; }
            let Ok(meta) = entry.metadata() else { continue };
            let state = self.files.entry(path.to_owned()).or_default();
            if state.offset == 0 && meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|d| d.as_secs() > 35 * 86400) { continue; }
            if meta.len() < state.offset { *state = FileState::default(); }
            if meta.len() == state.offset { continue; }
            let Ok(mut file) = std::fs::File::open(path) else { continue };
            if file.seek(SeekFrom::Start(state.offset)).is_err() { continue; }
            let mut buf = Vec::new();
            if file.take(64 * 1024 * 1024).read_to_end(&mut buf).is_err() { continue; }
            let Some(end) = buf.iter().rposition(|b| *b == b'\n').map(|i| i + 1) else { continue };
            for raw in buf[..end].split(|b| *b == b'\n') {
                // Only parse metadata and accounting events; never retain conversation content.
                let Ok(text) = std::str::from_utf8(raw) else { continue };
                if !["\"session_meta\"", "\"turn_context\"", "\"token_count\""].iter().any(|s| text.contains(s)) { continue; }
                let Ok(v) = serde_json::from_str::<Value>(text) else { continue };
                let p = &v["payload"];
                match v["type"].as_str() {
                    Some("session_meta" | "turn_context") => {
                        if let Some(cwd) = p["cwd"].as_str() { state.cwd = cwd.into(); }
                        if let Some(model) = p["model"].as_str() { state.model = model.into(); }
                    }
                    Some("event_msg") if p["type"] == "token_count" => {
                        let ts = v["timestamp"].as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp_millis());
                        let rates = &p["rate_limits"];
                        if rates.is_object() && rates["limit_id"].as_str().is_none_or(|id| id == "codex")
                            && ts.is_some() && ts >= self.updated && !usage_windows(rates).as_object().unwrap().is_empty() {
                            self.limits = Some(usage_windows(rates));
                            self.updated = ts;
                            self.plan = rates["plan_type"].as_str().map(str::to_owned);
                        }
                        let total = &p["info"]["total_token_usage"];
                        if !total.is_object() { continue; }
                        let next = [total["input_tokens"].as_u64().unwrap_or(0), total["output_tokens"].as_u64().unwrap_or(0), total["cached_input_tokens"].as_u64().unwrap_or(0)];
                        if next == state.totals { continue; }
                        let delta = [next[0].saturating_sub(state.totals[0]), next[1].saturating_sub(state.totals[1]), next[2].saturating_sub(state.totals[2])];
                        state.totals = next;
                        let line = json!({"type":"assistant", "timestamp":v["timestamp"], "cwd":state.cwd,
                            "sessionId":path.to_string_lossy(), "message":{"id":format!("{}:{}:{}", next[0], next[1], next[2]),
                            "model":if state.model.is_empty() { "Codex" } else { &state.model }, "usage":{
                                "input_tokens":delta[0].saturating_sub(delta[2]), "output_tokens":delta[1], "cache_read_input_tokens":delta[2]}}});
                        self.stats.ingest(path, format!("{line}\n").as_bytes(), None);
                    }
                    _ => {}
                }
            }
            state.offset += end as u64;
        }
        let mut local = self.stats.compute(None);
        local.roots = vec![root.display().to_string()];
        local.sources.cli_roots = local.roots.clone();
        local.files = self.files.len();
        Snapshot {
            usage: self.limits.as_ref().map(|l| expire_windows(l, Utc::now())), local: Some(local), updated_at: self.updated,
            local_updated_at: Some(Utc::now().timestamp_millis()),
            profile: Some(json!({"plan":self.plan})),
            error: self.limits.is_none().then(|| ErrInfo { kind:"codex_no_data".into(), message:"Nenhum limite do Codex encontrado. Use o Codex conectado ao ChatGPT neste computador para registrar o uso local.".into() }),
            stale: self.updated.is_some_and(|t| Utc::now().timestamp_millis() - t > 300_000),
            app_version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_exposes_only_display_fields() {
        let claims = json!({"name":" Pessoa Exemplo ", "email":"pessoa@example.com",
            "sub":"private-subject", "https://api.openai.com/auth":{"chatgpt_plan_type":"plus", "chatgpt_account_id":"private-id"}});
        let token = format!("header.{}.signature", URL_SAFE_NO_PAD.encode(claims.to_string()));
        let auth = json!({"auth_mode":"chatgpt", "tokens":{"id_token":token,
            "access_token":"secret-access", "refresh_token":"secret-refresh"}});
        assert_eq!(account_profile(&auth, Some("pro")), json!({"plan":"plus",
            "account":{"name":"Pessoa Exemplo", "email":"pessoa@example.com"}}));
        let api_auth = json!({"auth_mode":"apikey", "tokens":auth["tokens"]});
        assert_eq!(account_profile(&api_auth, None), json!({"plan":null}));
    }

    #[test]
    fn unavailable_or_malformed_identity_keeps_a_generic_profile() {
        for auth in [Value::Null, json!({"tokens":{"id_token":"broken"}}),
            json!({"tokens":{"id_token":"header.!!!.signature"}}),
            json!({"tokens":{"id_token":format!("header.{}.signature", URL_SAFE_NO_PAD.encode(b"not json"))}})] {
            assert_eq!(account_profile(&auth, Some("plus")), json!({"plan":"plus"}));
        }
        let claims = json!({"name":null,"email":12});
        let auth = json!({"tokens":{"id_token":format!("header.{}.signature", URL_SAFE_NO_PAD.encode(claims.to_string()))}});
        assert_eq!(account_profile(&auth, None), json!({"plan":null,"account":{"name":null,"email":null}}));
    }

    #[test]
    fn limits_follow_duration_and_ignore_unknown_windows() {
        let result = usage_windows(&json!({"primary":{"used_percent":23.0,"window_minutes":10080,"resets_at":1794948767i64}, "secondary":{"used_percent":47.0,"window_minutes":300}}));
        assert_eq!(result["five_hour"]["utilization"], 47.0);
        assert_eq!(result["seven_day"]["utilization"], 23.0);
        assert!(result["seven_day"]["resets_at"].as_str().unwrap().contains('T'));
        assert_eq!(usage_windows(&json!({"primary":{"used_percent":1,"window_minutes":60}})), json!({}));
    }

    #[test]
    fn elapsed_windows_reset_to_zero_without_a_reset_time() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T19:10:00Z").unwrap().with_timezone(&Utc);
        let limits = json!({
            "five_hour": {"utilization": 62.0, "resets_at": "2026-11-12T13:47:00+00:00"},
            "seven_day": {"utilization": 30.0, "resets_at": "2026-11-15T08:00:00+00:00"},
        });
        let shown = expire_windows(&limits, now);
        assert_eq!(shown["five_hour"], json!({"utilization": 0.0, "resets_at": null}));
        assert_eq!(shown["seven_day"], limits["seven_day"]);
        // the cached limits are left untouched
        assert_eq!(limits["five_hour"]["utilization"], 62.0);
        // windows without a reset time are kept as reported
        let unknown = json!({"five_hour": {"utilization": 10.0, "resets_at": null}});
        assert_eq!(expire_windows(&unknown, now), unknown);
    }
    #[test]
    fn incremental_accounting_skips_duplicates_and_partial_lines() {
        let root = std::env::temp_dir().join(format!("codex-usage-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("rollout.jsonl");
        let timestamp = Utc::now().to_rfc3339();
        let meta = json!({"type":"session_meta", "payload":{"cwd":"/work/demo"}});
        let context = json!({"type":"turn_context", "payload":{"model":"codex-test"}});
        let event = |input, cached, output| json!({"type":"event_msg", "timestamp":timestamp, "payload":{
            "type":"token_count", "info":{"total_token_usage":{"input_tokens":input,"cached_input_tokens":cached,"output_tokens":output}},
            "rate_limits":{"limit_id":"codex","plan_type":"plus","primary":{"used_percent":47,"window_minutes":300}}
        }});
        let first = event(100, 40, 20);
        let second = event(180, 70, 35);
        std::fs::write(&path, format!("{meta}\n{context}\n{first}\n{first}\n{second}")).unwrap();
        let mut scanner = CodexScanner::default();
        let s = scanner.scan_root(root.clone());
        assert_eq!(s.local.unwrap().today.total, 120);
        assert_eq!(s.usage.unwrap()["five_hour"]["utilization"], 47.0);
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"\n").unwrap();
        let s = scanner.scan_root(root.clone());
        let local = s.local.unwrap();
        assert_eq!(local.today.total, 215);
        assert_eq!(local.today.cache_read, 70);
        assert_eq!(local.today.input, 110);
        assert_eq!(local.today.messages, 2);
        assert_eq!(local.models[0].model, "codex-test");
        assert_eq!(scanner.scan_root(root).local.unwrap().today.total, 215);
        std::fs::remove_file(path).unwrap();
    }

}
