//! Preferências do usuário persistidas em `<app_config_dir>/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Intervalo de atualização da API de uso (segundos). Mínimo 60.
    pub refresh_secs: u64,
    pub provider: String,
    pub provider_shortcut: String,
    /// Widget flutuante visível.
    pub widget_visible: bool,
    /// Posição física (px) do widget, salva após arrastar.
    pub widget_pos: Option<(i32, i32)>,
    /// "system" | "light" | "dark"
    pub theme: String,
    /// Notificações nativas ao cruzar limiares.
    pub notifications: bool,
    /// Limiares (%) que disparam notificação.
    pub thresholds: Vec<u8>,
    /// Qual janela o anel externo do widget acompanha: "five_hour" | "seven_day".
    pub widget_metric: String,
    /// Animações reduzidas (economia de CPU/GPU e acessibilidade).
    pub reduce_motion: bool,
    /// Já passou pela configuração inicial (ex.: ativar autostart).
    pub initialized: bool,
    /// Quando o login do Claude Code expira, abre o Claude Code em segundo plano (sem janela)
    /// por alguns segundos para ele mesmo renovar o login.
    pub auto_renew: bool,
    /// Avisos da sessão de 5h: "faltam 15 min para reiniciar" e "sessão reiniciada".
    pub notify_reset: bool,
    /// Atalho global (Ctrl+Alt+U / ⌘⌥U) abre e fecha o painel.
    pub shortcut: bool,
    /// Esconde o widget enquanto outro app está em tela cheia ou apresentando (Windows).
    pub hide_fullscreen: bool,
    /// O widget não aparece em compartilhamento de tela, gravações e prints.
    pub hide_on_share: bool,
    /// Consulta status.claude.com e avisa no painel quando houver instabilidade.
    pub service_status: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_secs: 180,
            provider: "claude".into(),
            provider_shortcut: "PageUp".into(),
            widget_visible: true,
            widget_pos: None,
            theme: "system".into(),
            notifications: true,
            thresholds: vec![75, 90],
            widget_metric: "five_hour".into(),
            reduce_motion: false,
            initialized: false,
            auto_renew: true,
            notify_reset: true,
            shortcut: true,
            hide_fullscreen: true,
            hide_on_share: true,
            service_status: true,
        }
    }
}

impl Settings {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("settings.json")
    }

    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(config_dir))
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(|s| s.sanitized())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) {
        let _ = std::fs::create_dir_all(config_dir);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let tmp = config_dir.join("settings.json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, Self::path(config_dir));
            }
        }
    }

    pub fn sanitized(mut self) -> Self {
        if !matches!(self.provider.as_str(), "claude" | "codex") {
            self.provider = "claude".into();
        }
        if self.provider_shortcut.parse::<tauri_plugin_global_shortcut::Shortcut>().is_err()
            || self.provider_shortcut.parse::<tauri_plugin_global_shortcut::Shortcut>().ok()
                == crate::presentation::SHORTCUT.parse::<tauri_plugin_global_shortcut::Shortcut>().ok() {
            self.provider_shortcut = "PageUp".into();
        }
        self.refresh_secs = self.refresh_secs.clamp(60, 3600);
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = "system".into();
        }
        if !matches!(self.widget_metric.as_str(), "five_hour" | "seven_day") {
            self.widget_metric = "five_hour".into();
        }
        self.thresholds.retain(|t| (1..=100).contains(t));
        self.thresholds.sort_unstable();
        self.thresholds.dedup();
        self
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_and_invalid_shortcuts_keep_safe_defaults() {
        let old: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(old.provider, "claude");
        assert_eq!(old.provider_shortcut, "PageUp");
        let invalid = Settings { provider: "invalid".into(), provider_shortcut: "not-a-key".into(), ..Settings::default() }.sanitized();
        assert_eq!(invalid.provider, "claude");
        assert_eq!(invalid.provider_shortcut, "PageUp");
        let conflict = Settings { provider_shortcut: crate::presentation::SHORTCUT.into(), ..Settings::default() }.sanitized();
        assert_eq!(conflict.provider_shortcut, "PageUp");
        let custom = Settings { provider_shortcut: "Control+F9".into(), ..Settings::default() }.sanitized();
        assert_eq!(custom.provider_shortcut, "Control+F9");
    }
}
