//! Leitura (somente leitura!) das credenciais OAuth que o próprio Claude Code
//! mantém na máquina. O app nunca grava nem renova o token: quem faz isso é o
//! Claude Code, então relemos a fonte a cada ciclo para pegar o token novo.
//!
//! Ordem de busca:
//!   1. variável `CLAUDE_CODE_OAUTH_TOKEN`
//!   2. `$CLAUDE_CONFIG_DIR/.credentials.json` ou `~/.claude/.credentials.json`
//!      (Windows e Linux)
//!   3. macOS: Keychain, item "Claude Code-credentials"
//!
//! Se houver mais de uma fonte válida, vence a que expira por último.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCreds {
    #[serde(skip_serializing)]
    pub access_token: String,
    /// epoch em milissegundos
    pub expires_at: Option<i64>,
    pub scopes: Vec<String>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub source: String,
}

impl OAuthCreds {
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(ms) => ms <= chrono::Utc::now().timestamp_millis() + 15_000,
            None => false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOauth {
    access_token: Option<String>,
    expires_at: Option<Value>,
    #[serde(default)]
    scopes: Vec<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

pub fn claude_config_dirs() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    // Assim como o Claude Code, se CLAUDE_CONFIG_DIR estiver definido ele é a única fonte.
    if let Ok(custom) = std::env::var("CLAUDE_CONFIG_DIR") {
        for p in std::env::split_paths(&custom) {
            if !p.as_os_str().is_empty() {
                dirs_out.push(p);
            }
        }
    }
    if !dirs_out.is_empty() {
        return dirs_out;
    }
    if let Some(home) = dirs::home_dir() {
        dirs_out.push(home.join(".claude"));
        dirs_out.push(home.join(".config").join("claude"));
    }
    dirs_out.dedup();
    dirs_out
}

fn parse_blob(text: &str, source: &str) -> Option<OAuthCreds> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    // Formato atual: { "claudeAiOauth": { accessToken, refreshToken, expiresAt, scopes, subscriptionType, rateLimitTier } }
    let node = v.get("claudeAiOauth").cloned().unwrap_or(v);
    let raw: RawOauth = serde_json::from_value(node).ok()?;
    let token = raw.access_token.filter(|t| !t.is_empty())?;
    let expires_at = raw.expires_at.and_then(|e| match e {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok().or_else(|| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.timestamp_millis())
        }),
        _ => None,
    });
    Some(OAuthCreds {
        access_token: token,
        expires_at,
        scopes: raw.scopes,
        subscription_type: raw.subscription_type,
        rate_limit_tier: raw.rate_limit_tier,
        source: source.to_string(),
    })
}

#[cfg(target_os = "macos")]
fn from_keychain() -> Option<OAuthCreds> {
    let user = std::env::var("USER").unwrap_or_default();
    let mut attempts: Vec<Vec<&str>> = vec![];
    if !user.is_empty() {
        attempts.push(vec!["find-generic-password", "-s", "Claude Code-credentials", "-a", &user, "-w"]);
    }
    attempts.push(vec!["find-generic-password", "-s", "Claude Code-credentials", "-w"]);
    for args in attempts {
        let out = std::process::Command::new("/usr/bin/security").args(&args).output().ok()?;
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(c) = parse_blob(&text, "Keychain do macOS") {
                return Some(c);
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn from_keychain() -> Option<OAuthCreds> {
    None
}

pub fn load() -> Result<OAuthCreds, String> {
    let mut candidates: Vec<OAuthCreds> = Vec::new();

    if let Ok(tok) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !tok.trim().is_empty() {
            candidates.push(OAuthCreds {
                access_token: tok.trim().to_string(),
                expires_at: None,
                scopes: vec![],
                subscription_type: None,
                rate_limit_tier: None,
                source: "variável CLAUDE_CODE_OAUTH_TOKEN".into(),
            });
        }
    }

    for dir in claude_config_dirs() {
        let file = dir.join(".credentials.json");
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Some(c) = parse_blob(&text, &file.display().to_string()) {
                candidates.push(c);
            }
        }
    }

    if let Some(c) = from_keychain() {
        candidates.push(c);
    }

    if candidates.is_empty() {
        return Err(
            "Login do Claude Code não encontrado. Abra um terminal, rode `claude` e faça login com sua conta Claude (Pro, Max, Team ou Enterprise)."
                .into(),
        );
    }

    // Prefere a credencial válida que expira por último.
    candidates.sort_by_key(|c| std::cmp::Reverse(c.expires_at.unwrap_or(i64::MAX)));
    let best = candidates
        .iter()
        .find(|c| !c.is_expired())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone());
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_code_credentials_file() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc","refreshToken":"r","expiresAt":1893456000000,"scopes":["user:inference","user:profile"],"subscriptionType":"team","rateLimitTier":"default_claude_max_5x"}}"#;
        let c = parse_blob(json, "test").unwrap();
        assert_eq!(c.access_token, "sk-ant-oat01-abc");
        assert_eq!(c.expires_at, Some(1893456000000));
        assert_eq!(c.subscription_type.as_deref(), Some("team"));
        assert!(!c.is_expired());
        assert_eq!(c.scopes.len(), 2);
    }

    #[test]
    fn rejects_empty_token() {
        assert!(parse_blob(r#"{"claudeAiOauth":{"accessToken":""}}"#, "t").is_none());
        assert!(parse_blob("not json", "t").is_none());
    }
}
