//! Estado global compartilhado entre o loop de atualização, comandos e janelas.

use crate::credentials::OAuthCreds;
use crate::local_stats::{LocalStats, Scanner};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistPoint {
    pub ts: i64,
    pub five_hour: Option<f64>,
    pub seven_day: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ErrInfo {
    pub kind: String,
    pub message: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub codex: Option<Box<Snapshot>>,
    pub usage: Option<Value>,
    pub profile: Option<Value>,
    pub creds: Option<OAuthCreds>,
    pub local: Option<LocalStats>,
    pub history: Vec<HistPoint>,
    /// epoch ms do último /usage bem-sucedido
    pub updated_at: Option<i64>,
    pub local_updated_at: Option<i64>,
    pub next_refresh_at: Option<i64>,
    pub error: Option<ErrInfo>,
    pub refreshing: bool,
    /// renovação automática do login em andamento / última tentativa falhou
    pub renewing: bool,
    pub renew_failed: bool,
    /// dados vieram do cache em disco (app acabou de abrir)
    pub stale: bool,
    /// status público do Claude (status.claude.com); None = desligado ou ainda não consultado
    pub service: Option<crate::status::ServiceStatus>,
    pub claude_version: String,
    pub app_version: String,
}

/// O que sobrevive entre execuções (em `<app_cache_dir>/cache.json`).
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiskCache {
    pub usage: Option<Value>,
    pub profile: Option<Value>,
    pub updated_at: Option<i64>,
    pub history: Vec<HistPoint>,
    /// chave da janela → (limiar já notificado, resets_at)
    pub notified: HashMap<String, (u8, String)>,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub snapshot: Mutex<Snapshot>,
    pub scanner: Mutex<Scanner>,
    pub codex_scanner: Mutex<crate::codex::CodexScanner>,
    pub client: reqwest::Client,
    pub user_agent: Mutex<String>,
    pub wake: tokio::sync::Notify,
    pub force_api: AtomicBool,
    pub notified: Mutex<HashMap<String, (u8, String)>>,
    pub pinned: AtomicBool,
    settings_dirty: AtomicBool,
    last_panel_hide: Mutex<Option<Instant>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, cache_dir: PathBuf) -> Self {
        let settings = Settings::load(&config_dir);
        let cache: DiskCache = std::fs::read_to_string(cache_dir.join("cache.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let snapshot = Snapshot {
            usage: cache.usage,
            profile: cache.profile,
            updated_at: cache.updated_at,
            history: cache.history,
            stale: true,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            claude_version: "2.1.0".into(),
            ..Default::default()
        };
        Self {
            settings: Mutex::new(settings),
            config_dir,
            cache_dir,
            snapshot: Mutex::new(snapshot),
            scanner: Mutex::new(Scanner::new()),
            codex_scanner: Mutex::new(crate::codex::CodexScanner::default()),
            client: crate::api::build_client(),
            user_agent: Mutex::new("claude-cli/2.1.0 (external, cli)".into()),
            wake: tokio::sync::Notify::new(),
            force_api: AtomicBool::new(false),
            notified: Mutex::new(cache.notified),
            pinned: AtomicBool::new(false),
            settings_dirty: AtomicBool::new(false),
            last_panel_hide: Mutex::new(None),
        }
    }

    pub fn mark_settings_dirty(&self) {
        self.settings_dirty.store(true, Ordering::Relaxed);
    }

    pub fn save_settings_if_dirty(&self) {
        if self.settings_dirty.swap(false, Ordering::Relaxed) {
            self.settings.lock().unwrap().save(&self.config_dir);
        }
    }

    pub fn save_settings_now(&self) {
        self.settings_dirty.store(false, Ordering::Relaxed);
        self.settings.lock().unwrap().save(&self.config_dir);
    }

    pub fn save_cache(&self) {
        let snap = self.snapshot.lock().unwrap();
        let cache = DiskCache {
            usage: snap.usage.clone(),
            profile: snap.profile.clone(),
            updated_at: snap.updated_at,
            history: snap.history.clone(),
            notified: self.notified.lock().unwrap().clone(),
        };
        drop(snap);
        let _ = std::fs::create_dir_all(&self.cache_dir);
        if let Ok(json) = serde_json::to_string(&cache) {
            let tmp = self.cache_dir.join("cache.json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, self.cache_dir.join("cache.json"));
            }
        }
    }

    pub fn note_panel_hidden(&self) {
        *self.last_panel_hide.lock().unwrap() = Some(Instant::now());
    }

    pub fn panel_hidden_recently(&self) -> bool {
        self.last_panel_hide
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_millis() < 300)
            .unwrap_or(false)
    }
}
