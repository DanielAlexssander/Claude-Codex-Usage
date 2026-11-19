//! Estilo "ilha": a janela do widget vira uma pílula centralizada numa borda da tela.
//!
//! A janela tem tamanho fixo (o da ilha aberta) e é transparente. Fechada, ela ignora o mouse
//! (os cliques passam para o que está embaixo) e uma thread acompanha o cursor: quando ele para
//! sobre a pílula, a ilha abre, passa a receber cliques e a interface anima a expansão.
//! Assim a animação é só CSS, sem redimensionar nem mover a janela.

use crate::state::AppState;
use crate::windows::{self, Rect};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

/// Folga entre a pílula e a borda da janela (espaço da sombra). Igual a `--isl-pad` no CSS.
const PAD: f64 = 10.0;
/// Pílula fechada no topo/base e nas laterais (px lógicos). Igual ao `island.css`.
const COLLAPSED_H: (f64, f64) = (128.0, 36.0);
const COLLAPSED_V: (f64, f64) = (36.0, 112.0);
/// Ilha aberta (px lógicos). Igual ao `island.css`.
const EXPANDED: (f64, f64) = (300.0, 176.0);
/// Janela fixa: a ilha aberta mais a folga.
pub const WINDOW: (f64, f64) = (EXPANDED.0 + 2.0 * PAD, EXPANDED.1 + 2.0 * PAD);

const TICK: Duration = Duration::from_millis(40);
const IDLE: Duration = Duration::from_millis(300);
/// O cursor precisa parar um instante sobre a pílula (evita abrir só de passar por cima).
const OPEN_DELAY: Duration = Duration::from_millis(120);
const CLOSE_DELAY: Duration = Duration::from_millis(250);
/// Tolerância ao redor da ilha aberta antes de começar a fechar (px lógicos).
const LEAVE_SLACK: f64 = 8.0;

static OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn parse(s: &str) -> Edge {
        match s {
            "bottom" => Edge::Bottom,
            "left" => Edge::Left,
            "right" => Edge::Right,
            _ => Edge::Top,
        }
    }
    fn vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

pub fn enabled(app: &AppHandle) -> bool {
    app.state::<AppState>().settings.lock().unwrap().widget_style == "island"
}

fn edge(app: &AppHandle) -> Edge {
    Edge::parse(&app.state::<AppState>().settings.lock().unwrap().island_edge)
}

fn scaled((w, h): (f64, f64), scale: f64) -> (i32, i32) {
    ((w * scale).round() as i32, (h * scale).round() as i32)
}

/// Posição da janela (w×h físicos) centralizada na borda escolhida da área útil.
pub(crate) fn window_pos(edge: Edge, wa: Rect, (w, h): (i32, i32)) -> (i32, i32) {
    match edge {
        Edge::Top => (wa.cx() - w / 2, wa.y),
        Edge::Bottom => (wa.cx() - w / 2, wa.y + wa.h - h),
        Edge::Left => (wa.x, wa.cy() - h / 2),
        Edge::Right => (wa.x + wa.w - w, wa.cy() - h / 2),
    }
}

/// Retângulo da pílula (fechada ou aberta) dentro da janela, em px físicos.
pub(crate) fn pill_rect(edge: Edge, open: bool, win: Rect, scale: f64) -> Rect {
    let size = if open {
        EXPANDED
    } else if edge.vertical() {
        COLLAPSED_V
    } else {
        COLLAPSED_H
    };
    let (w, h) = scaled(size, scale);
    let pad = (PAD * scale).round() as i32;
    let (x, y) = match edge {
        Edge::Top => (win.cx() - w / 2, win.y + pad),
        Edge::Bottom => (win.cx() - w / 2, win.y + win.h - pad - h),
        Edge::Left => (win.x + pad, win.cy() - h / 2),
        Edge::Right => (win.x + win.w - pad - w, win.cy() - h / 2),
    };
    Rect { x, y, w, h }
}

/// Monitor da ilha: o mesmo em que o widget círculo foi deixado, ou o principal.
fn work_area(app: &AppHandle) -> Option<(Rect, f64)> {
    let hint = app.state::<AppState>().settings.lock().unwrap().widget_pos;
    match hint {
        // centro aproximado do disco (52 = metade da janela de 104 px)
        Some((x, y)) => windows::work_area_at(app, x + 52, y + 52),
        None => windows::primary_work_area(app),
    }
}

/// Centraliza a janela da ilha na borda escolhida.
pub fn place(app: &AppHandle) {
    let Some(w) = app.get_webview_window(windows::WIDGET) else { return };
    let Some((wa, scale)) = work_area(app) else { return };
    let (x, y) = window_pos(edge(app), wa, scaled(WINDOW, scale));
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// Abre/fecha a ilha: aberta recebe cliques; fechada deixa o mouse passar.
fn set_open(app: &AppHandle, open: bool) {
    OPEN.store(open, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window(windows::WIDGET) {
        let _ = w.set_ignore_cursor_events(enabled(app) && !open);
    }
    let _ = app.emit_to(windows::WIDGET, "island-open", open);
}

/// Ajusta a janela ao estilo atual (ilha começa fechada; o círculo sempre recebe cliques).
pub fn sync_mode(app: &AppHandle) {
    set_open(app, false);
}

/// Onde o painel abre quando a âncora é a ilha: centralizado junto à pílula, para dentro da tela.
pub fn panel_position(app: &AppHandle, (pw, ph): (i32, i32), gap: i32) -> Option<(i32, i32)> {
    if !enabled(app) {
        return None;
    }
    let w = app.get_webview_window(windows::WIDGET)?;
    let win = windows::window_rect(&w)?;
    let scale = w.scale_factor().ok()?;
    let pill = pill_rect(edge(app), false, win, scale);
    Some(panel_next_to(edge(app), pill, (pw, ph), gap))
}

fn panel_next_to(edge: Edge, pill: Rect, (pw, ph): (i32, i32), gap: i32) -> (i32, i32) {
    match edge {
        Edge::Top => (pill.cx() - pw / 2, pill.y + pill.h + gap),
        Edge::Bottom => (pill.cx() - pw / 2, pill.y - gap - ph),
        Edge::Left => (pill.x + pill.w + gap, pill.cy() - ph / 2),
        Edge::Right => (pill.x - gap - pw, pill.cy() - ph / 2),
    }
}

/// Decide quando abrir/fechar a partir das amostras do cursor.
#[derive(Default)]
struct Hover {
    inside_since: Option<Instant>,
    outside_since: Option<Instant>,
}

impl Hover {
    /// `blocked`: o painel está aberto (a ilha fica fechada). Devolve o novo estado quando muda.
    fn step(&mut self, open: bool, inside: bool, blocked: bool, now: Instant) -> Option<bool> {
        if blocked {
            *self = Hover::default();
            return open.then_some(false);
        }
        if inside {
            self.outside_since = None;
        } else {
            self.inside_since = None;
        }
        let (since, delay, next) = match (open, inside) {
            (false, true) => (*self.inside_since.get_or_insert(now), OPEN_DELAY, true),
            (true, false) => (*self.outside_since.get_or_insert(now), CLOSE_DELAY, false),
            _ => return None,
        };
        if now.duration_since(since) >= delay {
            *self = Hover::default();
            Some(next)
        } else {
            None
        }
    }
}

/// (cursor sobre a ilha?, painel aberto?) — `None` quando a ilha não está em uso.
fn sample(app: &AppHandle) -> Option<(bool, bool)> {
    if !enabled(app) {
        return None;
    }
    let w = app.get_webview_window(windows::WIDGET)?;
    if !w.is_visible().unwrap_or(false) {
        return None;
    }
    let win = windows::window_rect(&w)?;
    let scale = w.scale_factor().ok()?;
    let cursor = app.cursor_position().ok()?;
    let open = OPEN.load(Ordering::Relaxed);
    let pill = pill_rect(edge(app), open, win, scale);
    let zone = if open { pill.inflate((LEAVE_SLACK * scale).round() as i32) } else { pill };
    let inside = zone.contains(cursor.x as i32, cursor.y as i32);
    let panel_open = app
        .get_webview_window(windows::PANEL)
        .is_some_and(|p| p.is_visible().unwrap_or(false));
    Some((inside, panel_open))
}

/// Acompanha o cursor e abre/fecha a ilha. Fora do estilo ilha, só dorme.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut hover = Hover::default();
        loop {
            let Some((inside, panel_open)) = sample(&app) else {
                hover = Hover::default();
                std::thread::sleep(IDLE);
                continue;
            };
            let open = OPEN.load(Ordering::Relaxed);
            if let Some(next) = hover.step(open, inside, panel_open, Instant::now()) {
                // grava já, para a próxima amostra usar o retângulo certo
                OPEN.store(next, Ordering::Relaxed);
                let h = app.clone();
                let _ = app.run_on_main_thread(move || set_open(&h, next));
            }
            std::thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const WA: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 };

    #[test]
    fn window_is_centered_on_each_edge() {
        let size = scaled(WINDOW, 1.0);
        assert_eq!(size, (320, 196));
        assert_eq!(window_pos(Edge::Top, WA, size), (800, 0));
        assert_eq!(window_pos(Edge::Bottom, WA, size), (800, 844));
        assert_eq!(window_pos(Edge::Left, WA, size), (0, 422));
        assert_eq!(window_pos(Edge::Right, WA, size), (1600, 422));
    }

    #[test]
    fn pill_hugs_the_chosen_edge_and_scales() {
        let win = Rect { x: 800, y: 0, w: 320, h: 196 };
        assert_eq!(pill_rect(Edge::Top, false, win, 1.0), Rect { x: 896, y: 10, w: 128, h: 36 });
        assert_eq!(pill_rect(Edge::Top, true, win, 1.0), Rect { x: 810, y: 10, w: 300, h: 176 });
        assert_eq!(pill_rect(Edge::Bottom, false, win, 1.0), Rect { x: 896, y: 150, w: 128, h: 36 });
        let side = Rect { x: 0, y: 422, w: 320, h: 196 };
        assert_eq!(pill_rect(Edge::Left, false, side, 1.0), Rect { x: 10, y: 464, w: 36, h: 112 });
        let right = Rect { x: 1600, y: 422, w: 320, h: 196 };
        assert_eq!(pill_rect(Edge::Right, false, right, 1.0), Rect { x: 1874, y: 464, w: 36, h: 112 });
        let hi = Rect { x: 0, y: 0, w: 480, h: 294 };
        assert_eq!(pill_rect(Edge::Top, false, hi, 1.5), Rect { x: 144, y: 15, w: 192, h: 54 });
    }

    #[test]
    fn panel_opens_inward_centered_on_the_pill() {
        let pill = Rect { x: 896, y: 10, w: 128, h: 36 };
        assert_eq!(panel_next_to(Edge::Top, pill, (420, 560), 10), (750, 56));
        assert_eq!(panel_next_to(Edge::Bottom, pill, (420, 560), 10), (750, -560));
        let side = Rect { x: 10, y: 464, w: 36, h: 112 };
        assert_eq!(panel_next_to(Edge::Left, side, (420, 560), 10), (56, 240));
        assert_eq!(panel_next_to(Edge::Right, side, (420, 560), 10), (-420, 240));
    }

    #[test]
    fn hover_opens_after_a_pause_and_closes_after_leaving() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hover::default();
        assert_eq!(h.step(false, true, false, ms(0)), None);
        assert_eq!(h.step(false, false, false, ms(60)), None, "passou direto: não abre");
        assert_eq!(h.step(false, true, false, ms(80)), None);
        assert_eq!(h.step(false, true, false, ms(200)), Some(true));
        assert_eq!(h.step(true, true, false, ms(240)), None);
        assert_eq!(h.step(true, false, false, ms(280)), None);
        assert_eq!(h.step(true, true, false, ms(300)), None, "voltou a tempo: continua aberta");
        assert_eq!(h.step(true, false, false, ms(320)), None);
        assert_eq!(h.step(true, false, false, ms(570)), Some(false));
    }

    #[test]
    fn open_panel_keeps_the_island_closed() {
        let t0 = Instant::now();
        let mut h = Hover::default();
        assert_eq!(h.step(true, true, true, t0), Some(false));
        assert_eq!(h.step(false, true, true, t0 + Duration::from_secs(1)), None);
    }
}
