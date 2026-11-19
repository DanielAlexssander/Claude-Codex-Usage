#!/usr/bin/env bash
# Gera o .app / .dmg universal (Apple Silicon + Intel) do Claude + Codex Usage.
# Uso: ./build-macos.sh   (requer Xcode Command Line Tools, Node 20+ e Rust)
set -euo pipefail
cd "$(dirname "$0")"

command -v node >/dev/null || { echo "Instale o Node.js (brew install node)"; exit 1; }
command -v cargo >/dev/null || { echo "Instale o Rust: curl https://sh.rustup.rs -sSf | sh"; exit 1; }
xcode-select -p >/dev/null 2>&1 || xcode-select --install

rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm ci
npx tauri build --target universal-apple-darwin

mkdir -p release
cp -R src-tauri/target/universal-apple-darwin/release/bundle/macos/*.app release/
cp src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg release/
echo "Pronto! Arquivos em ./release"
