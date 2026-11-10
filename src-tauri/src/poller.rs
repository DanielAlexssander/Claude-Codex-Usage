//! Loop de atualização:
//!  * estatísticas locais a cada 30 s (leitura incremental, barato);
//!  * API de uso no intervalo configurado (padrão 3 min), com backoff em 429;
//!  * perfil (conta/organização) a cada 1 h.

use crate::api::{self, ApiError};
use crate::state::{AppState, ErrInfo, HistPoint};
use crate::{credentials, tray};
use chrono::Utc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

const LOCAL_TICK: u64 = 30;
const MIN_MANUAL_GAP: u64 = 20;
const PROFILE_EVERY: u64 = 3600;
const HISTORY_KEEP_MS: i64 = 7 * 86_400_000;
/// status.claude.com: a cada 5 min (de novo em 2 min se a consulta falhar)
const STATUS_EVERY: u64 = 300;
const STATUS_RETRY: u64 = 120;
/// aviso "faltam N min para a sessão reiniciar"
const SOON_MIN: i64 = 15;

pub fn emit_snapshot(app: &AppHandle) {
    let snap = app.state::<AppState>().snapshot.lock().unwrap().clone();
    let _ = app.emit("snapshot", &snap);
}

fn set_refreshing(app: &AppHandle, v: bool) {
    app.state::<AppState>().snapshot.lock().unwrap().refreshing = v;
    let _ = app.emit("refreshing", v);
}

pub async fn run(app: AppHandle) {
    // Detecta versão do Claude Code para o User-Agent (uma vez, fora da thread principal).
    {
        let v = tauri::async_runtime::spawn_blocking(api::detect_claude_version).await.unwrap_or_else(|_| "2.1.0".into());
        let st = app.state::<AppState>();
        *st.user_agent.lock().unwrap() = format!("claude-cli/{v} (external, cli)");
        st.snapshot.lock().unwrap().claude_version = v;
    }

    let mut last_api: Option<Instant> = None;
    let mut last_profile: Option<Instant> = None;
    let mut backoff_secs: Option<u64> = None;
    // Quando o login falha (expirado / 401), guarda o `expiresAt` da credencial que falhou.
    // A cada ciclo de 30 s relê a credencial: se o Claude Code a renovou, consulta a API na hora.
    let mut blocked_exp: Option<Option<i64>> = None;
    // credencial para a qual já tentamos a renovação automática (uma tentativa por expiração)
    let mut renew_tried: Option<Option<i64>> = None;
    let mut last_status: Option<(Instant, bool)> = None;

    loop {
        let st = app.state::<AppState>();
        let interval = st.settings.lock().unwrap().refresh_secs;
        let forced = st.force_api.swap(false, Ordering::Relaxed);
        let wait_api = backoff_secs.unwrap_or(interval);
        let renewed = blocked_exp.is_some_and(|b| {
            credentials::load().map(|c| !c.is_expired() && c.expires_at != b).unwrap_or(false)
        });
        let api_due = renewed || match last_api {
            None => true,
            Some(t) => {
                let el = t.elapsed().as_secs();
                el >= wait_api || (forced && el >= MIN_MANUAL_GAP && backoff_secs.is_none())
            }
        };

        set_refreshing(&app, true);
        refresh_local(&app).await;
        refresh_status(&app, &mut last_status).await;
        if check_session_events(&app) {
            // a janela de 5h virou: busca o % novo (respeitando o intervalo mínimo entre consultas)
            st.force_api.store(true, Ordering::Relaxed);
        }

        if api_due {
            last_api = Some(Instant::now());
            match refresh_api(&app, &mut last_profile).await {
                Ok(()) => {
                    backoff_secs = None;
                    blocked_exp = None;
                    renew_tried = None;
                }
                Err(ApiError::Unauthorized(_)) => {
                    backoff_secs = None;
                    let exp = credentials::load().ok().and_then(|c| c.expires_at);
                    blocked_exp = Some(exp);
                    let st = app.state::<AppState>();
                    let kind = st.snapshot.lock().unwrap().error.as_ref().map(|e| e.kind.clone()).unwrap_or_default();
                    let auto = st.settings.lock().unwrap().auto_renew;
                    // só quando o token venceu: um 401 com token "válido" (revogado) não se resolve renovando
                    if auto && kind == "expired" && renew_tried != Some(exp) {
                        renew_tried = Some(exp);
                        crate::renew::start(app.clone(), exp);
                    }
                }
                Err(ApiError::RateLimited(retry)) => {
                    let next = retry.unwrap_or_else(|| backoff_secs.map(|b| (b * 2).min(900)).unwrap_or(interval.max(180)));
                    backoff_secs = Some(next.clamp(60, 900));
                }
                Err(_) => backoff_secs = None,
            }
        }

        let st = app.state::<AppState>();
        let until_api = {
            let w = backoff_secs.unwrap_or(interval);
            let el = last_api.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            w.saturating_sub(el)
        };
        let sleep_for = until_api.clamp(1, LOCAL_TICK);
        {
            let mut snap = st.snapshot.lock().unwrap();
            snap.refreshing = false;
            snap.next_refresh_at = Some(Utc::now().timestamp_millis() + (until_api as i64) * 1000);
        }
        st.save_settings_if_dirty();
        tray::update(&app);
        emit_snapshot(&app);
        if std::env::var_os("CLAUDE_USAGE_DEBUG").is_some() {
            let snap = st.snapshot.lock().unwrap().clone();
            eprintln!("[claude-usage] snapshot: {}", serde_json::to_string(&snap).unwrap_or_default());
        }
        let _ = app.emit("refreshing", false);

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(sleep_for)) => {},
            _ = st.wake.notified() => {},
        }
    }
}

async fn refresh_local(app: &AppHandle) {
    let block_start = {
        let st = app.state::<AppState>();
        let snap = st.snapshot.lock().unwrap();
        snap.usage
            .as_ref()
            .and_then(|u| api::resets_at(u, "five_hour"))
            .and_then(|r| chrono::DateTime::parse_from_rfc3339(&r).ok())
            .map(|d| d.timestamp() - 5 * 3600)
    };
    let handle = app.clone();
    let stats = tauri::async_runtime::spawn_blocking(move || {
        let st = handle.state::<AppState>();
        let mut sc = st.scanner.lock().unwrap();
        sc.scan();
        sc.compute(block_start)
    })
    .await;
    if let Ok(stats) = stats {
        let st = app.state::<AppState>();
        // diagnóstico das fontes lidas (sem conteúdo de conversa: só pastas, contagens e modelos)
        let diag = serde_json::json!({
            "geradoEm": Utc::now().to_rfc3339(),
            "fontes": stats.sources,
            "arquivosLidos": stats.files,
            "registros": stats.records,
            "modelos7d": stats.models.iter().map(|m| (m.model.clone(), m.messages)).collect::<Vec<_>>(),
            "projetos7d": stats.projects.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        });
        let _ = std::fs::create_dir_all(&st.cache_dir);
        let _ = std::fs::write(st.cache_dir.join("diagnostico.json"), serde_json::to_string_pretty(&diag).unwrap_or_default());
        let mut snap = st.snapshot.lock().unwrap();
        snap.local = Some(stats);
        snap.local_updated_at = Some(Utc::now().timestamp_millis());
    }
}

async fn refresh_status(app: &AppHandle, last: &mut Option<(Instant, bool)>) {
    let st = app.state::<AppState>();
    if !st.settings.lock().unwrap().service_status {
        st.snapshot.lock().unwrap().service = None;
        *last = None;
        return;
    }
    let due = match last {
        None => true,
        Some((t, ok)) => t.elapsed().as_secs() >= if *ok { STATUS_EVERY } else { STATUS_RETRY },
    };
    if !due {
        return;
    }
    let client = st.client.clone();
    match crate::status::fetch(&client, Utc::now().timestamp_millis()).await {
        Ok(s) => {
            let mut snap = st.snapshot.lock().unwrap();
            let was = snap.service.as_ref().is_some_and(|x| x.is_degraded());
            if s.is_degraded() != was {
                crate::diag_log(&format!("status do Claude: {} ({} incidente(s))", s.description, s.incidents.len()));
            }
            snap.service = Some(s);
            *last = Some((Instant::now(), true));
        }
        Err(e) => {
            // mantém o último status conhecido; só descarta se estiver velho (> 30 min)
            let mut snap = st.snapshot.lock().unwrap();
            if snap.service.as_ref().is_some_and(|x| Utc::now().timestamp_millis() - x.checked_at > 30 * 60_000) {
                snap.service = None;
            }
            if last.is_none_or(|(_, ok)| ok) {
                crate::diag_log(&format!("status do Claude indisponível: {e}"));
            }
            *last = Some((Instant::now(), false));
        }
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Avisos ligados ao relógio da sessão de 5h (rodam a cada ciclo de 30 s, mesmo sem consultar a API):
///  * faltam 15 min para reiniciar — só se a sessão estiver acima do primeiro limiar;
///  * sessão reiniciada — só se ela tinha passado do primeiro limiar.
///
/// Devolve `true` quando a janela acabou de virar.
fn check_session_events(app: &AppHandle) -> bool {
    let st = app.state::<AppState>();
    let (enabled, min_thr) = {
        let s = st.settings.lock().unwrap();
        (s.notifications && s.notify_reset, s.thresholds.first().copied().unwrap_or(75) as f64)
    };
    let (util, resets) = {
        let snap = st.snapshot.lock().unwrap();
        let Some(u) = snap.usage.as_ref() else { return false };
        (api::utilization(u, "five_hour"), api::resets_at(u, "five_hour"))
    };
    let (Some(util), Some(resets)) = (util, resets) else { return false };
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(&resets) else { return false };
    let left_ms = at.timestamp_millis() - Utc::now().timestamp_millis();

    let mut notified = st.notified.lock().unwrap();
    let already = |n: &std::collections::HashMap<String, (u8, String)>, k: &str| n.get(k).is_some_and(|(_, r)| *r == resets);

    if left_ms > 0 && left_ms <= SOON_MIN * 60_000 && util >= min_thr && !already(&notified, "evt_soon") {
        notified.insert("evt_soon".into(), (1, resets.clone()));
        if enabled {
            let mins = (left_ms as f64 / 60_000.0).ceil() as i64;
            let hora = at.with_timezone(&chrono::Local).format("%H:%M");
            notify(
                app,
                &format!("Claude · sessão reinicia em {mins} min"),
                &format!("A sessão de 5h está em {util:.0}% e volta a 0% às {hora}."),
            );
        }
    }

    // a janela virou: avisa uma vez (e não se o app estava fechado havia muito tempo)
    if left_ms <= 0 && !already(&notified, "evt_reset") {
        notified.insert("evt_reset".into(), (1, resets.clone()));
        if enabled && util >= min_thr && left_ms > -10 * 60_000 {
            notify(app, "Claude · sessão reiniciada", "O limite da sessão de 5h voltou a 0%. Pode voltar a usar.");
        }
        return true;
    }
    false
}

async fn refresh_api(app: &AppHandle, last_profile: &mut Option<Instant>) -> Result<(), ApiError> {
    let st = app.state::<AppState>();
    let creds = match credentials::load() {
        Ok(c) => c,
        Err(msg) => {
            let mut snap = st.snapshot.lock().unwrap();
            snap.creds = None;
            snap.error = Some(ErrInfo { kind: "no_login".into(), message: msg });
            return Err(ApiError::Unauthorized("sem login".into()));
        }
    };
    st.snapshot.lock().unwrap().creds = Some(creds.clone());

    if creds.is_expired() {
        let mut snap = st.snapshot.lock().unwrap();
        snap.error = Some(ErrInfo {
            kind: "expired".into(),
            message: "O login do Claude Code salvo neste computador expirou. Ele só é renovado quando o Claude Code roda no terminal — o Claude Desktop não atualiza esse login. Clique em \"Renovar login\": o Claude Code abre num terminal e, assim que iniciar, pode fechá-lo; o app detecta sozinho. Os dados abaixo são da última leitura.".into(),
        });
        return Err(ApiError::Unauthorized("expirado".into()));
    }

    let ua = st.user_agent.lock().unwrap().clone();
    let client = st.client.clone();

    let need_profile = last_profile.map(|t| t.elapsed().as_secs() >= PROFILE_EVERY).unwrap_or(true);
    if need_profile {
        if let Ok(p) = api::fetch_profile(&client, &creds.access_token, &ua).await {
            st.snapshot.lock().unwrap().profile = Some(p);
            *last_profile = Some(Instant::now());
        }
    }

    match api::fetch_usage(&client, &creds.access_token, &ua).await {
        Ok(usage) => {
            let now = Utc::now().timestamp_millis();
            {
                let mut snap = st.snapshot.lock().unwrap();
                snap.history.push(HistPoint {
                    ts: now,
                    five_hour: api::utilization(&usage, "five_hour"),
                    seven_day: api::utilization(&usage, "seven_day"),
                });
                snap.history.retain(|h| h.ts >= now - HISTORY_KEEP_MS);
                snap.usage = Some(usage.clone());
                snap.updated_at = Some(now);
                snap.error = None;
                snap.stale = false;
            }
            check_thresholds(app, &usage);
            st.save_cache();
            if crate::renew::JUST_RENEWED.swap(false, std::sync::atomic::Ordering::SeqCst) {
                crate::diag_log(&format!(
                    "renovação automática: sessão 5h depois = {}",
                    crate::renew::session_summary(app)
                ));
            }
            Ok(())
        }
        Err(e) => {
            let mut snap = st.snapshot.lock().unwrap();
            snap.error = Some(ErrInfo { kind: e.kind().into(), message: e.message() });
            Err(e)
        }
    }
}

fn label_for(key: &str) -> String {
    match key {
        "five_hour" => "Sessão atual (5h)".into(),
        "seven_day" => "Limite semanal".into(),
        "seven_day_opus" => "Limite semanal · Opus".into(),
        "seven_day_sonnet" => "Limite semanal · Sonnet".into(),
        k => k.replace('_', " "),
    }
}

fn check_thresholds(app: &AppHandle, usage: &serde_json::Value) {
    let st = app.state::<AppState>();
    let (enabled, thresholds) = {
        let s = st.settings.lock().unwrap();
        (s.notifications, s.thresholds.clone())
    };
    let Some(obj) = usage.as_object() else { return };

    // (chave, rótulo, %, resets_at) — janelas no topo do payload + limites por modelo em `limits[]`
    let mut windows: Vec<(String, String, f64, String)> = Vec::new();
    let mut scoped_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (key, val) in obj {
        let Some(util) = val.get("utilization").and_then(|u| u.as_f64()) else { continue };
        let resets = val.get("resets_at").and_then(|r| r.as_str()).unwrap_or("").to_string();
        windows.push((key.clone(), label_for(key), util, resets));
    }
    if let Some(limits) = obj.get("limits").and_then(|l| l.as_array()) {
        for l in limits {
            if l.get("kind").and_then(|k| k.as_str()) != Some("weekly_scoped") {
                continue;
            }
            let Some(util) = l.get("percent").and_then(|p| p.as_f64()) else { continue };
            let name = l
                .pointer("/scope/model/display_name")
                .or_else(|| l.pointer("/scope/surface/display_name"))
                .and_then(|n| n.as_str())
                .unwrap_or("modelo");
            let resets = l.get("resets_at").and_then(|r| r.as_str()).unwrap_or("").to_string();
            windows.push((format!("scoped_{name}"), format!("Limite semanal · {name}"), util, resets));
            scoped_names.insert(format!("scoped_{name}"), name.to_string());
        }
    }

    let mut notified = st.notified.lock().unwrap();
    for (key, label, util, resets) in windows {
        let key = &key;
        let prev = notified.get(key).filter(|(_, r)| *r == resets).map(|(t, _)| *t).unwrap_or(0);
        let crossed = thresholds.iter().copied().filter(|t| util >= *t as f64).max().unwrap_or(0);
        if crossed > prev {
            notified.insert(key.clone(), (crossed, resets.clone()));
            if enabled {
                let when = chrono::DateTime::parse_from_rfc3339(&resets)
                    .ok()
                    .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
                    .map(|s| format!(" Reinicia em {s}."))
                    .unwrap_or_default();
                // limite próprio de um modelo (ex.: Fable): sugere trocar de modelo, porque o
                // limite semanal geral continua valendo para os outros
                let tip = scoped_names
                    .get(key)
                    .map(|m| {
                        let alt = if m.to_lowercase().contains("opus") { "Sonnet" } else { "Opus ou Sonnet" };
                        format!(" Para não travar o {m}, use {alt} no que não precisar dele (/model no Claude Code).")
                    })
                    .unwrap_or_default();
                notify(app, &format!("Claude · {} em {:.0}%", label, util), &format!("Você passou de {crossed}% do limite.{when}{tip}"));
            }
        } else if util < 1.0 && prev > 0 {
            notified.remove(key);
        }
    }
}
