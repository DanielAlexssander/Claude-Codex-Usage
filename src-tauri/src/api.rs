//! Cliente dos endpoints OAuth usados pelo próprio Claude Code (`/usage`, `/status`).
//!
//! * `GET https://api.anthropic.com/api/oauth/usage`   → janelas de limite (5h, semanal, por modelo, extra usage)
//! * `GET https://api.anthropic.com/api/oauth/profile` → conta, organização, tipo de plano
//!
//! São endpoints não documentados publicamente; o parser no front-end é tolerante
//! a campos novos/ausentes.

use reqwest::{header, Client, StatusCode};
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://api.anthropic.com";
const BETA: &str = "oauth-2025-04-20";

#[derive(Debug, Clone)]
pub enum ApiError {
    Unauthorized(String),
    RateLimited(Option<u64>),
    Http(u16, String),
    Network(String),
}

impl ApiError {
    pub fn kind(&self) -> &'static str {
        match self {
            ApiError::Unauthorized(_) => "auth",
            ApiError::RateLimited(_) => "rate_limited",
            ApiError::Http(..) => "http",
            ApiError::Network(_) => "network",
        }
    }
    pub fn message(&self) -> String {
        match self {
            ApiError::Unauthorized(m) => format!(
                "Sessão do Claude Code expirada ou sem permissão ({m}). Abra o Claude Code (rode `claude`) para renovar o login."
            ),
            ApiError::RateLimited(r) => match r {
                Some(s) => format!("A API pediu uma pausa. Nova tentativa em {}s.", s),
                None => "A API pediu uma pausa (limite de requisições). Tentando de novo em breve.".into(),
            },
            ApiError::Http(code, body) => format!("Erro HTTP {code}: {}", body.chars().take(160).collect::<String>()),
            ApiError::Network(m) => format!("Sem conexão com api.anthropic.com ({m}). Verifique rede/proxy."),
        }
    }
}

pub fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .gzip(true)
        .build()
        .expect("http client")
}

/// Descobre a versão instalada do Claude Code para montar o User-Agent
/// (sem ele a API coloca as chamadas num balde de rate-limit muito restrito).
pub fn detect_claude_version() -> String {
    let fallback = "2.1.0".to_string();
    let output = {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            std::process::Command::new("cmd")
                .args(["/C", "claude", "--version"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Apps GUI no macOS não herdam o PATH do shell; tenta locais comuns.
            let home = dirs::home_dir().unwrap_or_default();
            let candidates = [
                home.join(".local/bin/claude"),
                home.join(".claude/local/claude"),
                "/opt/homebrew/bin/claude".into(),
                "/usr/local/bin/claude".into(),
                "claude".into(),
            ];
            let mut res = Err(std::io::Error::new(std::io::ErrorKind::NotFound, "claude"));
            for c in candidates.iter() {
                if c.as_os_str() != "claude" && !c.exists() {
                    continue;
                }
                res = std::process::Command::new(c).arg("--version").output();
                if res.is_ok() {
                    break;
                }
            }
            res
        }
    };
    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.split_whitespace()
                .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains('.'))
                .map(|t| t.to_string())
                .unwrap_or(fallback)
        }
        _ => fallback,
    }
}

async fn get_json(client: &Client, path: &str, token: &str, ua: &str) -> Result<Value, ApiError> {
    let resp = client
        .get(format!("{BASE}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("anthropic-beta", BETA)
        .header(header::USER_AGENT, ua)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| ApiError::Network(short_err(&e)))?;

    let status = resp.status();
    if status.is_success() {
        return resp.json::<Value>().await.map_err(|e| ApiError::Network(short_err(&e)));
    }
    let retry_after = resp
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let body = resp.text().await.unwrap_or_default();
    Err(match status {
        StatusCode::UNAUTHORIZED => ApiError::Unauthorized("401".into()),
        StatusCode::FORBIDDEN => ApiError::Unauthorized(format!("403: {}", body.chars().take(120).collect::<String>())),
        StatusCode::TOO_MANY_REQUESTS => ApiError::RateLimited(retry_after),
        s => ApiError::Http(s.as_u16(), body),
    })
}

fn short_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "tempo esgotado".into()
    } else if e.is_connect() {
        "falha de conexão".into()
    } else {
        e.to_string().chars().take(140).collect()
    }
}

pub async fn fetch_usage(client: &Client, token: &str, ua: &str) -> Result<Value, ApiError> {
    get_json(client, "/api/oauth/usage", token, ua).await
}

pub async fn fetch_profile(client: &Client, token: &str, ua: &str) -> Result<Value, ApiError> {
    get_json(client, "/api/oauth/profile", token, ua).await
}

/// Utilização (%) de uma janela do payload de uso, se existir.
pub fn utilization(usage: &Value, key: &str) -> Option<f64> {
    usage.get(key)?.get("utilization")?.as_f64()
}

pub fn resets_at(usage: &Value, key: &str) -> Option<String> {
    usage.get(key)?.get("resets_at")?.as_str().map(|s| s.to_string())
}
