//! Atalho global e "modo apresentação" do widget.
//!
//!  * Atalho Ctrl+Alt+U (⌘⌥U no macOS) abre/fecha o painel de qualquer lugar.
//!  * Opção "aparecer em transmissão de tela": desligada, o widget e o painel ficam fora de
//!    compartilhamentos de tela, gravações e prints (continuam visíveis para você). No Windows usa
//!    `WDA_EXCLUDEFROMCAPTURE`, no macOS `NSWindowSharingNone`.
//!  * Windows: o widget some enquanto outro app está em tela cheia ou apresentando
//!    (PowerPoint, vídeo em tela cheia, jogo) e volta sozinho depois.

use crate::state::AppState;
use crate::{diag_log, windows};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
pub const SHORTCUT: &str = "Command+Alt+U";
#[cfg(not(target_os = "macos"))]
pub const SHORTCUT: &str = "Control+Alt+U";

/// o widget está escondido por causa de um app em tela cheia (e não por escolha do usuário)
static HIDDEN_FOR_FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// o atalho não pôde ser registrado (provavelmente outro app já usa a mesma combinação)
pub static SHORTCUT_FAILED: AtomicBool = AtomicBool::new(false);

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut: &Shortcut, event| {
            if event.state == ShortcutState::Pressed {
                windows::toggle_panel(app, None);
            }
        })
        .build()
}

/// Aplica as preferências de atalho e de captura de tela. Chamado na inicialização e quando as
/// configurações mudam.
pub fn apply(app: &AppHandle) {
    let (shortcut_on, hide_on_share) = {
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        (s.shortcut, s.hide_on_share)
    };

    let gs = app.global_shortcut();
    let registered = gs.is_registered(SHORTCUT);
    if shortcut_on && !registered {
        match gs.register(SHORTCUT) {
            Ok(()) => SHORTCUT_FAILED.store(false, Ordering::Relaxed),
            Err(e) => {
                SHORTCUT_FAILED.store(true, Ordering::Relaxed);
                diag_log(&format!("atalho {SHORTCUT} não registrado: {e}"));
            }
        }
    } else if !shortcut_on {
        if registered {
            let _ = gs.unregister(SHORTCUT);
        }
        SHORTCUT_FAILED.store(false, Ordering::Relaxed);
    }

    // widget e painel: nenhum dos dois aparece na transmissão quando a opção está desligada
    for label in [windows::WIDGET, windows::PANEL] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_content_protected(hide_on_share);
        }
    }
}

/// Liga/desliga "aparecer em transmissão de tela" (bandeja e configurações usam isto).
pub fn set_visible_on_share(app: &AppHandle, visible: bool) {
    let st = app.state::<AppState>();
    st.settings.lock().unwrap().hide_on_share = !visible;
    st.save_settings_now();
    apply(app);
    crate::tray::sync_checks(app);
    crate::emit_settings(app);
}

/// Windows: algum app em tela cheia / apresentação no primeiro plano?
#[cfg(target_os = "windows")]
fn fullscreen_busy() -> bool {
    use windows_sys::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    let mut state = 0;
    // SAFETY: chamada simples que só escreve no inteiro passado
    let hr = unsafe { SHQueryUserNotificationState(&mut state) };
    hr >= 0 && matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
}

#[cfg(not(target_os = "windows"))]
fn fullscreen_busy() -> bool {
    false
}

/// Verifica a cada 2 s se o widget deve sumir/voltar por causa de tela cheia.
pub fn start_watcher(app: AppHandle) {
    if !cfg!(target_os = "windows") {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let (want_visible, hide_fs) = {
            let s = app.state::<AppState>().settings.lock().unwrap().clone();
            (s.widget_visible, s.hide_fullscreen)
        };
        let busy = hide_fs && want_visible && fullscreen_busy();
        let hidden = HIDDEN_FOR_FULLSCREEN.load(Ordering::Relaxed);
        if busy && !hidden {
            HIDDEN_FOR_FULLSCREEN.store(true, Ordering::Relaxed);
            let h = app.clone();
            let _ = app.run_on_main_thread(move || windows::set_widget_visible(&h, false));
        } else if !busy && hidden {
            HIDDEN_FOR_FULLSCREEN.store(false, Ordering::Relaxed);
            if want_visible {
                let h = app.clone();
                let _ = app.run_on_main_thread(move || windows::set_widget_visible(&h, true));
            }
        }
    });
}
