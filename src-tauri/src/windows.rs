//! Posicionamento e visibilidade do widget flutuante e do painel.

use crate::state::AppState;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalRect, WebviewWindow};

pub const WIDGET: &str = "widget";
pub const PANEL: &str = "panel";
const MARGIN: f64 = 20.0; // px lógicos
const GAP: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Rect {
    fn from_phys(r: &PhysicalRect<i32, u32>) -> Self {
        Rect { x: r.position.x, y: r.position.y, w: r.size.width as i32, h: r.size.height as i32 }
    }
    pub(crate) fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub(crate) fn cx(&self) -> i32 {
        self.x + self.w / 2
    }
    pub(crate) fn cy(&self) -> i32 {
        self.y + self.h / 2
    }
    /// Retângulo aumentado `d` px em cada lado (tolerância de hover).
    pub(crate) fn inflate(&self, d: i32) -> Rect {
        Rect { x: self.x - d, y: self.y - d, w: self.w + 2 * d, h: self.h + 2 * d }
    }
}

/// Mantém (x, y) de uma janela w×h dentro da área útil, com margem.
pub(crate) fn clamp_into(wa: Rect, margin: i32, x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    (
        x.clamp(wa.x + margin, (wa.x + wa.w - w - margin).max(wa.x + margin)),
        y.clamp(wa.y + margin, (wa.y + wa.h - h - margin).max(wa.y + margin)),
    )
}

/// Área útil (sem barra de tarefas / dock) do monitor que contém o ponto, ou do principal.
pub(crate) fn work_area_at(app: &AppHandle, x: i32, y: i32) -> Option<(Rect, f64)> {
    if let Ok(mons) = app.available_monitors() {
        for m in mons {
            let full = Rect {
                x: m.position().x,
                y: m.position().y,
                w: m.size().width as i32,
                h: m.size().height as i32,
            };
            if full.contains(x, y) {
                return Some((Rect::from_phys(m.work_area()), m.scale_factor()));
            }
        }
    }
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| (Rect::from_phys(m.work_area()), m.scale_factor()))
}

pub(crate) fn primary_work_area(app: &AppHandle) -> Option<(Rect, f64)> {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| (Rect::from_phys(m.work_area()), m.scale_factor()))
}

/// Tamanho físico da janela; se ainda não foi realizada (tamanho 0), usa o tamanho lógico configurado.
fn phys_size(w: &WebviewWindow, logical: (f64, f64)) -> (i32, i32) {
    let scale = w.scale_factor().unwrap_or(1.0);
    match w.outer_size() {
        Ok(s) if s.width > 0 && s.height > 0 => (s.width as i32, s.height as i32),
        _ => ((logical.0 * scale) as i32, (logical.1 * scale) as i32),
    }
}
const WIDGET_SIZE: (f64, f64) = (104.0, 104.0);
const PANEL_SIZE: (f64, f64) = (420.0, 560.0);

/// Tamanho lógico da janela do widget no estilo atual (círculo ou ilha).
fn widget_size(app: &AppHandle) -> (f64, f64) {
    if crate::island::enabled(app) { crate::island::WINDOW } else { WIDGET_SIZE }
}

/// Garante o tamanho lógico fixo da janela no monitor em que ela está agora.
/// Ao trocar de monitor com escalas diferentes (notebook 150% + monitor 100%, dock/undock,
/// mudança de escala do Windows), a janela pode manter o tamanho físico antigo e ficar menor
/// (ou maior) que o normal. Também zera qualquer zoom da página.
pub(crate) fn ensure_size(w: &WebviewWindow, logical: (f64, f64)) {
    let scale = w.scale_factor().unwrap_or(1.0);
    let want = ((logical.0 * scale).round() as i64, (logical.1 * scale).round() as i64);
    if let Ok(cur) = w.inner_size() {
        let (cw, ch) = (cur.width as i64, cur.height as i64);
        if (cw - want.0).abs() > 2 || (ch - want.1).abs() > 2 {
            crate::diag_log(&format!(
                "janela '{}' com {cw}×{ch} px; esperado {}×{} (escala {scale}) — corrigindo",
                w.label(),
                want.0,
                want.1
            ));
            let _ = w.set_size(LogicalSize::new(logical.0, logical.1));
        }
    }
    let _ = w.set_zoom(1.0);
}


pub(crate) fn window_rect(w: &WebviewWindow) -> Option<Rect> {
    let p = w.outer_position().ok()?;
    let s = w.outer_size().ok()?;
    Some(Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 })
}

/// Coloca o widget na posição salva (se ainda estiver em algum monitor) ou no canto inferior direito.
/// No estilo ilha, centraliza na borda escolhida.
pub fn place_widget(app: &AppHandle) {
    if crate::island::enabled(app) {
        crate::island::place(app);
        return;
    }
    let Some(widget) = app.get_webview_window(WIDGET) else { return };
    let saved = app.state::<AppState>().settings.lock().unwrap().widget_pos;
    let (sw, sh) = phys_size(&widget, WIDGET_SIZE);

    if let Some((x, y)) = saved {
        let cx = x + sw / 2;
        let cy = y + sh / 2;
        if let Some((wa, _)) = work_area_at(app, cx, cy) {
            if wa.contains(cx, cy) {
                let _ = widget.set_position(PhysicalPosition::new(x, y));
                return;
            }
        }
    }
    default_widget_position(app);
}

pub fn default_widget_position(app: &AppHandle) {
    if crate::island::enabled(app) {
        crate::island::place(app);
        return;
    }
    let Some(widget) = app.get_webview_window(WIDGET) else { return };
    let Some((wa, scale)) = primary_work_area(app) else { return };
    let (sw, sh) = phys_size(&widget, WIDGET_SIZE);
    let m = (MARGIN * scale) as i32;
    let x = wa.x + wa.w - sw - m;
    let y = wa.y + wa.h - sh - m;
    let _ = widget.set_position(PhysicalPosition::new(x, y));
    let st = app.state::<AppState>();
    st.settings.lock().unwrap().widget_pos = Some((x, y));
    st.mark_settings_dirty();
}

pub fn set_widget_visible(app: &AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window(WIDGET) {
        if visible {
            ensure_size(&w, widget_size(app));
            place_widget(app);
            let _ = w.show();
            ensure_size(&w, widget_size(app));
            // alguns gerenciadores de janela ignoram a posição antes de a janela existir
            place_widget(app);
            let _ = w.set_always_on_top(true);
            crate::island::sync_mode(app);
        } else {
            let _ = w.hide();
        }
    }
}

/// Windows: ao acoplar o WebView2 a uma janela criada escondida, a janela pode continuar
/// desenhada na tela mesmo sem `WS_VISIBLE`: um "fantasma" branco, com barra de título e sem
/// conteúdo, que não recebe cliques e só sumia depois de abrir e fechar o painel.
/// Um `SW_HIDE` sozinho não resolve (sem `WS_VISIBLE` o Windows acha que ela já está oculta):
/// devolvemos o bit e escondemos de verdade. Chamado no `on_page_load` (início e fim do
/// carregamento), quando o WebView já está acoplado.
#[cfg(target_os = "windows")]
pub fn hide_ghost(w: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, IsWindowVisible, SetWindowLongW, ShowWindow, GWL_STYLE, SW_HIDE, WS_VISIBLE,
    };
    // só mexe no que o app considera oculto
    if w.is_visible().unwrap_or(true) {
        return;
    }
    let Ok(hwnd) = w.hwnd() else { return };
    let h = hwnd.0 as _;
    // SAFETY: HWND válido da própria janela, na thread principal; só mexe na visibilidade dela.
    unsafe {
        let style = GetWindowLongW(h, GWL_STYLE);
        SetWindowLongW(h, GWL_STYLE, style | WS_VISIBLE as i32);
        ShowWindow(h, SW_HIDE);
        if IsWindowVisible(h) != 0 {
            ShowWindow(h, SW_HIDE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn hide_ghost(_w: &WebviewWindow) {}

/// Posiciona o painel junto ao widget (ou junto ao ícone da bandeja / canto da tela).
fn position_panel(app: &AppHandle, panel: &WebviewWindow, anchor: Option<Rect>) {
    let widget_rect = app
        .get_webview_window(WIDGET)
        .filter(|w| w.is_visible().unwrap_or(false))
        .and_then(|w| window_rect(&w));
    let anchor = anchor.or(widget_rect);

    let (wa, scale) = match anchor {
        Some(a) => work_area_at(app, a.cx(), a.cy()),
        None => primary_work_area(app),
    }
    .unwrap_or((Rect { x: 0, y: 0, w: 1280, h: 800 }, 1.0));

    let (pw, ph) = phys_size(panel, PANEL_SIZE);
    let gap = (GAP * scale) as i32;
    let margin = (MARGIN * scale) as i32 / 2;

    // Ilha: o painel abre centralizado junto à pílula, do lado de dentro da tela.
    if anchor_is_widget(app, anchor, widget_rect) {
        if let Some((x, y)) = crate::island::panel_position(app, (pw, ph), gap) {
            let (x, y) = clamp_into(wa, margin, x, y, pw, ph);
            let _ = panel.set_position(PhysicalPosition::new(x, y));
            return;
        }
    }

    let (mut x, mut y) = match anchor {
        Some(a) => {
            let x = if a.cx() >= wa.cx() { a.x + a.w - pw } else { a.x };
            let y = if a.cy() >= wa.cy() { a.y - ph - gap } else { a.y + a.h + gap };
            (x, y)
        }
        None => {
            // sem âncora: canto inferior direito (Windows) / superior direito (macOS)
            let x = wa.x + wa.w - pw - margin;
            let y = if cfg!(target_os = "macos") { wa.y + margin } else { wa.y + wa.h - ph - margin };
            (x, y)
        }
    };
    // Se não couber acima/abaixo, encosta ao lado do widget.
    if y < wa.y || y + ph > wa.y + wa.h {
        if let Some(a) = anchor {
            y = (a.y + a.h - ph).max(wa.y + margin);
            x = if a.cx() >= wa.cx() { a.x - pw - gap } else { a.x + a.w + gap };
        }
    }
    (x, y) = clamp_into(wa, margin, x, y, pw, ph);
    let _ = panel.set_position(PhysicalPosition::new(x, y));
}

/// O painel está sendo ancorado no próprio widget (e não na bandeja)?
fn anchor_is_widget(app: &AppHandle, anchor: Option<Rect>, widget_rect: Option<Rect>) -> bool {
    match (anchor, widget_rect) {
        (Some(a), Some(w)) => a.x == w.x && a.y == w.y && crate::island::enabled(app),
        _ => false,
    }
}

pub fn show_panel(app: &AppHandle, anchor: Option<Rect>) {
    let Some(panel) = app.get_webview_window(PANEL) else { return };
    position_panel(app, &panel, anchor);
    let _ = panel.show();
    // já no monitor certo: confere o tamanho (escala do monitor) e reposiciona com o tamanho final
    ensure_size(&panel, PANEL_SIZE);
    position_panel(app, &panel, anchor);
    let _ = panel.unminimize();
    let _ = panel.set_focus();
    let _ = panel.set_always_on_top(true);
    let _ = app.emit_to(PANEL, "panel-shown", ());
    let _ = app.emit_to(WIDGET, "panel-visibility", true);
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        if panel.is_visible().unwrap_or(false) {
            let _ = panel.hide();
            app.state::<AppState>().note_panel_hidden();
        }
    }
    let _ = app.emit_to(WIDGET, "panel-visibility", false);
}

pub fn toggle_panel(app: &AppHandle, anchor: Option<Rect>) {
    let Some(panel) = app.get_webview_window(PANEL) else { return };
    let st = app.state::<AppState>();
    if panel.is_visible().unwrap_or(false) {
        hide_panel(app);
    } else if st.panel_hidden_recently() {
        // O clique no widget tirou o foco do painel (que acabou de se esconder): não reabrir.
    } else {
        show_panel(app, anchor);
    }
}

pub fn tray_anchor(position: PhysicalPosition<f64>, size: tauri::PhysicalSize<u32>) -> Rect {
    Rect { x: position.x as i32, y: position.y as i32, w: size.width as i32, h: size.height as i32 }
}

