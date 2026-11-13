//! Estatísticas locais a partir dos transcripts do Claude Code
//! (`~/.claude/projects/**/*.jsonl`).
//!
//! * Leitura **incremental**: guarda o offset de cada arquivo e só lê o que foi
//!   anexado desde a última varredura (os arquivos são append-only).
//! * Deduplicação por `message.id` + `requestId` (o Claude Code grava uma linha
//!   por bloco de conteúdo com o mesmo `usage`).
//! * Mantém em memória apenas os últimos `RETENTION_DAYS` dias, em registros
//!   compactos (~64 bytes cada).

use chrono::{DateTime, Datelike, Duration as CDuration, Local, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

const RETENTION_DAYS: i64 = 35;
/// Lê no máximo isto por arquivo e por varredura (evita picos de memória).
const MAX_CHUNK: u64 = 64 * 1024 * 1024;

#[derive(Default)]
struct Interner {
    map: HashMap<String, u32>,
    names: Vec<String>,
}
impl Interner {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.map.get(s) {
            return i;
        }
        let i = self.names.len() as u32;
        self.names.push(s.to_string());
        self.map.insert(s.to_string(), i);
        i
    }
    fn name(&self, i: u32) -> &str {
        &self.names[i as usize]
    }
}

struct FileState {
    offset: u64,
    project: Option<u32>,
}

#[derive(Clone, Copy)]
struct Rec {
    ts: i64,
    model: u32,
    project: u32,
    session: u32,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}
impl Rec {
    fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Default)]
pub struct Scanner {
    files: HashMap<PathBuf, FileState>,
    records: HashMap<u64, Rec>,
    models: Interner,
    projects: Interner,
    sessions: Interner,
    roots: Vec<PathBuf>,
    /// pastas com transcripts encontradas dentro dos dados do Claude Desktop
    desktop_dirs: Vec<PathBuf>,
    last_discovery: Option<Instant>,
    sources: Sources,
}

/// Diagnóstico das fontes lidas (aparece em Configurações → Sobre e em `diagnostico.json`).
#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    /// pastas do Claude Desktop procuradas e se existem
    pub desktop_bases: Vec<(String, bool)>,
    /// pastas onde foram achados transcripts do Claude Code (até 12)
    pub desktop_dirs: Vec<String>,
    pub desktop_dir_count: usize,
    pub desktop_file_count: usize,
    /// entradas percorridas na última busca (para detectar pastas enormes)
    pub walked_entries: usize,
    pub discovery_ms: u64,
    pub cli_roots: Vec<String>,
    /// inventário das pastas do Claude Desktop (só nomes e contagens, sem conteúdo):
    /// (pasta, arquivos, .jsonl, bytes) até 3 níveis — para descobrir onde o app guarda as sessões
    pub desktop_inventory: Vec<(String, usize, usize, u64)>,
}

// ---------- formato das linhas JSONL ----------
#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    cwd: Option<String>,
    message: Option<Msg>,
}
#[derive(Deserialize)]
struct Msg {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

// ---------- saída serializada para o front-end ----------
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
    pub cost: f64,
    pub messages: u32,
    pub sessions: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub date: String,
    pub total: u64,
    pub output: u64,
    pub cost: f64,
    pub messages: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub model: String,
    pub display: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
    pub cost: f64,
    pub messages: u32,
    /// false quando não há preço público conhecido (custo não estimado)
    pub priced: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub name: String,
    pub total: u64,
    pub cost: f64,
    pub messages: u32,
    pub sessions: u32,
    pub last_ts: i64,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalStats {
    pub today: Totals,
    pub block: Totals,
    pub block_start: i64,
    pub week: Totals,
    pub month: Totals,
    /// últimos 14 dias (mais antigo → hoje)
    pub daily: Vec<DayPoint>,
    /// [dia da semana seg..dom][hora 0..23] → nº de requisições (últimos 28 dias)
    pub heatmap: Vec<Vec<u32>>,
    pub models: Vec<ModelRow>,
    pub projects: Vec<ProjectRow>,
    pub last_activity: Option<i64>,
    pub active_sessions: u32,
    /// tokens/min (sem cache read) na última hora
    pub burn_rate_tpm: f64,
    /// custo equivalente de API por hora, na última hora
    pub cost_per_hour: f64,
    pub files: usize,
    pub records: usize,
    pub scan_ms: u64,
    pub roots: Vec<String>,
    pub sources: Sources,
}

// ---------- preços (USD por milhão de tokens) — estimativa de custo equivalente em API ----------
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}
const fn pr(input: f64, output: f64, cache_read: f64) -> Price {
    Price { input, output, cache_write: input * 1.25, cache_read }
}

/// Preço público da API por família/versão. `None` = modelo sem preço conhecido (custo não estimado).
pub fn price(model: &str) -> Option<Price> {
    let m = model.to_ascii_lowercase();
    if m.contains("fable") || m.contains("mythos") {
        Some(pr(10.0, 50.0, 0.25))
    } else if m.contains("opus") {
        let legacy = ["opus-4-0", "opus-4-1", "opus-4-2025", "3-opus", "opus-3"];
        if legacy.iter().any(|l| m.contains(l)) { Some(pr(15.0, 75.0, 1.5)) } else { Some(pr(5.0, 25.0, 0.5)) }
    } else if m.contains("sonnet") {
        if m.contains("sonnet-5") { Some(pr(2.0, 10.0, 0.2)) } else { Some(pr(3.0, 15.0, 0.3)) }
    } else if m.contains("haiku") {
        if m.contains("3-5-haiku") || m.contains("3-haiku") { Some(pr(0.8, 4.0, 0.08)) } else { Some(pr(1.0, 5.0, 0.1)) }
    } else {
        None
    }
}

fn cost_of(r: &Rec, model: &str) -> f64 {
    let Some(p) = price(model) else { return 0.0 };
    (r.input as f64 * p.input
        + r.output as f64 * p.output
        + r.cache_write as f64 * p.cache_write
        + r.cache_read as f64 * p.cache_read)
        / 1_000_000.0
}

/// "claude-opus-4-5-20251101" → "Opus 4.5"; "claude-3-5-sonnet-20241022" → "Sonnet 3.5";
/// "claude-fable-5-1" → "Fable 5.1". Funciona para qualquer família nova do Claude.
pub fn display_model(model: &str) -> String {
    // remove sufixos como "[1m]" ou "@20250101"
    let base = model.split(['[', '@']).next().unwrap_or(model);
    let m = base.to_ascii_lowercase();
    let parts: Vec<&str> = m.split(['-', '_', '.']).filter(|p| !p.is_empty()).collect();
    if parts.first() != Some(&"claude") {
        return model.to_string();
    }
    let is_ver = |p: &&&str| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit());
    let is_word = |p: &&str| p.chars().any(|c| c.is_ascii_alphabetic()) && !p.chars().all(|c| c.is_ascii_digit());
    let Some(fi) = parts.iter().skip(1).position(is_word).map(|i| i + 1) else { return model.to_string() };
    let fam = parts[fi];
    let after: Vec<&str> = parts[fi + 1..].iter().take_while(is_ver).cloned().collect();
    let before: Vec<&str> = {
        let mut v: Vec<&str> = parts[1..fi].iter().rev().take_while(is_ver).cloned().collect();
        v.reverse();
        v
    };
    let ver = if !after.is_empty() { after.join(".") } else { before.join(".") };
    let mut name = fam.to_string();
    name[..1].make_ascii_uppercase();
    if ver.is_empty() { name } else { format!("{name} {ver}") }
}

fn hash_key(parts: &[&str]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    h.finish()
}

fn project_from_path(file: &Path) -> String {
    // .../projects/<pasta-codificada>/<sessao>.jsonl  → último segmento legível
    let enc = file
        .ancestors()
        .find(|a| a.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "projects"))
        .and_then(|a| a.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    enc.rsplit('-').find(|s| !s.is_empty()).unwrap_or("—").to_string()
}

fn basename(p: &str) -> String {
    p.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(p)
        .to_string()
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Transcripts do Claude Code na CLI: `~/.claude/projects` (ou `$CLAUDE_CONFIG_DIR/projects`).
    fn cli_roots() -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = crate::credentials::claude_config_dirs()
            .into_iter()
            .map(|d| d.join("projects"))
            .filter(|p| p.is_dir())
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    /// Pastas de dados do Claude Desktop onde procurar transcripts (Windows: `%APPDATA%\Claude`,
    /// `%LOCALAPPDATA%\Claude`, `Claude-3p`; macOS: `~/Library/Application Support/Claude`).
    fn desktop_bases() -> Vec<PathBuf> {
        let mut bases = vec![];
        for root in [dirs::config_dir(), dirs::data_local_dir(), dirs::data_dir()].into_iter().flatten() {
            for name in ["Claude", "Claude-3p", "Claude-Data"] {
                bases.push(root.join(name));
            }
        }
        // Windows (app da Microsoft Store / MSIX): o Claude Desktop grava em uma pasta virtualizada,
        // %LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\{Roaming,Local}\Claude — outros apps
        // não enxergam o %APPDATA%\Claude "de verdade".
        if let Some(local) = dirs::data_local_dir() {
            if let Ok(rd) = std::fs::read_dir(local.join("Packages")) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with("claude") || name.contains("anthropic") {
                        bases.push(e.path().join("LocalCache"));
                    }
                }
            }
        }
        bases.sort();
        bases.dedup();
        bases
    }

    /// Um .jsonl é transcript do Claude Code se as primeiras linhas têm `sessionId`.
    fn looks_like_transcript(path: &Path) -> bool {
        let mut buf = [0u8; 8192];
        let Ok(mut f) = std::fs::File::open(path) else { return false };
        let n = f.read(&mut buf).unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]);
        head.contains("\"sessionId\"") && (head.contains("\"type\"") || head.contains("\"message\""))
    }

    /// Procura transcripts dentro dos dados do Claude Desktop, sem depender de nomes de pasta
    /// (o app muda a estrutura entre versões). Pula caches do navegador e imagens de VM.
    fn discover_desktop(&mut self) {
        const SKIP: [&str; 22] = [
            "Cache", "Code Cache", "GPUCache", "DawnCache", "DawnGraphiteCache", "DawnWebGPUCache", "GrShaderCache",
            "ShaderCache", "IndexedDB", "blob_storage", "Service Worker", "Local Storage", "Session Storage",
            "Crashpad", "Shared Dictionary", "WebStorage", "node_modules", ".git", "vm_bundles", "Network",
            "component_crx_cache", "extensions_crx_cache",
        ];
        const MAX_ENTRIES: usize = 300_000;
        let t0 = Instant::now();
        let bases = Self::desktop_bases();
        let mut dirs_found: HashSet<PathBuf> = HashSet::new();
        let mut files_found = 0usize;
        let mut walked = 0usize;
        let mut base_info = vec![];
        for base in &bases {
            let exists = base.is_dir();
            base_info.push((base.display().to_string(), exists));
            if !exists {
                continue;
            }
            let walker = walkdir::WalkDir::new(base)
                .max_depth(16)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !(e.file_type().is_dir() && e.file_name().to_str().is_some_and(|n| SKIP.contains(&n))));
            for entry in walker.filter_map(|e| e.ok()) {
                walked += 1;
                if walked > MAX_ENTRIES {
                    break;
                }
                if !entry.file_type().is_file() || entry.path().extension().is_none_or(|e| e != "jsonl") {
                    continue;
                }
                if let Some(parent) = entry.path().parent() {
                    if dirs_found.contains(parent) || Self::looks_like_transcript(entry.path()) {
                        dirs_found.insert(parent.to_path_buf());
                        files_found += 1;
                    }
                }
            }
        }
        let mut dirs_vec: Vec<PathBuf> = dirs_found.into_iter().collect();
        dirs_vec.sort();
        self.sources.desktop_inventory = Self::desktop_inventory(&bases);
        self.sources.desktop_bases = base_info;
        self.sources.desktop_dir_count = dirs_vec.len();
        self.sources.desktop_file_count = files_found;
        self.sources.desktop_dirs = dirs_vec.iter().take(12).map(|d| d.display().to_string()).collect();
        self.sources.walked_entries = walked;
        self.sources.discovery_ms = t0.elapsed().as_millis() as u64;
        self.desktop_dirs = dirs_vec;
        self.last_discovery = Some(Instant::now());
    }

    /// Inventário raso (3 níveis, sem pular nada) das pastas do Claude Desktop: nome da pasta,
    /// quantos arquivos ela tem no total, quantos .jsonl e o tamanho somado. Serve para o
    /// diagnóstico: mostra onde ficam os dados sem ler nenhum conteúdo.
    fn desktop_inventory(bases: &[PathBuf]) -> Vec<(String, usize, usize, u64)> {
        const MAX_DIRS: usize = 160;
        const MAX_WALK: usize = 60_000;
        let mut out = vec![];
        for base in bases.iter().filter(|b| b.is_dir()) {
            let mut acc: std::collections::BTreeMap<PathBuf, (usize, usize, u64)> = Default::default();
            let mut walked = 0usize;
            for entry in walkdir::WalkDir::new(base).max_depth(8).follow_links(false).into_iter().filter_map(|e| e.ok()) {
                walked += 1;
                if walked > MAX_WALK {
                    break;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let jsonl = entry.path().extension().is_some_and(|e| e == "jsonl");
                // credita o arquivo à sua pasta e às ancestrais até o 3º nível abaixo da base
                let rel = entry.path().strip_prefix(base).unwrap_or(entry.path());
                let comps: Vec<_> = rel.components().collect();
                let depth = comps.len().saturating_sub(1).min(3);
                for d in 0..=depth {
                    let dir: PathBuf = comps[..d].iter().collect();
                    let e = acc.entry(dir).or_default();
                    e.0 += 1;
                    if jsonl {
                        e.1 += 1;
                    }
                    e.2 += size;
                }
            }
            let name = base.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            for (dir, (files, jsonl, bytes)) in acc.into_iter().take(MAX_DIRS) {
                let label = if dir.as_os_str().is_empty() { name.clone() } else { format!("{name}/{}", dir.display()) };
                out.push((label, files, jsonl, bytes));
            }
        }
        out
    }

    /// Varre os diretórios e incorpora apenas os bytes novos.
    pub fn scan(&mut self) {
        let cutoff = SystemTime::now() - std::time::Duration::from_secs((RETENTION_DAYS as u64) * 86_400);
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // Busca completa no Claude Desktop a cada 10 min; nos outros ciclos só relê as pastas já achadas.
        let desktop_enabled = std::env::var_os("CLAUDE_CONFIG_DIR").is_none();
        if desktop_enabled && self.last_discovery.is_none_or(|t| t.elapsed().as_secs() >= 600) {
            self.discover_desktop();
        }

        let cli = Self::cli_roots();
        self.sources.cli_roots = cli.iter().map(|p| p.display().to_string()).collect();
        let mut roots: Vec<(PathBuf, usize)> = cli.into_iter().map(|p| (p, 5)).collect();
        if desktop_enabled {
            roots.extend(self.desktop_dirs.iter().map(|d| (d.clone(), 2)));
        }
        self.roots = roots.iter().map(|(p, _)| p.clone()).collect();
        for (root, depth) in roots {
            for entry in walkdir::WalkDir::new(&root)
                .max_depth(depth)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if !entry.file_type().is_file() || path.extension().is_none_or(|e| e != "jsonl") {
                    continue;
                }
                if seen.contains(path) {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let len = meta.len();
                seen.insert(path.to_path_buf());

                let state = self.files.entry(path.to_path_buf()).or_insert_with(|| FileState { offset: 0, project: None });
                // Arquivo antigo que nunca lemos: ignora o histórico.
                if state.offset == 0 && meta.modified().map(|m| m < cutoff).unwrap_or(false) {
                    state.offset = len;
                    continue;
                }
                if len < state.offset {
                    state.offset = 0; // truncado / reescrito
                }
                if len == state.offset {
                    continue;
                }
                let start = state.offset;
                let project_hint = state.project;
                match read_chunk(path, start, len) {
                    Ok(buf) => {
                        let consumed = self.ingest(path, &buf, project_hint);
                        if let Some(st) = self.files.get_mut(path) {
                            st.offset = start + consumed.0 as u64;
                            if st.project.is_none() {
                                st.project = consumed.1;
                            }
                        }
                    }
                    Err(e) => log::warn!("falha lendo {}: {e}", path.display()),
                }
            }
        }

        self.files.retain(|p, _| seen.contains(p));
        let min_ts = Utc::now().timestamp() - RETENTION_DAYS * 86_400;
        self.records.retain(|_, r| r.ts >= min_ts);
    }

    /// Processa linhas completas; retorna (bytes consumidos, projeto detectado).
    pub(crate) fn ingest(&mut self, path: &Path, buf: &[u8], project_hint: Option<u32>) -> (usize, Option<u32>) {
        let consumed = match buf.iter().rposition(|&b| b == b'\n') {
            Some(i) => i + 1,
            None => return (0, project_hint),
        };
        let mut project = project_hint;
        let min_ts = Utc::now().timestamp() - RETENTION_DAYS * 86_400;

        for raw in buf[..consumed].split(|&b| b == b'\n') {
            if raw.len() < 20 {
                continue;
            }
            let Ok(text) = std::str::from_utf8(raw) else { continue };
            if project.is_none() && text.contains("\"cwd\"") {
                if let Ok(l) = serde_json::from_str::<Line>(text) {
                    if let Some(cwd) = l.cwd.as_deref() {
                        project = Some(self.projects.id(&basename(cwd)));
                    }
                }
            }
            if !text.contains("\"usage\"") || !text.contains("\"assistant\"") {
                continue;
            }
            let Ok(line) = serde_json::from_str::<Line>(text) else { continue };
            if line.kind.as_deref() != Some("assistant") {
                continue;
            }
            let Some(msg) = line.message else { continue };
            let Some(u) = msg.usage else { continue };
            let model = msg.model.unwrap_or_else(|| "desconhecido".into());
            if model == "<synthetic>" {
                continue;
            }
            let Some(ts) = line
                .timestamp
                .as_deref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|d| d.timestamp())
            else {
                continue;
            };
            if ts < min_ts {
                continue;
            }
            let rec_input = u.input_tokens.unwrap_or(0);
            let rec_output = u.output_tokens.unwrap_or(0);
            let rec_cr = u.cache_read_input_tokens.unwrap_or(0);
            let rec_cw = u.cache_creation_input_tokens.unwrap_or(0);
            if rec_input + rec_output + rec_cr + rec_cw == 0 {
                continue;
            }
            if project.is_none() {
                if let Some(cwd) = line.cwd.as_deref() {
                    project = Some(self.projects.id(&basename(cwd)));
                }
            }
            let proj = project.unwrap_or_else(|| {
                let name = project_from_path(path);
                self.projects.id(&name)
            });
            let session = line.session_id.as_deref().unwrap_or("?");
            let key = match (msg.id.as_deref(), line.request_id.as_deref()) {
                (Some(id), Some(rq)) => hash_key(&[id, rq]),
                (Some(id), None) => hash_key(&[session, id]),
                _ => hash_key(&[text]),
            };
            let rec = Rec {
                ts,
                model: self.models.id(&model),
                project: proj,
                session: self.sessions.id(session),
                input: rec_input,
                output: rec_output,
                cache_read: rec_cr,
                cache_write: rec_cw,
            };
            // a última linha (cumulativa) de uma mesma mensagem vence
            self.records.insert(key, rec);
        }
        (consumed, project)
    }

    /// Agrega os registros. `block_start`: início da janela de 5h atual (epoch s).
    pub fn compute(&self, block_start: Option<i64>) -> LocalStats {
        let t0 = Instant::now();
        let now = Local::now();
        let now_ts = now.timestamp();
        let today: NaiveDate = now.date_naive();
        let start_of_day = |d: NaiveDate| -> i64 {
            Local
                .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
                .earliest()
                .map(|x| x.timestamp())
                .unwrap_or(0)
        };
        let today_start = start_of_day(today);
        let week_start = now_ts - 7 * 86_400;
        let month_start = now_ts - 30 * 86_400;
        let heat_start = now_ts - 28 * 86_400;
        let block_start = block_start.filter(|b| *b <= now_ts && *b > now_ts - 5 * 3600 - 60).unwrap_or(now_ts - 5 * 3600);

        let mut out = LocalStats { block_start, ..Default::default() };
        let mut sess_today = HashSet::new();
        let mut sess_block = HashSet::new();
        let mut sess_week = HashSet::new();
        let mut sess_month = HashSet::new();
        let mut active = HashSet::new();

        let days: Vec<NaiveDate> = (0..14).rev().map(|i| today - CDuration::days(i)).collect();
        let mut daily: Vec<DayPoint> = days
            .iter()
            .map(|d| DayPoint { date: d.format("%Y-%m-%d").to_string(), total: 0, output: 0, cost: 0.0, messages: 0 })
            .collect();
        let first_day_start = start_of_day(days[0]);
        let mut heat = vec![vec![0u32; 24]; 7];
        let mut models: HashMap<u32, ModelRow> = HashMap::new();
        let mut projects: HashMap<u32, (ProjectRow, HashSet<u32>)> = HashMap::new();
        let mut last_hour_tokens = 0u64;
        let mut last_hour_cost = 0f64;

        fn add(t: &mut Totals, r: &Rec, c: f64) {
            t.input += r.input;
            t.output += r.output;
            t.cache_read += r.cache_read;
            t.cache_write += r.cache_write;
            t.total += r.total();
            t.cost += c;
            t.messages += 1;
        }

        for r in self.records.values() {
            let model_name = self.models.name(r.model);
            let c = cost_of(r, model_name);
            out.last_activity = Some(out.last_activity.map_or(r.ts, |l: i64| l.max(r.ts)));

            if r.ts >= month_start {
                add(&mut out.month, r, c);
                sess_month.insert(r.session);
            }
            if r.ts >= week_start {
                add(&mut out.week, r, c);
                sess_week.insert(r.session);
                let m = models.entry(r.model).or_insert_with(|| ModelRow {
                    model: model_name.to_string(),
                    display: display_model(model_name),
                    input: 0,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                    total: 0,
                    cost: 0.0,
                    messages: 0,
                    priced: price(model_name).is_some(),
                });
                m.input += r.input;
                m.output += r.output;
                m.cache_read += r.cache_read;
                m.cache_write += r.cache_write;
                m.total += r.total();
                m.cost += c;
                m.messages += 1;

                let p = projects.entry(r.project).or_insert_with(|| {
                    (
                        ProjectRow { name: self.projects.name(r.project).to_string(), total: 0, cost: 0.0, messages: 0, sessions: 0, last_ts: 0 },
                        HashSet::new(),
                    )
                });
                p.0.total += r.total();
                p.0.cost += c;
                p.0.messages += 1;
                p.0.last_ts = p.0.last_ts.max(r.ts);
                p.1.insert(r.session);
            }
            if r.ts >= today_start {
                add(&mut out.today, r, c);
                sess_today.insert(r.session);
            }
            if r.ts >= block_start {
                add(&mut out.block, r, c);
                sess_block.insert(r.session);
            }
            if r.ts >= now_ts - 3600 {
                last_hour_tokens += r.input + r.output + r.cache_write;
                last_hour_cost += c;
            }
            if r.ts >= now_ts - 15 * 60 {
                active.insert(r.session);
            }
            if r.ts >= first_day_start {
                if let Some(dt) = Local.timestamp_opt(r.ts, 0).single() {
                    let d = dt.date_naive();
                    let idx = (d - days[0]).num_days();
                    if (0..14).contains(&idx) {
                        let dp = &mut daily[idx as usize];
                        dp.total += r.total();
                        dp.output += r.output;
                        dp.cost += c;
                        dp.messages += 1;
                    }
                }
            }
            if r.ts >= heat_start {
                if let Some(dt) = Local.timestamp_opt(r.ts, 0).single() {
                    let wd = dt.weekday().num_days_from_monday() as usize;
                    heat[wd][dt.hour() as usize] += 1;
                }
            }
        }

        out.today.sessions = sess_today.len() as u32;
        out.block.sessions = sess_block.len() as u32;
        out.week.sessions = sess_week.len() as u32;
        out.month.sessions = sess_month.len() as u32;
        out.active_sessions = active.len() as u32;
        out.burn_rate_tpm = last_hour_tokens as f64 / 60.0;
        out.cost_per_hour = last_hour_cost;
        out.daily = daily;
        out.heatmap = heat;

        let mut mv: Vec<ModelRow> = models.into_values().collect();
        mv.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
        out.models = mv;

        let mut pv: Vec<ProjectRow> = projects
            .into_values()
            .map(|(mut p, s)| {
                p.sessions = s.len() as u32;
                p
            })
            .collect();
        pv.sort_by_key(|p| std::cmp::Reverse(p.total));
        pv.truncate(10);
        out.projects = pv;

        out.files = self.files.len();
        out.records = self.records.len();
        out.roots = self.roots.iter().map(|r| r.display().to_string()).collect();
        out.sources = self.sources.clone();
        out.scan_ms = t0.elapsed().as_millis() as u64;
        out
    }
}

fn read_chunk(path: &Path, start: u64, len: u64) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let to_read = (len - start).min(MAX_CHUNK);
    let mut buf = Vec::with_capacity(to_read as usize);
    f.take(to_read).read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn line(id: &str, rq: &str, ts: &str, model: &str, inp: u64, out: u64, cr: u64, cw: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","requestId":"{rq}","cwd":"C:\\Users\\dev\\meu-projeto","message":{{"id":"{id}","model":"{model}","role":"assistant","content":[{{"type":"text","text":"oi"}}],"usage":{{"input_tokens":{inp},"output_tokens":{out},"cache_read_input_tokens":{cr},"cache_creation_input_tokens":{cw}}}}}}}"#
        )
    }

    #[test]
    fn display_names() {
        assert_eq!(display_model("claude-opus-4-5-20251101"), "Opus 4.5");
        assert_eq!(display_model("claude-sonnet-4-20250514"), "Sonnet 4");
        assert_eq!(display_model("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(display_model("claude-haiku-4-5"), "Haiku 4.5");
        assert_eq!(display_model("gpt-x"), "gpt-x");
        assert_eq!(display_model("claude-fable-5-1"), "Fable 5.1");
        assert_eq!(display_model("claude-opus-4-6[1m]"), "Opus 4.6");
        assert_eq!(display_model("claude-mythos-5"), "Mythos 5");
    }

    #[test]
    fn prices() {
        let io = |m: &str| price(m).map(|p| (p.input, p.output));
        assert_eq!(io("claude-opus-4-1-20250805"), Some((15.0, 75.0)));
        assert_eq!(io("claude-opus-4-5-20251101"), Some((5.0, 25.0)));
        assert_eq!(io("claude-sonnet-4-5"), Some((3.0, 15.0)));
        assert_eq!(io("claude-sonnet-5"), Some((2.0, 10.0)));
        assert_eq!(io("claude-haiku-4-5"), Some((1.0, 5.0)));
        assert_eq!(io("claude-fable-5-1"), Some((10.0, 50.0)));
        assert_eq!(price("claude-fable-5-1").unwrap().cache_read, 0.25);
        assert_eq!(price("claude-novo-1"), None);
    }

    #[test]
    fn incremental_scan_dedup_and_aggregate() {
        let dir = std::env::temp_dir().join(format!("claude-usage-test-{}", std::process::id()));
        let proj = dir.join("projects").join("C--Users-dev-meu-projeto");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("sess.jsonl");
        let now = Utc::now();
        let ts = (now - CDuration::minutes(5)).to_rfc3339();
        let old = (now - CDuration::days(3)).to_rfc3339();
        let mut content = String::new();
        content.push_str(&line("m1", "r1", &ts, "claude-sonnet-4-5", 100, 50, 1000, 10));
        content.push('\n');
        // duplicata (mesmo id+request) — não deve contar 2x
        content.push_str(&line("m1", "r1", &ts, "claude-sonnet-4-5", 100, 50, 1000, 10));
        content.push('\n');
        content.push_str(&line("m2", "r2", &old, "claude-opus-4-5-20251101", 10, 20, 0, 0));
        content.push('\n');
        content.push_str(r#"{"type":"user","message":{"role":"user","content":"x"}}"#);
        content.push('\n');
        // linha incompleta (sem \n) deve ser ignorada até completar
        content.push_str(r#"{"type":"assistant","partial"#);
        std::fs::write(&file, &content).unwrap();

        std::env::set_var("CLAUDE_CONFIG_DIR", &dir);
        let mut s = Scanner::new();
        s.scan();
        assert_eq!(s.records.len(), 2);
        let st = s.compute(None);
        assert_eq!(st.week.messages, 2);
        assert_eq!(st.block.messages, 1);
        assert_eq!(st.block.total, 1160);
        assert_eq!(st.active_sessions, 1);
        assert_eq!(st.projects[0].name, "meu-projeto");
        assert_eq!(st.models.len(), 2);
        assert_eq!(st.daily.len(), 14);

        // anexa nova mensagem → leitura incremental
        let mut more = content.clone();
        more.truncate(content.rfind('\n').unwrap() + 1);
        more.push_str(&line("m3", "r3", &ts, "claude-haiku-4-5", 1, 2, 3, 4));
        more.push('\n');
        std::fs::write(&file, &more).unwrap();
        s.scan();
        assert_eq!(s.records.len(), 3);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
