//! Renovação automática do login do Claude Code.
//!
//! O Claude Code só renova o próprio login (`~/.claude/.credentials.json`) quando roda em modo
//! interativo, ou seja, ligado a um terminal. Para não abrir janela nenhuma, o app inicia o
//! Claude Code dentro de um pseudo-terminal invisível (ConPTY no Windows, pty no macOS/Linux),
//! espera a credencial ser renovada e encerra o processo. O app nunca renova nem grava a
//! credencial: quem faz isso é o próprio Claude Code.

use crate::state::AppState;
use crate::{credentials, diag_log, poller};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const TIMEOUT: Duration = Duration::from_secs(75);
/// intervalo mínimo entre duas tentativas automáticas
const COOLDOWN: Duration = Duration::from_secs(10 * 60);
static RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_TRY: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
/// sinaliza ao poller que a próxima leitura é a primeira depois de uma renovação (para o log)
pub static JUST_RENEWED: AtomicBool = AtomicBool::new(false);

/// "62% · reinicia 2026-10-21T22:00" — estado da sessão de 5h, para comparar antes/depois no log
pub fn session_summary(app: &AppHandle) -> String {
    let st = app.state::<AppState>();
    let snap = st.snapshot.lock().unwrap();
    let u = snap.usage.as_ref();
    let pct = u.and_then(|u| crate::api::utilization(u, "five_hour"));
    let reset = u.and_then(|u| crate::api::resets_at(u, "five_hour"));
    match (pct, reset) {
        (Some(p), Some(r)) => format!("{p:.0}% · reinicia {r}"),
        (Some(p), None) => format!("{p:.0}% · sem janela ativa"),
        _ => "sem dados".into(),
    }
}

/// Inicia a renovação em segundo plano (no máximo uma por vez).
/// `failed_exp`: o `expiresAt` da credencial que falhou — renovação = valor diferente e válido.
pub fn start(app: AppHandle, failed_exp: Option<i64>) {
    {
        let mut last = LAST_TRY.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < COOLDOWN) {
            return;
        }
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        *last = Some(Instant::now());
    }
    diag_log(&format!("renovação automática: sessão 5h antes = {}", session_summary(&app)));
    set_status(&app, true, false);
    std::thread::spawn(move || {
        let result = run_hidden(&app, failed_exp);
        let ok = matches!(result, Ok(true));
        match &result {
            Ok(true) => diag_log("renovação automática: login renovado pelo Claude Code"),
            Ok(false) => diag_log("renovação automática: o Claude Code iniciou mas não renovou o login a tempo"),
            Err(e) => diag_log(&format!("renovação automática falhou: {e}")),
        }
        set_status(&app, false, !ok);
        RUNNING.store(false, Ordering::SeqCst);
        if ok {
            JUST_RENEWED.store(true, Ordering::SeqCst);
            // consulta a API já com o login novo
            let st = app.state::<AppState>();
            st.force_api.store(true, Ordering::Relaxed);
            st.wake.notify_one();
        }
    });
}

fn set_status(app: &AppHandle, renewing: bool, failed: bool) {
    {
        let st = app.state::<AppState>();
        let mut snap = st.snapshot.lock().unwrap();
        snap.renewing = renewing;
        snap.renew_failed = failed;
    }
    poller::emit_snapshot(app);
}

fn renewed(failed_exp: Option<i64>) -> bool {
    credentials::load()
        .map(|c| !c.is_expired() && c.expires_at != failed_exp)
        .unwrap_or(false)
}

fn command() -> CommandBuilder {
    #[cfg(target_os = "windows")]
    {
        // via cmd para achar claude.exe / claude.cmd no PATH
        let mut c = CommandBuilder::new("cmd.exe");
        c.args(["/D", "/C", "claude"]);
        c
    }
    #[cfg(not(target_os = "windows"))]
    {
        // shell de login: apps gráficos no macOS não herdam o PATH do terminal
        let shell = if std::path::Path::new("/bin/zsh").exists() { "/bin/zsh" } else { "/bin/bash" };
        let mut c = CommandBuilder::new(shell);
        c.args(["-lc", "claude"]);
        c
    }
}

fn run_hidden(app: &AppHandle, failed_exp: Option<i64>) -> Result<bool, String> {
    diag_log("renovação automática: iniciando o Claude Code em segundo plano");
    // pasta própria e vazia, para não abrir o Claude Code dentro de nenhum projeto
    let work = app.state::<AppState>().cache_dir.join("renovacao-login");
    let _ = std::fs::create_dir_all(&work);

    let pty = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("pty: {e}"))?;
    let mut cmd = command();
    cmd.cwd(&work);
    // sem atualização automática durante esses segundos (o resto do tráfego fica ligado: é numa
    // dessas chamadas de inicialização que o Claude Code percebe o token vencido e o renova)
    cmd.env("DISABLE_AUTOUPDATER", "1");
    cmd.env("TERM", "xterm-256color");
    let mut child = pty.slave.spawn_command(cmd).map_err(|e| format!("não iniciou o claude: {e}"))?;
    drop(pty.slave);

    // drena a saída (senão o processo trava quando o buffer enche); guarda o fim dela para o log
    let tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    if let Ok(mut reader) = pty.master.try_clone_reader() {
        let tail = tail.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = tail.lock().unwrap();
                t.extend_from_slice(&buf[..n]);
                if t.len() > 16 * 1024 {
                    let cut = t.len() - 8 * 1024;
                    t.drain(..cut);
                }
            }
        });
    }
    let mut writer = pty.master.take_writer().ok();

    let t0 = Instant::now();
    let mut ok = false;
    let mut enters = 0;
    while t0.elapsed() < TIMEOUT {
        std::thread::sleep(Duration::from_secs(2));
        // Na primeira vez numa pasta o Claude Code pergunta "Do you trust the files in this folder?"
        // (a pasta é nossa e está vazia): Enter aceita a opção padrão e deixa ele seguir.
        let secs = t0.elapsed().as_secs();
        if enters < 3 && secs >= 6 * (enters + 1) {
            enters += 1;
            if let Some(w) = writer.as_mut() {
                let _ = w.write_all(b"\r");
                let _ = w.flush();
            }
        }
        if renewed(failed_exp) {
            ok = true;
            // dá um instante para o Claude Code terminar de gravar
            std::thread::sleep(Duration::from_millis(800));
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            // saiu sozinho (ex.: claude não instalado)
            ok = renewed(failed_exp);
            break;
        }
    }

    // encerra o Claude Code e tudo que ele abriu
    #[cfg(target_os = "windows")]
    if let Some(pid) = child.process_id() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    // macOS/Linux: o pty cria uma sessão nova; mata o grupo inteiro (shell + claude + filhos)
    #[cfg(not(target_os = "windows"))]
    if let Some(pid) = child.process_id() {
        let _ = std::process::Command::new("kill").args(["-9", "--", &format!("-{pid}")]).output();
    }
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    if !ok {
        // o que o Claude Code mostrou (tela de boas-vindas / pergunta), para entender por que não renovou
        let raw = tail.lock().unwrap().clone();
        let txt = strip_ansi(&String::from_utf8_lossy(&raw));
        let txt: String = txt.chars().rev().take(700).collect::<Vec<_>>().into_iter().rev().collect();
        diag_log(&format!("renovação automática: saída do Claude Code (fim): {}", txt.trim()));
    }
    Ok(ok)
}

/// Remove sequências de escape ANSI e caracteres de controle (texto limpo para o log).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for d in chars.by_ref() {
                        if ('@'..='~').contains(&d) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(d) = chars.next() {
                        if d == '\x07' {
                            break;
                        }
                        if d == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c == '\n' || c == ' ' || !c.is_control() {
            out.push(c);
        }
    }
    // comprime linhas vazias e espaços repetidos
    let mut res = String::new();
    let mut last_blank = false;
    for line in out.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            if !last_blank {
                res.push('\n');
            }
            last_blank = true;
        } else {
            res.push_str(&l);
            res.push('\n');
            last_blank = false;
        }
    }
    res
}

#[cfg(test)]
mod tests {
    #[test]
    fn strip_ansi_keeps_text() {
        let s = "\x1b[1mWelcome\x1b[0m   to \x1b]0;title\x07Claude\r\n\r\n\r\nDo you trust?";
        assert_eq!(super::strip_ansi(s), "Welcome to Claude\n\nDo you trust?\n");
    }
}
