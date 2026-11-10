import { invoke, listen, openUrl } from "../lib/ipc";
import { accountInfo, componentStatus, elapsedFraction, extraUsage, incidentStatus, limitWindows, planLabel, projectSession, serviceIssue, weeklyPace } from "../lib/normalize";
import { ago, compact, duration, esc, int, level, levelIcon, levelLabel, money, pct, when, clock } from "../lib/format";
import { dailyBars, gauge, heatmap, sparkline, stackBar, tokenLegend } from "../lib/charts";
import { claudeMark } from "../lib/brand";
import { applyPrefs } from "../lib/prefs";
import type { LimitWindow, Settings, Snapshot, Totals } from "../lib/types";

const I = {
  refresh: `<svg viewBox="0 0 16 16"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  pin: `<svg viewBox="0 0 16 16"><path d="M6 2h4l-.6 4 2.6 2.4V10H4V8.4L6.6 6 6 2zM8 10v4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round"/></svg>`,
  gear: `<svg viewBox="0 0 24 24"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>`,
  close: `<svg viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`,
  back: `<svg viewBox="0 0 16 16"><path d="M10 3L5 8l5 5" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  bolt: `<svg viewBox="0 0 16 16"><path d="M9 1.5L3.5 9H8l-1 5.5L12.5 7H8z" fill="currentColor"/></svg>`,
  cal: `<svg viewBox="0 0 16 16"><rect x="2.5" y="3.5" width="11" height="10" rx="2" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M2.5 6.5h11M5.5 2v3M10.5 2v3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>`,
  status: `<svg viewBox="0 0 16 16"><path d="M1.5 8h3l1.5-4 3 8 1.5-4h4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  ext: `<svg viewBox="0 0 16 16"><path d="M9 3h4v4M13 3L7.5 8.5M11.5 9.5v3a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1h3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  plug: `<svg viewBox="0 0 24 24"><path d="M9 2v5M15 2v5M6 7h12v4a6 6 0 0 1-12 0zM12 17v5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
};

type Tab = "overview" | "activity" | "models" | "projects";
const TABS: { id: Tab; label: string }[] = [
  { id: "overview", label: "Visão geral" },
  { id: "activity", label: "Atividade" },
  { id: "models", label: "Modelos" },
  { id: "projects", label: "Projetos" },
];

export function mountPanel(root: HTMLElement) {
  root.innerHTML = `
  <div class="panel" id="panel">
    <header class="hd">
      <div class="brand">
        <div class="brand-claude">${claudeMark()}</div>
        <div class="brand-title"><b>Claude Usage</b><span>Uso do Claude Code</span></div>
      </div>
      <div class="hd-actions">
        <button class="ib" id="btn-refresh" title="Atualizar agora (Ctrl+R)">${I.refresh}</button>
        <button class="ib" id="btn-pin" title="Fixar painel (não esconder ao perder o foco)">${I.pin}</button>
        <button class="ib" id="btn-settings" title="Configurações">${I.gear}</button>
        <button class="ib" id="btn-close" title="Fechar (Esc)">${I.close}</button>
      </div>
    </header>
    <div class="acct" id="acct"></div>
    <div class="banner" id="banner" hidden></div>
    <div class="svc" id="svc" hidden></div>
    <nav class="tabs" role="tablist">
      <div class="tab-ind" id="tab-ind"></div>
      ${TABS.map((t, i) => `<button class="tab" role="tab" data-tab="${t.id}" aria-selected="${i === 0}">${t.label}</button>`).join("")}
    </nav>
    <main class="body" id="body">
      ${TABS.map((t, i) => `<section class="pane ${i === 0 ? "active" : ""}" data-pane="${t.id}"></section>`).join("")}
    </main>
    <footer class="ft">
      <span id="ft-status"><span class="ft-dot"></span>carregando…</span>
      <button class="link" id="lnk-web">claude.ai › Uso</button>
    </footer>
    <aside class="sheet" id="sheet" aria-hidden="true"></aside>
    <div class="tip" id="tip"></div>
  </div>`;

  const $ = <T extends HTMLElement = HTMLElement>(sel: string) => root.querySelector<T>(sel)!;
  const panel = $("#panel");
  const body = $("#body");
  const tip = $("#tip");
  let snap: Snapshot | null = null;
  let settings: Settings | null = null;

  // ------------------------------------------------------------ abas
  function selectTab(t: Tab) {
    const idx = TABS.findIndex((x) => x.id === t);
    root.querySelectorAll<HTMLElement>(".tab").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === t)));
    root.querySelectorAll<HTMLElement>(".pane").forEach((p) => p.classList.toggle("active", p.dataset.pane === t));
    $("#tab-ind").style.transform = `translateX(${idx * 100}%)`;
    body.scrollTop = 0;
  }

  root.querySelectorAll<HTMLElement>(".tab").forEach((b) => b.addEventListener("click", () => selectTab(b.dataset.tab as Tab)));

  // ------------------------------------------------------------ tooltip
  body.addEventListener("mouseover", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-tip]");
    if (!el) return tip.classList.remove("show");
    tip.innerHTML = el.dataset.tip!;
    const pr = panel.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    tip.classList.add("show");
    const tw = tip.offsetWidth, th = tip.offsetHeight;
    // centraliza no elemento, mas nunca deixa o balão sair do painel
    let x = r.left - pr.left + r.width / 2 - tw / 2;
    x = Math.max(8, Math.min(pr.width - tw - 8, x));
    let y = r.top - pr.top - th - 8; // preferencialmente acima
    if (y < 8) y = r.bottom - pr.top + 8; // sem espaço acima → abaixo
    y = Math.max(8, Math.min(pr.height - th - 8, y));
    tip.style.left = `${x}px`;
    tip.style.top = `${y}px`;
  });
  body.addEventListener("mouseleave", () => tip.classList.remove("show"));
  body.addEventListener("scroll", () => tip.classList.remove("show"), { passive: true });

  // ------------------------------------------------------------ ações
  const refresh = () => {
    $("#btn-refresh").classList.add("spin");
    invoke("refresh_now");
  };
  $("#btn-refresh").addEventListener("click", refresh);
  $("#btn-close").addEventListener("click", () => invoke("hide_panel"));
  $("#btn-settings").addEventListener("click", () => openSheet(true));
  $("#btn-pin").addEventListener("click", () => {
    const on = !$("#btn-pin").classList.contains("on");
    $("#btn-pin").classList.toggle("on", on);
    invoke("set_pinned", { pinned: on });
  });
  $("#lnk-web").addEventListener("click", () => openUrl("https://claude.ai/settings/usage"));
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if ($("#sheet").classList.contains("open")) openSheet(false);
      else invoke("hide_panel");
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r") {
      e.preventDefault();
      refresh();
    }
    if ((e.ctrlKey || e.metaKey) && /^[1-4]$/.test(e.key)) selectTab(TABS[Number(e.key) - 1].id);
  });
  body.addEventListener("click", (e) => {
    const a = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
    if (!a) return;
    if (a.dataset.act === "retry") refresh();
    if (a.dataset.act === "docs") openUrl("https://docs.claude.com/en/docs/claude-code/setup");
  });
  $("#banner").addEventListener("click", (e) => {
    const a = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
    if (!a) return;
    if (a.dataset.act === "renew") {
      a.textContent = "Abrindo o Claude Code…";
      invoke("renew_login").catch(() => {
        a.textContent = "Não abriu: rode `claude` num terminal";
      });
    }
    if (a.dataset.act === "retry") refresh();
    if (a.dataset.act === "docs") openUrl("https://docs.claude.com/en/docs/claude-code/setup");
  });

  $("#svc").addEventListener("click", (e) => {
    const a = (e.target as HTMLElement).closest<HTMLElement>("[data-url]");
    if (a) openUrl(a.dataset.url!);
  });

  // ------------------------------------------------------------ render
  function renderAccount(s: Snapshot) {
    const a = accountInfo(s);
    const p = planLabel(s);
    const name = a.name || a.email.split("@")[0] || "Conta Claude";
    const initials = name
      .split(/[\s._-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((x) => x[0]!.toUpperCase())
      .join("");
    const stale = s.stale && s.updatedAt ? `<span class="chip muted" title="Dados da última execução — atualizando">cache</span>` : "";
    $("#acct").innerHTML = `
      <div class="avatar">${esc(initials || "C")}</div>
      <div class="acct-who">
        <b title="${esc(name)}">${esc(name)}</b>
        ${a.email ? `<span class="acct-mail" title="${esc(a.email)}">${esc(a.email)}</span>` : `<span class="acct-mail">Login do Claude Code</span>`}
        <div class="acct-meta">
          ${a.org ? `<span class="acct-org" title="${esc(a.org)}">${esc(a.org)}</span>` : ""}
          <span class="chip" title="${esc(s.creds?.rateLimitTier ?? "")}">${I.bolt}${esc(p.plan)}${p.tier ? ` · ${esc(p.tier)}` : ""}</span>
          ${stale}
        </div>
      </div>`;
    root.querySelector(".brand-title span")!.textContent = `Uso do Claude Code · ${p.plan}`;
  }

  function renderBanner(s: Snapshot) {
    const b = $("#banner");
    const e = s.error;
    if (s.renewing) {
      b.className = "banner";
      b.hidden = false;
      b.innerHTML = `<span class="bi spin-ic">${I.refresh}</span><div><b>Renovando o login do Claude Code…</b>O Claude Code está sendo aberto em segundo plano, sem janela, para renovar o login. Leva alguns segundos.</div>`;
      return;
    }
    if (!e) {
      b.hidden = true;
      return;
    }
    const titles: Record<string, string> = {
      no_login: "Faça login no Claude Code",
      expired: "Token do Claude Code expirado",
      auth: "Sessão sem permissão",
      rate_limited: "Pausa temporária da API",
      network: "Sem conexão",
      http: "Erro ao consultar o uso",
    };
    const crit = e.kind === "no_login" || e.kind === "auth";
    let msg = esc(e.message).replace(/`([^`]+)`/g, "<code>$1</code>");
    if (e.kind === "expired" && s.renewFailed) {
      msg = `A renovação automática não conseguiu renovar o login (o Claude Code pode estar pedindo alguma confirmação). Clique em <strong>Renovar login</strong> para abrir o Claude Code numa janela; assim que ele iniciar, pode fechá-lo.`;
    }
    const actions =
      e.kind === "no_login"
        ? `<button class="btn" data-act="docs">Como instalar/logar</button><button class="btn primary" data-act="retry">Tentar de novo</button>`
        : e.kind === "expired" || e.kind === "auth"
          ? `<button class="btn primary" data-act="renew">Renovar login</button><button class="btn" data-act="retry">Tentar de novo</button>`
          : `<button class="btn" data-act="retry">Tentar de novo</button>`;
    b.className = `banner ${crit ? "crit" : ""}`;
    b.hidden = false;
    b.innerHTML = `<span class="bi">${levelIcon[crit ? "crit" : "high"]}</span><div><b>${titles[e.kind] ?? "Aviso"}</b>${msg}<div class="ba">${actions}</div></div>`;
  }

  function renderService(s: Snapshot) {
    const el = $("#svc");
    const issue = serviceIssue(s);
    const v = s.service;
    if (!issue || !v) {
      el.hidden = true;
      return;
    }
    const inc = v.incidents.slice(0, 1);
    const comps = v.components.slice(0, 3);
    const lines = inc.length
      ? inc
          .map(
            (i) =>
              `<button class="svc-inc" data-url="${esc(i.url || v.url)}" title="Abrir em status.claude.com"><span>${esc(i.name)}</span><em>${esc(incidentStatus(i.status))}</em></button>`,
          )
          .join("")
      : `<div class="svc-desc">${esc(v.description)}</div>`;
    const more = v.incidents.length > inc.length ? `<button class="svc-more" data-url="${esc(v.url)}">+${v.incidents.length - inc.length} ${v.incidents.length - inc.length === 1 ? "outro incidente" : "outros incidentes"}</button>` : "";
    el.className = `svc s-${issue.severity}`;
    el.hidden = false;
    el.innerHTML = `
      <span class="svc-ic">${I.status}</span>
      <div class="svc-main">
        <div class="svc-hd"><b>${issue.title}</b><button class="svc-link" data-url="${esc(v.url)}">status.claude.com ${I.ext}</button></div>
        ${lines}${more}
        ${comps.length ? `<div class="svc-comps">${comps.map((c) => `<span>${esc(c.name)}: ${esc(componentStatus(c.status))}</span>`).join("")}</div>` : ""}
        <div class="svc-foot">Pode não ser o seu computador · verificado ${ago(v.checkedAt)}</div>
      </div>`;
  }

  function limitRow(w: LimitWindow, i: number): string {
    const lv = level(w.utilization);
    // janelas semanais: ritmo por dia útil (fim de semana não conta); demais: ritmo linear
    const wp = weeklyPace(w);
    const ef = wp ? wp.fraction : elapsedFraction(w);
    const remaining = w.resetsAt ? w.resetsAt.getTime() - Date.now() : null;
    let pace = "";
    if (ef != null && ef > 0.03) {
      const expected = ef * 100;
      const what = wp ? "do ritmo por dia útil" : "do ritmo linear da janela";
      pace =
        w.utilization > expected + 5
          ? `<span class="pace-up" title="Consumo acima ${what}">▲ acima do ritmo</span>`
          : `<span class="pace-ok" title="Consumo dentro ${what}">● no ritmo</span>`;
    }
    const fracTip = ef != null ? `<br>${Math.round(ef * 100)}% do período ${wp ? "(em dias úteis) " : ""}já passou` : "";
    let budget = "";
    if (wp && w.key === "seven_day") {
      budget =
        wp.perDay != null
          ? `<div class="lim-pace${wp.over ? " over" : ""}" data-tip="${esc(`Divide o que sobra do limite (${pct(100 - w.utilization)}) igualmente<br>pelos dias úteis até o reset, contando hoje.<br>Sábado e domingo não entram na conta.`)}">
              ${I.cal}<span>Dá para usar <b>~${pct(wp.perDay)} por dia útil</b> · ${wp.daysLeft} ${wp.daysLeft === 1 ? "dia útil" : "dias úteis"} até reiniciar</span></div>`
          : `<div class="lim-pace">${I.cal}<span>Reinicia antes do próximo dia útil · <b>${pct(100 - w.utilization)}</b> livre</span></div>`;
    }
    return `
      <div class="lim" style="--i:${i}">
        <div class="lim-top"><span><i class="mini-ic" style="color:var(--${lv === "ok" ? "good" : lv === "warn" ? "warning" : lv === "high" ? "serious" : "critical"}-ink)">${levelIcon[lv]}</i>${esc(w.label)}</span><b>${pct(w.utilization)}</b></div>
        <div class="bar l-${lv}" data-tip="${esc(`<b>${w.label}</b><br>${pct(w.utilization)} usado · ${levelLabel[lv]}${fracTip}`)}">
          <i style="width:${Math.min(100, w.utilization)}%"></i>${ef != null ? `<u style="left:${ef * 100}%"></u>` : ""}
        </div>
        <div class="lim-sub"><span>${w.resetsAt ? `reinicia ${when(w.resetsAt)}${remaining != null ? ` · em ${duration(remaining)}` : ""}` : "sem reset informado"}</span>${pace}</div>
        ${budget}
        ${w.note ? `<div class="lim-note">${esc(w.note)}</div>` : ""}
      </div>`;
  }

  function totalsRow(label: string, t: Totals | undefined) {
    if (!t) return "";
    return `<tr><td>${label}</td><td>${compact(t.total)}</td><td>${int(t.messages)}</td><td>${int(t.sessions)}</td><td>${money(t.cost)}</td></tr>`;
  }

  function renderOverview(s: Snapshot): string {
    const wins = limitWindows(s.usage);
    const five = wins.find((w) => w.key === "five_hour");
    const others = wins.filter((w) => w !== five);
    const L = s.local;
    let html = "";
    let i = 0;

    if (five) {
      const lv = level(five.utilization);
      const ef = elapsedFraction(five);
      const proj = projectSession(s.history, five);
      const start = five.resetsAt ? five.resetsAt.getTime() - 5 * 3600_000 : Date.now() - 5 * 3600_000;
      const end = five.resetsAt ? five.resetsAt.getTime() : Date.now();
      let projTxt = "";
      if (proj) {
        const rate = `${proj.ratePerHour >= 0 ? "+" : ""}${proj.ratePerHour.toFixed(1).replace(".", ",")}%/h`;
        projTxt = proj.hitsAt
          ? `<div class="kv">Ritmo <b>${rate}</b> · <em>atinge 100% ${when(proj.hitsAt)}</em></div>`
          : `<div class="kv">Ritmo <b>${rate}</b> · <em>não deve atingir o limite</em></div>`;
      }
      html += `
        <div class="card hero" style="--i:${i++};--lvl:var(--${lv === "ok" ? "accent" : lv === "warn" ? "warning" : lv === "high" ? "serious" : "critical"})">
          ${gauge(five.utilization, lv, ef, "sessão 5h")}
          <div class="hero-info">
            <span class="status lvl-${lv}">${levelIcon[lv]}${levelLabel[lv]}</span>
            <div class="kv">Reinicia <b>${five.resetsAt ? `em ${duration(five.resetsAt.getTime() - Date.now())}` : "—"}</b> <em>${five.resetsAt ? when(five.resetsAt) : ""}</em></div>
            ${projTxt}
            ${L
              ? L.block.messages > 0
                ? `<div class="kv">Neste bloco <b>${compact(L.block.total)}</b> <em>tokens · ${int(L.block.messages)} req.</em></div>`
                : `<div class="kv" data-tip="${esc("O % da sessão inclui o uso na nuvem (Cowork, claude.ai).<br>Tokens e requisições só aparecem para sessões<br>do Claude Code que rodam neste computador.")}"><em>Sem uso local neste bloco · uso na nuvem</em></div>`
              : ""}
            <div class="spark">${sparkline(s.history, start, end)}</div>
          </div>
        </div>`;
    } else if (!s.usage) {
      html += `<div class="card" style="--i:${i++}"><div class="empty">${I.plug}Ainda sem dados de limite do plano.<br>Eles aparecem aqui assim que o login do Claude Code for encontrado e a API responder.</div></div>`;
    }

    if (others.length) {
      html += `<div class="card" style="--i:${i++}"><h3>Limites do plano <small>marcador = ritmo esperado</small></h3>${others.map(limitRow).join("")}</div>`;
    }

    const ex = extraUsage(s.usage);
    if (ex && (ex.enabled || ex.used)) {
      const u = ex.utilization ?? 0;
      html += `<div class="card" style="--i:${i++}">
        <h3>Uso extra <span class="chip ${ex.enabled ? "" : "muted"}">${ex.enabled ? "ativado" : "desativado"}</span></h3>
        <div class="lim"><div class="lim-top"><span>${ex.used != null ? money(ex.used) : "—"} usados</span><b>${ex.limit != null ? `de ${money(ex.limit)}` : "sem limite"}</b></div>
        <div class="bar l-${level(u)}"><i style="width:${Math.min(100, u)}%"></i></div>
        <div class="lim-sub"><span>Créditos pagos além do limite do plano (mês)</span><span>${ex.utilization != null ? pct(ex.utilization) : ""}</span></div></div>
      </div>`;
    }

    if (L) {
      const live = L.activeSessions > 0;
      html += `<div class="tiles">
        <div class="tile" style="--i:${i++}"><span>Tokens hoje</span><b>${compact(L.today.total)}</b><em>${compact(L.today.output)} de saída</em></div>
        <div class="tile" style="--i:${i++}"><span>Requisições hoje</span><b>${int(L.today.messages)}</b><em>${int(L.today.sessions)} sessões</em></div>
        <div class="tile" style="--i:${i++}"><span>${live ? `<i class="dot-live"></i>` : ""}Sessões ativas</span><b>${int(L.activeSessions)}</b><em>${L.lastActivity ? `última atividade ${ago(L.lastActivity * 1000)}` : "sem atividade"}</em></div>
        <div class="tile" style="--i:${i++}" title="Quanto custaria pela API — na assinatura o valor é fixo"><span>Custo eq. API · hoje</span><b>${money(L.today.cost)}</b><em>${money(L.week.cost)} em 7 dias</em></div>
      </div>`;
    }
    return html;
  }

  function renderActivity(s: Snapshot): string {
    const L = s.local;
    if (!L) return emptyLocal();
    const total14 = L.daily.reduce((a, d) => a + d.total, 0);
    return `
      <div class="card" style="--i:0"><h3>Últimos 14 dias <small>${compact(total14)} tokens</small></h3>${dailyBars(L.daily)}</div>
      <div class="card" style="--i:1"><h3>Quando você usa <small>requisições · 28 dias</small></h3>${heatmap(L.heatmap)}</div>
      <div class="card" style="--i:2"><h3>Resumo por período</h3>
        <table class="tbl"><thead><tr><th>Período</th><th>Tokens</th><th>Req.</th><th>Sessões</th><th>Eq. API</th></tr></thead><tbody>
          ${totalsRow(`Bloco 5h <span style="color:var(--text-3);font-weight:400">desde ${clock(new Date(L.blockStart * 1000))}</span>`, L.block)}
          ${totalsRow("Hoje", L.today)}
          ${totalsRow("7 dias", L.week)}
          ${totalsRow("30 dias", L.month)}
        </tbody></table>
      </div>
      <div class="tiles">
        <div class="tile" style="--i:3"><span>Ritmo (última hora)</span><b>${compact(L.burnRateTpm)}</b><em>tokens/min (sem cache lido)</em></div>
        <div class="tile" style="--i:4"><span>Custo eq. por hora</span><b>${money(L.costPerHour)}</b><em>última hora</em></div>
      </div>
      <p class="note">Tokens totais incluem leitura de cache, que é a maior parte do volume no Claude Code. Dados lidos localmente de ${int(L.files)} transcripts.</p>`;
  }

  function renderModels(s: Snapshot): string {
    const models = s.local?.models ?? [];
    // Modelos com limite próprio no plano (ex.: Fable) que não têm tokens registrados neste computador
    // — o uso foi na nuvem (Cowork / claude.ai). Mostra a linha com o % do limite em vez dos tokens.
    const has = (name: string) => models.some((m) => m.display.toLowerCase().startsWith(name.toLowerCase()));
    const cloudOnly = limitWindows(s.usage).filter((w) => w.scoped && !has(w.short));
    if (!models.length && !cloudOnly.length) {
      return s.local ? `<div class="card"><div class="empty">Nenhum uso nos últimos 7 dias.</div></div>` : emptyLocal();
    }
    const max = Math.max(1, ...models.map((m) => m.total));
    const localRows = models
      .map((m) => {
        const lim = limitWindows(s.usage).find((w) => w.scoped && m.display.toLowerCase().startsWith(w.short.toLowerCase()));
        return `<div class="mrow">
          <div class="mrow-top"><b>${esc(m.display)}</b><span>${m.priced === false ? "sem preço público" : `${money(m.cost)} eq. API`}</span></div>
          ${stackBar(m, max)}
          <div class="msub"><span>${compact(m.total)} tokens</span><span>${int(m.messages)} req.</span><span>${compact(m.output)} saída</span><span>${compact(m.input)} entrada</span>${lim ? `<span class="mlim">${pct(lim.utilization)} do limite semanal</span>` : ""}</div>
        </div>`;
      })
      .join("");
    const cloudRows = cloudOnly
      .map(
        (w) => `<div class="mrow">
          <div class="mrow-top"><b>${esc(w.short)}</b><span class="mlim">${pct(w.utilization)} do limite semanal</span></div>
          <div class="stack stack-cloud" data-tip="${esc(`<b>${esc(w.short)}</b><br>Uso feito na nuvem (Cowork / claude.ai):<br>os tokens não ficam registrados neste computador.`)}"><i style="width:${Math.max(2, Math.min(100, w.utilization))}%"></i></div>
          <div class="msub"><span>uso na nuvem · tokens não registrados neste computador</span>${w.note ? `<span>${esc(w.note)}</span>` : ""}</div>
        </div>`,
      )
      .join("");
    return `
      <div class="card" style="--i:0"><h3>Tokens por modelo <small>7 dias</small></h3>${models.length ? tokenLegend() : ""}${localRows}${cloudRows}</div>
      <p class="note">"Eq. API" é uma estimativa do custo pelos preços públicos da API (entrada, saída e cache). Na assinatura (Pro, Max, Team ou Enterprise) o valor é fixo; isto serve só para comparar o peso de cada modelo.</p>`;
  }

  function renderProjects(s: Snapshot): string {
    const L = s.local;
    if (!L) return emptyLocal();
    if (!L.projects.length) return `<div class="card"><div class="empty">Nenhum projeto com uso nos últimos 7 dias.</div></div>${sourcesNote(s)}`;
    const max = Math.max(...L.projects.map((p) => p.total));
    return `<div class="card" style="--i:0"><h3>Projetos <small>7 dias</small></h3>
      ${L.projects
        .map(
          (p) => `<div class="prow" data-tip="${esc(`<b>${esc(p.name)}</b><br>${compact(p.total)} tokens · ${int(p.messages)} req.<br>${money(p.cost)} eq. API`)}">
            <b>${esc(p.name)}</b><span class="pv">${compact(p.total)}</span>
            <div class="pbar"><i style="width:${(p.total / max) * 100}%"></i></div>
            <span class="ps">${int(p.messages)} req. · ${int(p.sessions)} ${p.sessions === 1 ? "sessão" : "sessões"} · ${money(p.cost)} · ${ago(p.lastTs * 1000)}</span>
          </div>`,
        )
        .join("")}</div>
      ${sourcesNote(s)}`;
  }

  function sourcesNote(s: Snapshot): string {
    const src = s.local?.sources;
    const desk = src?.desktopDirCount
      ? ` e do Claude Desktop (${int(src.desktopDirCount)} pasta${src.desktopDirCount === 1 ? "" : "s"})`
      : "";
    return `<p class="note">Fontes: Claude Code no terminal${desk}. O Cowork do Claude Desktop roda dentro de uma máquina virtual, e o chat do claude.ai e o Cowork na nuvem rodam nos servidores da Anthropic: nenhum deles deixa no computador um registro legível de tokens. Esse uso aparece só nos limites do plano (Visão geral).</p>`;
  }

  function emptyLocal() {
    return `<div class="card"><div class="empty">${I.plug}Nenhum log local do Claude Code encontrado ainda.<br>Os transcripts ficam em <b>~/.claude/projects</b> e aparecem aqui assim que você usar o Claude Code.</div></div>`;
  }

  function renderFooter() {
    const s = snap;
    if (!s) return;
    const el = $("#ft-status");
    const dotCls = s.error ? (s.usage ? "warn" : "err") : "";
    const next = s.nextRefreshAt ? Math.max(0, s.nextRefreshAt - Date.now()) : null;
    const parts = [`Atualizado ${ago(s.updatedAt)}`];
    if (s.refreshing) parts.push("atualizando…");
    else if (next != null) parts.push(`próxima em ${next < 60_000 ? `${Math.ceil(next / 1000)}s` : duration(next)}`);
    el.innerHTML = `<span class="ft-dot ${dotCls}"></span>${parts.join(" · ")}`;
  }

  function render(s: Snapshot) {
    snap = s;
    const scroll = body.scrollTop;
    renderAccount(s);
    renderBanner(s);
    renderService(s);
    const panes: Record<Tab, string> = {
      overview: renderOverview(s),
      activity: renderActivity(s),
      models: renderModels(s),
      projects: renderProjects(s),
    };
    root.querySelectorAll<HTMLElement>(".pane").forEach((p) => (p.innerHTML = panes[p.dataset.pane as Tab]));
    body.scrollTop = scroll;
    $("#btn-refresh").classList.toggle("spin", s.refreshing);
    renderFooter();
  }

  // ------------------------------------------------------------ configurações
  function openSheet(open: boolean) {
    const sh = $("#sheet");
    if (open) renderSheet();
    sh.classList.toggle("open", open);
    sh.setAttribute("aria-hidden", String(!open));
  }

  function seg<T extends string | number>(name: string, value: T, opts: [T, string][]) {
    return `<div class="seg" data-seg="${name}">${opts.map(([v, l]) => `<button data-v="${v}" class="${v === value ? "on" : ""}">${l}</button>`).join("")}</div>`;
  }
  /** "Control+Alt+U" → "Ctrl + Alt + U" (Windows) / "⌘⌥U" (macOS) */
  const keysLabel = (st: Settings) => {
    const keys = (st.shortcutKeys ?? (st.platform === "macos" ? "Command+Alt+U" : "Control+Alt+U")).split("+");
    if (st.platform === "macos") return keys.map((k) => ({ Command: "⌘", Alt: "⌥", Shift: "⇧", Control: "⌃" })[k] ?? k).join("");
    return keys.map((k) => (k === "Control" ? "Ctrl" : k)).join(" + ");
  };
  /** `inverted`: a chave mostra o contrário do valor salvo (ex.: "aparecer" = !hideOnShare) */
  const sw = (name: string, on: boolean, inverted = false) =>
    `<button class="sw" role="switch" data-sw="${name}" aria-checked="${inverted ? !on : on}"></button>`;

  function renderSheet() {
    const st = settings;
    if (!st) return;
    const s = snap;
    const L = s?.local;
    $("#sheet").innerHTML = `
      <div class="sheet-hd"><button class="ib" data-close>${I.back}</button><b>Configurações</b></div>
      <div class="sheet-body">
        <div class="card">
          <h3>Atualização</h3>
          <div class="row"><label>Consultar limites do plano a cada<small>Logs locais são lidos a cada 30 s</small></label>
            ${seg("refreshSecs", st.refreshSecs, [[60, "1 min"], [180, "3 min"], [300, "5 min"], [600, "10 min"]])}</div>
        </div>
        <div class="card">
          <h3>Aparência</h3>
          <div class="row"><label>Tema</label>${seg("theme", st.theme, [["system", "Sistema"], ["light", "Claro"], ["dark", "Escuro"]])}</div>
          <div class="row"><label>Reduzir animações<small>Economiza GPU/bateria</small></label>${sw("reduceMotion", st.reduceMotion)}</div>
        </div>
        <div class="card">
          <h3>Widget flutuante</h3>
          <div class="row"><label>Mostrar widget na tela</label>${sw("widgetVisible", st.widgetVisible)}</div>
          <div class="row"><label>Anel principal mostra</label>${seg("widgetMetric", st.widgetMetric, [["five_hour", "Sessão 5h"], ["seven_day", "Semanal"]])}</div>
          <div class="row"><label>Esconder em tela cheia${st.platform === "windows" ? "" : " (Windows)"}<small>Some enquanto outro app está em tela cheia ou apresentando (PowerPoint, vídeo) e volta depois</small></label>${sw("hideFullscreen", st.hideFullscreen)}</div>
          <div class="row"><label>Posição<small>Arraste o widget para mover</small></label><button class="btn" data-act-s="reset-pos">Voltar ao canto</button></div>
        </div>
        <div class="card">
          <h3>Transmissão de tela</h3>
          <div class="row"><label>Aparecer em transmissão de tela<small>${st.hideOnShare
            ? "Desligado: o widget e o painel não aparecem no Teams, em gravações nem em prints. Você continua vendo normalmente."
            : "Ligado: quem assiste à sua tela no Teams, em gravações ou prints vê o widget e o painel."}</small></label>${sw("hideOnShare", st.hideOnShare, true)}</div>
        </div>
        <div class="card">
          <h3>Atalho</h3>
          <div class="row"><label>Abrir o painel com <kbd>${esc(keysLabel(st))}</kbd><small>${st.shortcutFailed ? `<span class="warn-t">Outro app já usa essa combinação — o atalho não foi registrado.</span>` : "Funciona em qualquer app; aperte de novo para fechar"}</small></label>${sw("shortcut", st.shortcut)}</div>
        </div>
        <div class="card">
          <h3>Sistema</h3>
          <div class="row"><label>Iniciar com o ${st.platform === "macos" ? "macOS" : "Windows"}</label>${sw("autostart", st.autostart)}</div>
          <div class="row"><label>Notificações de limite<small>Avisa ao cruzar os limiares (no Fable, sugere trocar de modelo)</small></label>${sw("notifications", st.notifications)}</div>
          <div class="row${st.notifications ? "" : " off"}"><label>Avisos da sessão de 5h<small>"Faltam 15 min para reiniciar" e "sessão reiniciada" — quando a sessão passou do primeiro limiar</small></label>${sw("notifyReset", st.notifyReset)}</div>
          <div class="row"><label>Status do Claude<small>Consulta status.claude.com a cada 5 min e avisa no painel quando houver instabilidade</small></label>${sw("serviceStatus", st.serviceStatus)}</div>
          <div class="row"><label>Renovar login automaticamente<small>Quando o login do Claude Code expira, abre o Claude Code em segundo plano por alguns segundos para renová-lo</small></label>${sw("autoRenew", st.autoRenew)}</div>
          <div class="row"><label>Limiares</label>
            <div class="seg" data-multi="thresholds">${[50, 75, 90, 95].map((t) => `<button data-v="${t}" class="${st.thresholds.includes(t) ? "on" : ""}">${t}%</button>`).join("")}</div></div>
        </div>
        <div class="card about">
          <div class="about-logo">${claudeMark()}</div>
          <b>Claude Usage</b> v${esc(s?.appVersion ?? "")}<br>
          Claude Code detectado: <b>${esc(s?.claudeVersion ?? "—")}</b><br>
          Credenciais: ${esc(s?.creds?.source ?? "não encontradas")}<br>
          ${s?.creds?.expiresAt ? `Token expira: ${new Date(s.creds.expiresAt).toLocaleString("pt-BR")}<br>` : ""}
          Plano/tier: ${esc([s?.creds?.subscriptionType, s?.creds?.rateLimitTier].filter(Boolean).join(" / ") || "—")}<br>
          Logs (terminal): ${esc(L?.sources?.cliRoots.join(", ") || "—")}<br>
          Claude Desktop: ${L?.sources ? `${int(L.sources.desktopDirCount)} pasta(s) com sessões, ${int(L.sources.desktopFileCount)} arquivo(s) — procurado em ${esc(L.sources.desktopBases.filter((b) => b[1]).map((b) => b[0]).join(", ") || "nenhuma pasta do app encontrada")}` : "—"}<br>
          ${L ? `${int(L.files)} arquivos · ${int(L.records)} registros · varredura ${L.scanMs} ms<br>` : ""}
          <div style="margin-top:10px;display:flex;gap:6px"><button class="btn danger" data-act-s="quit">Sair do app</button></div>
        </div>
      </div>`;
  }

  async function saveSettings(patch: Partial<Settings>) {
    if (!settings) return;
    settings = { ...settings, ...patch };
    applyPrefs(settings);
    settings = await invoke<Settings>("update_settings", { settings });
    renderSheet();
  }

  $("#sheet").addEventListener("click", async (e) => {
    const t = e.target as HTMLElement;
    if (t.closest("[data-close]")) return openSheet(false);
    const segBtn = t.closest<HTMLElement>("[data-seg] button");
    if (segBtn) {
      const name = segBtn.parentElement!.dataset.seg!;
      const raw = segBtn.dataset.v!;
      const v = name === "refreshSecs" ? Number(raw) : raw;
      return saveSettings({ [name]: v } as Partial<Settings>);
    }
    const multi = t.closest<HTMLElement>("[data-multi] button");
    if (multi && settings) {
      const v = Number(multi.dataset.v);
      const set = new Set(settings.thresholds);
      set.has(v) ? set.delete(v) : set.add(v);
      return saveSettings({ thresholds: [...set].sort((a, b) => a - b) });
    }
    const s = t.closest<HTMLElement>("[data-sw]");
    if (s && settings) {
      const name = s.dataset.sw as keyof Settings;
      if (name === "autostart") {
        settings = await invoke<Settings>("set_autostart", { enabled: !settings.autostart });
        return renderSheet();
      }
      return saveSettings({ [name]: !settings[name] } as Partial<Settings>);
    }
    const act = t.closest<HTMLElement>("[data-act-s]")?.dataset.actS;
    if (act === "reset-pos") invoke("reset_widget_position");
    if (act === "quit") invoke("quit_app");
  });

  // ------------------------------------------------------------ eventos
  function playEnter() {
    panel.classList.remove("animate");
    void panel.offsetWidth;
    panel.classList.add("animate");
    setTimeout(() => panel.classList.remove("animate"), 1400);
  }

  invoke<Settings>("get_settings").then((st) => {
    settings = st;
    $("#btn-pin").classList.toggle("on", st.pinned);
  });
  invoke<Snapshot>("get_snapshot").then((s) => {
    render(s);
    playEnter();
  });
  listen<Snapshot>("snapshot", render);
  listen<boolean>("refreshing", (b) => {
    $("#btn-refresh").classList.toggle("spin", b);
    if (snap) {
      snap.refreshing = b;
      renderFooter();
    }
  });
  listen<Settings>("settings", (st) => {
    settings = st;
    applyPrefs(st);
    $("#btn-pin").classList.toggle("on", st.pinned);
    if ($("#sheet").classList.contains("open")) renderSheet();
  });
  listen("panel-shown", () => {
    playEnter();
    invoke<Snapshot>("get_snapshot").then(render);
  });

  // relógio do rodapé / contagens regressivas
  setInterval(() => {
    if (document.visibilityState === "visible") renderFooter();
  }, 1000);
  setInterval(() => {
    if (document.visibilityState === "visible" && snap) render(snap);
  }, 30_000);
}
