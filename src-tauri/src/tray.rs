//! Ícone na bandeja (Windows) / barra de menus (macOS).

use crate::state::AppState;
use crate::windows::{self, tray_anchor};
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

pub const TRAY_ID: &str = "main";

pub struct TrayItems {
    pub widget: CheckMenuItem<Wry>,
    pub share: CheckMenuItem<Wry>,
    pub autostart: CheckMenuItem<Wry>,
}
pub struct TrayHandles(pub Mutex<Option<TrayItems>>);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let (widget_visible, on_share) = {
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        (s.widget_visible, !s.hide_on_share)
    };
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);

    let open = MenuItem::with_id(app, "open", "Abrir painel de uso", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Atualizar agora", true, None::<&str>)?;
    let widget = CheckMenuItem::with_id(app, "widget", "Mostrar widget flutuante", true, widget_visible, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset_pos", "Recolocar widget no canto", true, None::<&str>)?;
    let share = CheckMenuItem::with_id(app, "share", "Aparecer em transmissão de tela", true, on_share, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", "Iniciar com o sistema", true, autostart_on, None::<&str>)?;
    let web = MenuItem::with_id(app, "web", "Abrir claude.ai › Uso", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &sep1, &widget, &reset, &share, &autostart, &web, &sep2, &quit])?;

    app.manage(TrayHandles(Mutex::new(Some(TrayItems { widget: widget.clone(), share: share.clone(), autostart: autostart.clone() }))));

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Claude Usage")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => windows::show_panel(app, None),
            "refresh" => request_refresh(app),
            "widget" => {
                let st = app.state::<AppState>();
                let v = {
                    let mut s = st.settings.lock().unwrap();
                    s.widget_visible = !s.widget_visible;
                    s.widget_visible
                };
                st.save_settings_now();
                windows::set_widget_visible(app, v);
                sync_checks(app);
                crate::emit_settings(app);
            }
            "reset_pos" => {
                windows::default_widget_position(app);
                windows::set_widget_visible(app, true);
                let st = app.state::<AppState>();
                st.settings.lock().unwrap().widget_visible = true;
                st.save_settings_now();
                sync_checks(app);
            }
            "share" => {
                let hidden = app.state::<AppState>().settings.lock().unwrap().hide_on_share;
                crate::presentation::set_visible_on_share(app, hidden);
            }
            "autostart" => {
                let on = app.autolaunch().is_enabled().unwrap_or(false);
                crate::apply_autostart(app, !on);
                sync_checks(app);
                crate::emit_settings(app);
            }
            "web" => {
                use tauri_plugin_opener::OpenerExt;
                let _ = app.opener().open_url("https://claude.ai/settings/usage", None::<&str>);
            }
            "quit" => {
                app.state::<AppState>().save_settings_now();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                let app = tray.app_handle();
                let scale = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
                let pos = rect.position.to_physical::<f64>(scale);
                let size = rect.size.to_physical::<u32>(scale);
                windows::toggle_panel(app, Some(tray_anchor(pos, size)));
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn request_refresh(app: &AppHandle) {
    let st = app.state::<AppState>();
    st.force_api.store(true, std::sync::atomic::Ordering::Relaxed);
    st.wake.notify_one();
}

pub fn sync_checks(app: &AppHandle) {
    let (visible, on_share) = {
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        (s.widget_visible, !s.hide_on_share)
    };
    let auto = app.autolaunch().is_enabled().unwrap_or(false);
    if let Some(h) = app.try_state::<TrayHandles>() {
        if let Some(items) = h.0.lock().unwrap().as_ref() {
            let _ = items.widget.set_checked(visible);
            let _ = items.share.set_checked(on_share);
            let _ = items.autostart.set_checked(auto);
        }
    }
}

/// Atualiza tooltip (Windows) e título na barra de menus (macOS) com os percentuais.
pub fn update(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let st = app.state::<AppState>();
    let snap = st.snapshot.lock().unwrap();
    let u = snap.usage.as_ref();
    let five = u.and_then(|u| crate::api::utilization(u, "five_hour"));
    let week = u.and_then(|u| crate::api::utilization(u, "seven_day"));
    let mut tip = String::from("Claude Usage");
    if let Some(f) = five {
        tip.push_str(&format!("\nSessão (5h): {:.0}%", f.floor()));
    }
    if let Some(w) = week {
        tip.push_str(&format!("\nSemanal: {:.0}%", w.floor()));
    }
    if let Some(e) = &snap.error {
        if snap.usage.is_none() {
            tip.push_str(&format!("\n⚠ {}", e.message.chars().take(60).collect::<String>()));
        }
    }
    drop(snap);
    let _ = tray.set_tooltip(Some(tip));
    #[cfg(target_os = "macos")]
    {
        let _ = tray.set_title(five.map(|f| format!("{:.0}%", f.floor())));
    }
}
