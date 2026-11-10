// Logos são opcionais: coloque os arquivos em `src/assets/brand/` e
// eles entram automaticamente no build. Nomes reconhecidos:
//   claude-logo.(svg|png|webp)     claude-logo-dark.(svg|png|webp)   (versão para tema escuro)
// Sem os arquivos, o app usa uma marca tipográfica neutra.

const files = import.meta.glob("../assets/brand/*.{svg,png,webp}", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

function find(base: string): string | null {
  for (const [path, url] of Object.entries(files)) {
    const name = path.split("/").pop()!.replace(/\.(svg|png|webp)$/i, "").toLowerCase();
    if (name === base) return url;
  }
  return null;
}

function mark(base: string, alt: string, fallback: string): string {
  const light = find(base);
  const dark = find(`${base}-dark`);
  if (!light && !dark) return fallback;
  const l = light ?? dark!;
  const d = dark ?? light!;
  return `<img class="logo logo-light" src="${l}" alt="${alt}" draggable="false"/><img class="logo logo-dark" src="${d}" alt="${alt}" draggable="false"/>`;
}

export const claudeMark = () =>
  mark("claude-logo", "Claude", `<span class="wordmark wm-claude" aria-label="Claude">Claude</span>`);

export const hasOfficialLogos = () => !!find("claude-logo");
