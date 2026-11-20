fn main() {
    // O ícone do .exe é embutido por este script. Sem isto o Cargo não percebe que os arquivos
    // de ícone mudaram e reaproveita o recurso antigo, mesmo recompilando o resto do app.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
