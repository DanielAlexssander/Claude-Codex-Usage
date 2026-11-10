//! Verificações de inicialização e avisos visíveis quando o app não consegue abrir.
//!
//! O .exe de release não tem console: se algo falhar antes da janela existir, o processo
//! simplesmente some (nem aparece no Gerenciador de Tarefas). Aqui:
//!  * confere se o WebView2 (motor do Edge que desenha a interface) está instalado e, se não
//!    estiver, explica e oferece abrir a página de download da Microsoft;
//!  * mostra uma caixa de mensagem do Windows para qualquer erro antes da interface abrir,
//!    apontando para o `erro.log`.

use crate::diag_log;
use std::sync::atomic::{AtomicBool, Ordering};

/// A interface já abriu: a partir daí, erros só vão para o log (sem caixas de mensagem).
pub static UI_READY: AtomicBool = AtomicBool::new(false);

/// Instalador "Evergreen Bootstrapper" oficial do WebView2 Runtime.
#[cfg(target_os = "windows")]
const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

pub fn log_path() -> String {
    dirs::data_local_dir()
        .map(|d| d.join("com.claudeusage.widget").join("erro.log").display().to_string())
        .unwrap_or_else(|| "erro.log".into())
}

/// Memória física livre em MB (só para o log de diagnóstico).
#[cfg(target_os = "windows")]
pub fn free_memory_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut st: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: a estrutura está zerada e com dwLength preenchido, como a API exige
    let ok = unsafe { GlobalMemoryStatusEx(&mut st) };
    (ok != 0).then_some(st.ullAvailPhys / (1024 * 1024))
}

#[cfg(not(target_os = "windows"))]
pub fn free_memory_mb() -> Option<u64> {
    None
}

#[cfg(target_os = "windows")]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Caixa de mensagem nativa. `ask` = botões Sim/Não; devolve `true` se o usuário clicou Sim/OK.
#[cfg(target_os = "windows")]
pub fn message_box(title: &str, text: &str, ask: bool, error: bool) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDOK, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    };
    let flags = (if ask { MB_YESNO } else { MB_OK })
        | (if error { MB_ICONERROR } else { MB_ICONWARNING })
        | MB_TOPMOST
        | MB_SETFOREGROUND;
    let (t, m) = (wide(title), wide(text));
    // SAFETY: strings UTF-16 terminadas em zero que vivem até o fim da chamada
    let r = unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), flags) };
    r == IDYES || r == IDOK
}

#[cfg(not(target_os = "windows"))]
pub fn message_box(_title: &str, text: &str, _ask: bool, _error: bool) -> bool {
    eprintln!("{text}");
    false
}

#[cfg(target_os = "windows")]
fn open_in_browser(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (op, u) = (wide("open"), wide(url));
    // SAFETY: strings UTF-16 terminadas em zero; demais parâmetros nulos são permitidos
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), u.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

/// Windows: o WebView2 Runtime está instalado? Se não, avisa e devolve `false` (o app encerra).
pub fn check_webview() -> bool {
    match tauri::webview_version() {
        Ok(v) => {
            diag_log(&format!("WebView2 {v}"));
            true
        }
        Err(e) => {
            diag_log(&format!("WebView2 não encontrado: {e}"));
            #[cfg(target_os = "windows")]
            {
                let open = message_box(
                    "Claude Usage",
                    "Este computador não tem o Microsoft Edge WebView2 Runtime, que o app usa para \
                     desenhar a janela.\n\nEle é gratuito, da Microsoft, e já vem no Windows 11 e na \
                     maioria dos Windows 10 atualizados. Se a instalação for bloqueada, peça à TI.\n\n\
                     Abrir a página de download agora?",
                    true,
                    false,
                );
                if open {
                    open_in_browser(WEBVIEW2_URL);
                }
            }
            false
        }
    }
}

/// Erro antes da interface abrir: registra e, se a janela ainda não existe, avisa o usuário.
pub fn fatal(context: &str, detail: &str) {
    let mem = free_memory_mb();
    diag_log(&format!(
        "{context}: {detail}{}",
        mem.map(|m| format!(" (memória livre: {m} MB)")).unwrap_or_default()
    ));
    if UI_READY.load(Ordering::Relaxed) {
        return;
    }
    let low_mem = mem.is_some_and(|m| m < 600);
    let hint = if low_mem {
        "\n\nA memória do computador está quase cheia. Feche alguns programas (navegador, Teams) e abra o app de novo."
    } else {
        ""
    };
    message_box(
        "Claude Usage não conseguiu abrir",
        &format!("{context}.\n\n{detail}{hint}\n\nDetalhes em:\n{}", log_path()),
        false,
        true,
    );
}
