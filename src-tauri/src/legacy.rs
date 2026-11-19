//! Migração do nome antigo do app ("Claude Usage" → "Claude + Codex Usage").
//!
//! O "iniciar com o sistema" é registrado com o nome do app. Depois da troca de nome, a entrada
//! antiga ficaria órfã: o sistema abriria o app duas vezes no login (a segunda instância mostra o
//! painel) ou, se só ela existisse, o app pareceria ter deixado de iniciar com o sistema.

/// Nome com que as versões anteriores registravam o "iniciar com o sistema".
const OLD_NAME: &str = "Claude Usage";

/// Remove o registro antigo de "iniciar com o sistema". Devolve `true` se ele existia, ou seja,
/// a opção estava ligada e o app deve se registrar de novo com o nome atual.
#[cfg(target_os = "windows")]
pub fn take_old_autostart() -> bool {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let value = wide(OLD_NAME);
    let delete = |key: &str| {
        let key = wide(key);
        // SAFETY: strings UTF-16 terminadas em zero que vivem até o fim da chamada.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) == 0 }
    };
    let existed = delete(r"Software\Microsoft\Windows\CurrentVersion\Run");
    // estado "ativado/desativado" que o Windows guarda à parte; sem a entrada acima, sobraria órfão
    delete(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
    existed
}

#[cfg(target_os = "macos")]
pub fn take_old_autostart() -> bool {
    dirs::home_dir()
        .map(|h| h.join("Library/LaunchAgents").join(format!("{OLD_NAME}.plist")))
        .is_some_and(|plist| std::fs::remove_file(plist).is_ok())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn take_old_autostart() -> bool {
    false
}
