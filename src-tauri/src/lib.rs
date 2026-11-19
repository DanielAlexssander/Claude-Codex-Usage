mod api;
mod codex;
mod credentials;
mod island;
mod legacy;
mod local_stats;
mod poller;
mod presentation;
mod renew;
mod settings;
mod startup;
mod state;
mod status;
mod tray;
mod windows;

use serde::Serialize;
use settings::Settings;
use state::{AppState, Snapshot};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SettingsPayload {
    #[serde(flatten)]
    settings: Settings,
    autostart: bool,
    pinned: bool,
    platform: &'static str,
    /// combinação do atalho global e se o registro falhou (combinação já usada por outro app)
    shortcut_keys: &'static str,
    shortcut_failed: bool,
    provider_shortcut_failed: bool,
}

fn settings_payload(app: &AppHandle) -> SettingsPayload {
    let st = app.state::<AppState>();
    let settings = st.settings.lock().unwrap().clone();
    SettingsPayload {
        settings,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        pinned: st.pinned.load(std::sync::atomic::Ordering::Relaxed),
        platform: std::env::consts::OS,
        shortcut_keys: presentation::SHORTCUT,
        shortcut_failed: presentation::SHORTCUT_FAILED.load(std::sync::atomic::Ordering::Relaxed),
        provider_shortcut_failed: presentation::PROVIDER_SHORTCUT_FAILED.load(std::sync::atomic::Ordering::Relaxed),
    }
}

pub(crate) fn emit_settings(app: &AppHandle) {
    let _ = app.emit("settings", settings_payload(app));
}

// ---------------- comandos chamados pelo front-end ----------------

#[tauri::command]
fn get_snapshot(state: tauri::State<'_, AppState>) -> Snapshot {
    state.snapshot.lock().unwrap().clone()
}

#[tauri::command]
fn refresh_now(app: AppHandle) {
    tray::request_refresh(&app);
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsPayload {
    settings_payload(&app)
}

#[tauri::command]
fn update_settings(app: AppHandle, settings: Settings) -> SettingsPayload {
    let st = app.state::<AppState>();
    let new = settings.sanitized();
    let (widget_changed, interval_changed, visible) = {
        let mut cur = st.settings.lock().unwrap();
        // trocar o estilo (círculo/ilha) ou a borda da ilha reposiciona e redimensiona a janela
        let wc = cur.widget_visible != new.widget_visible
            || cur.widget_style != new.widget_style
            || cur.island_edge != new.island_edge;
        let ic = cur.refresh_secs != new.refresh_secs || cur.service_status != new.service_status;
        let pos = cur.widget_pos; // posição é controlada pelo arrasto, não pelo front
        *cur = Settings { widget_pos: pos, initialized: true, ..new };
        (wc, ic, cur.widget_visible)
    };
    st.save_settings_now();
    if widget_changed {
        windows::set_widget_visible(&app, visible);
    }
    presentation::apply(&app);
    if interval_changed {
        st.wake.notify_one();
    }
    tray::sync_checks(&app);
    emit_settings(&app);
    settings_payload(&app)
}

/// Build de desenvolvimento (`npm run tauri dev`): carrega a interface do servidor do Vite em
/// localhost:1420, que não existe quando o Windows inicia. Esse executável nunca pode ficar
/// registrado para iniciar com o sistema — senão abre "localhost se recusou a se conectar".
pub(crate) const DEV_BUILD: bool = cfg!(debug_assertions);

/// Liga/desliga o "iniciar com o sistema" apontando para ESTE executável.
pub(crate) fn apply_autostart(app: &AppHandle, enabled: bool) {
    if DEV_BUILD && enabled {
        diag_log("build de desenvolvimento: \"iniciar com o sistema\" não é registrado (só no .exe de release)");
        return;
    }
    let al = app.autolaunch();
    let res = if enabled { al.enable() } else { al.disable() };
    if let Err(e) = res {
        diag_log(&format!("iniciar com o sistema ({enabled}): {e}"));
    }
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> SettingsPayload {
    apply_autostart(&app, enabled);
    tray::sync_checks(&app);
    emit_settings(&app);
    settings_payload(&app)
}

#[tauri::command]
fn set_pinned(app: AppHandle, pinned: bool) {
    app.state::<AppState>().pinned.store(pinned, std::sync::atomic::Ordering::Relaxed);
    emit_settings(&app);
}

#[tauri::command]
fn toggle_provider(app: AppHandle) {
    let st = app.state::<AppState>();
    {
        let mut settings = st.settings.lock().unwrap();
        settings.provider = if settings.provider == "codex" { "claude" } else { "codex" }.into();
    }
    st.save_settings_now();
    emit_settings(&app);
}

#[tauri::command]
fn toggle_panel(app: AppHandle) {
    windows::toggle_panel(&app, None);
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    windows::hide_panel(&app);
}

/// Abre o painel já na tela de configurações (botão da ilha).
#[tauri::command]
fn open_settings(app: AppHandle) {
    windows::show_panel(&app, None);
    let _ = app.emit_to(windows::PANEL, "open-settings", ());
}

/// Abre o Claude Code (CLI) num terminal. Ao iniciar de forma interativa ele renova o login
/// salvo em `~/.claude/.credentials.json`; o app percebe a renovação no próximo ciclo (≤ 30 s).
/// O app em si nunca renova nem grava a credencial.
#[tauri::command]
fn renew_login() -> Result<(), String> {
    diag_log("abrindo o Claude Code no terminal para renovar o login");
    #[cfg(target_os = "windows")]
    let res = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // na pasta do usuário (e não na pasta de onde o app foi iniciado, ex.: System32 no autostart)
        let mut c = std::process::Command::new("cmd");
        if let Some(home) = dirs::home_dir() {
            c.current_dir(home);
        }
        c.args(["/C", "start", "Claude Code - renovar login", "cmd", "/K", "claude"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let res = std::process::Command::new("osascript")
        .args(["-e", "tell application \"Terminal\" to do script \"claude\"", "-e", "tell application \"Terminal\" to activate"])
        .spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let res = std::process::Command::new("x-terminal-emulator").args(["-e", "claude"]).spawn();
    res.map(|_| ()).map_err(|e| format!("Não foi possível abrir o terminal: {e}. Abra um terminal e rode `claude`."))
}

#[tauri::command]
fn reset_widget_position(app: AppHandle) {
    windows::default_widget_position(&app);
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.state::<AppState>().save_settings_now();
    app.exit(0);
}

/// O registro de inicialização aponta para um executável de desenvolvimento (`target\debug`)?
/// Só o próprio build de desenvolvimento chama isto, para não apagar o registro do .exe de release.
fn is_dev_autostart_entry() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let Ok(out) = out else { return false };
        let txt = String::from_utf8_lossy(&out.stdout).to_lowercase();
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string().to_lowercase())
            .unwrap_or_default();
        !exe.is_empty() && txt.contains(&exe)
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

// ---------------- bootstrap ----------------

/// Registro de diagnóstico em `%LOCALAPPDATA%\com.claudeusage.widget\erro.log` (ou equivalente):
/// o .exe de release não tem console, então falhas na inicialização ficariam invisíveis.
pub(crate) fn diag_log(msg: &str) {
    let Some(dir) = dirs::data_local_dir().map(|d| d.join("com.claudeusage.widget")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("erro.log");
    // mantém o arquivo pequeno
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
    }
}

/// Mesmas pastas que `app_config_dir()` / `app_cache_dir()` do Tauri (%APPDATA% e %LOCALAPPDATA%
/// no Windows; Application Support e Caches no macOS), calculadas antes do app existir.
fn app_dirs() -> (std::path::PathBuf, std::path::PathBuf) {
    const ID: &str = "com.claudeusage.widget";
    let config = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(ID);
    let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join(ID);
    (config, cache)
}

pub fn run() {
    std::panic::set_hook(Box::new(|info| {
        startup::fatal(&format!("Erro inesperado (v{})", env!("CARGO_PKG_VERSION")), &info.to_string());
    }));
    diag_log(&format!(
        "iniciando v{} (pid {}){}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        startup::free_memory_mb().map(|m| format!(" · memória livre {m} MB")).unwrap_or_default()
    ));

    // Sem o WebView2 a janela não tem como existir: avisa em vez de sumir em silêncio.
    if !startup::check_webview() {
        std::process::exit(1);
    }

    // O estado é registrado ANTES de criar as janelas: no Tauri 2 o `setup` só roda depois que o
    // loop de eventos inicia, e a interface pode chamar comandos (get_settings, get_snapshot)
    // antes disso — o que causava "state() called before manage()" e fechava o app.
    let (config_dir, cache_dir) = app_dirs();
    diag_log(&format!("config: {} · cache: {}", config_dir.display(), cache_dir.display()));

    let app = tauri::Builder::default()
        .manage(AppState::new(config_dir, cache_dir))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            diag_log("app aberto de novo enquanto já rodava: mostrando o painel desta instância");
            windows::show_panel(app, None);
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(presentation::plugin())
        // o WebView acabou de se acoplar à janela: some com o "fantasma" (ver windows::hide_ghost)
        .on_page_load(|webview, _| {
            if let Some(w) = webview.app_handle().get_webview_window(webview.label()) {
                windows::hide_ghost(&w);
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            // Iniciar com o sistema
            {
                let st = app.state::<AppState>();
                let first = !st.settings.lock().unwrap().initialized;
                let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                if DEV_BUILD {
                    // Um build de desenvolvimento antigo pode ter se registrado (versões anteriores
                    // faziam isso na primeira execução): remove, para o Windows não abrir a página
                    // de localhost ao ligar. O .exe de release se registra de novo ao abrir.
                    if enabled && is_dev_autostart_entry() {
                        let _ = app.autolaunch().disable();
                        diag_log("removido o registro de inicialização do build de desenvolvimento");
                    }
                } else {
                    // o app se chamava "Claude Usage": a entrada antiga sai e, se existia, a opção
                    // continua ligada com o nome atual (sem abrir duas vezes no login)
                    let had_old = legacy::take_old_autostart();
                    if had_old {
                        diag_log("registro antigo de inicialização (\"Claude Usage\") migrado para o nome atual");
                    }
                    if first || enabled || had_old {
                        // primeira execução: liga; nas outras, regrava o caminho deste .exe (ele pode ter
                        // sido movido, renomeado ou o registro podia apontar para um build de desenvolvimento)
                        apply_autostart(app.handle(), true);
                    }
                }
                if first {
                    st.settings.lock().unwrap().initialized = true;
                    st.save_settings_now();
                }
            }

            tray::create(&handle)?;

            if app.state::<AppState>().settings.lock().unwrap().widget_visible {
                windows::set_widget_visible(&handle, true);
            } else {
                // sem widget: mostra o painel uma vez para o usuário saber que o app abriu
                let launched_by_autostart = std::env::args().any(|a| a == "--autostart");
                if !launched_by_autostart {
                    windows::show_panel(&handle, None);
                }
            }

            // Eventos de janela
            if let Some(widget) = app.get_webview_window(windows::WIDGET) {
                let h = handle.clone();
                widget.on_window_event(move |ev| {
                    if let WindowEvent::Moved(pos) = ev {
                        let st = h.state::<AppState>();
                        let mut s = st.settings.lock().unwrap();
                        // a ilha é posicionada pelo app: só o arrasto do círculo vira posição salva
                        if s.widget_style == "circle" {
                            s.widget_pos = Some((pos.x, pos.y));
                            drop(s);
                            st.mark_settings_dirty();
                        }
                    }
                });
            }
            if let Some(panel) = app.get_webview_window(windows::PANEL) {
                let h = handle.clone();
                panel.on_window_event(move |ev| match ev {
                    WindowEvent::Focused(false) => {
                        let pinned = h.state::<AppState>().pinned.load(std::sync::atomic::Ordering::Relaxed);
                        if !pinned {
                            windows::hide_panel(&h);
                        }
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        windows::hide_panel(&h);
                    }
                    _ => {}
                });
            }

            presentation::apply(&handle);
            presentation::start_watcher(handle.clone());
            island::start_watcher(handle.clone());
            tauri::async_runtime::spawn(poller::run(handle));
            startup::UI_READY.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            refresh_now,
            get_settings,
            update_settings,
            set_autostart,
            set_pinned,
            toggle_panel,
            toggle_provider,
            hide_panel,
            open_settings,
            reset_widget_position,
            renew_login,
            quit_app
        ])
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            startup::fatal("Não foi possível criar a janela do app", &e.to_string());
            std::process::exit(1);
        }
    };
    diag_log("janela e bandeja criadas");
    app.run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                // Fechar janelas não encerra o app (ele vive na bandeja); só "Sair" encerra.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                diag_log("encerrado");
                if let Some(st) = app.try_state::<AppState>() {
                    st.save_settings_now();
                    st.save_cache();
                }
            }
            _ => {}
        });
}
