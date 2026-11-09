// Painel da telemetria. Login pelo Supabase Auth (link mágico, sem senha e sem cadastro aberto) e números
// agregados de /api/telemetry/report. Quem decide se a pessoa é administradora é o banco (telemetry_report).
// Nenhum dado vindo do servidor entra como HTML: tudo vira texto com textContent.

const $ = (id) => document.getElementById(id);
const SESSION_KEY = "prancha.admin.session";
const SVG = "http://www.w3.org/2000/svg";
const nf = new Intl.NumberFormat("pt-BR");
const pf = new Intl.NumberFormat("pt-BR", { style: "percent", maximumFractionDigits: 1 });
const df = new Intl.DateTimeFormat("pt-BR", { day: "2-digit", month: "2-digit", timeZone: "UTC" });

const CATEGORY_LABELS = {
  network: "Rede", source: "Fonte", extension: "Extensão", reader: "Leitor", library: "Biblioteca", import: "Importação",
  database: "Banco", update: "Atualização", interface: "Interface", crash: "Saída inesperada", other: "Outra",
};
const METRIC_LABELS = {
  startup: "Abertura do app", library_load: "Biblioteca", explore_results: "Resultados do Explorar",
  series_open: "Abrir obra", chapter_open: "Abrir capítulo", reader_first_page: "Primeira página",
};
const PLATFORM_LABELS = { windows: "Windows", macos: "macOS", linux: "Linux" };
const CHANNEL_LABELS = { nsis: "Instalador EXE", msi: "Instalador MSI", dev: "Desenvolvimento", unknown: "Desconhecido" };

let config = null;
let session = null;
let report = null;
let tab = "overview";
let loading = false;

// ---------------------------------------------------------------------------------------------------------------
// Utilidades
// ---------------------------------------------------------------------------------------------------------------

function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    if (k === "class") el.className = v;
    else if (k === "color") el.style.background = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children.flat()) if (c !== null && c !== undefined && c !== false) el.append(c instanceof Node ? c : String(c));
  return el;
}

function s(tag, attrs = {}) {
  const el = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  return el;
}

function show(id) {
  for (const section of ["booting", "login", "fatal", "dash"]) $(section).hidden = section !== id;
}

function fatal(title, text) {
  $("fatal-title").textContent = title;
  $("fatal-text").textContent = text;
  show("fatal");
}

const iso = (d) => d.toISOString().slice(0, 10);
const num = (v) => Number(v ?? 0);
const ratio = (a, b) => (b > 0 ? a / b : null);
const fmtPct = (v) => (v === null ? "—" : pf.format(v));
const day = (value) => df.format(new Date(`${value}T00:00:00Z`));
const compareVersions = (a, b) => {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) if (pa[i] !== pb[i]) return pa[i] - pb[i];
  return 0;
};

// Dica ao passar o mouse ou focar uma marca.
const tooltip = $("tooltip");
function attachTip(el, lines) {
  el.setAttribute("tabindex", "0");
  el.setAttribute("aria-label", lines.join(", "));
  const move = (e) => {
    tooltip.replaceChildren(...lines.map((line, i) => (i === 0 ? h("div", {}, h("b", {}, line)) : h("div", {}, line))));
    tooltip.hidden = false;
    const rect = e.clientX !== undefined ? { x: e.clientX, y: e.clientY } : (() => { const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top }; })();
    const w = tooltip.offsetWidth;
    tooltip.style.left = `${Math.min(window.innerWidth - w - 8, Math.max(8, rect.x - w / 2))}px`;
    tooltip.style.top = `${Math.max(8, rect.y - tooltip.offsetHeight - 12)}px`;
  };
  el.addEventListener("mousemove", move);
  el.addEventListener("focus", move);
  el.addEventListener("mouseleave", () => (tooltip.hidden = true));
  el.addEventListener("blur", () => (tooltip.hidden = true));
}

// ---------------------------------------------------------------------------------------------------------------
// Gráficos (SVG simples: barras horizontais, colunas por dia e colunas empilhadas)
// ---------------------------------------------------------------------------------------------------------------

/** Barras horizontais em HTML (texto nítido em qualquer largura). Uma série: sem legenda, o título diz o que é. */
function hbars(rows, { label, value, format = nf.format, tip, color = "var(--s1)" }) {
  const shown = rows.slice(0, 12);
  const max = Math.max(1, ...shown.map((r) => num(value(r))));
  return h(
    "div",
    { class: "hbars", role: "list" },
    shown.map((r) => {
      const v = num(value(r));
      const text = String(label(r));
      const bar = h("span", { class: "hbar-fill", color });
      bar.style.width = `${Math.max(v > 0 ? 1 : 0, (100 * v) / max)}%`;
      const row = h("div", { class: "hbar", role: "listitem" }, h("span", { class: "hbar-label", title: text }, text), h("span", { class: "hbar-track" }, bar), h("span", { class: "hbar-value" }, format(v)));
      attachTip(row, tip ? tip(r) : [text, format(v)]);
      return row;
    }),
  );
}

/** Colunas por dia. Os rótulos do eixo saem espaçados para não colidir. */
function columns(rows, { value, format = nf.format, tip, color = "var(--s1)", height = 130 }) {
  const width = 420;
  const pad = { l: 36, r: 8, t: 8, b: 22 };
  const max = Math.max(1, ...rows.map((r) => num(value(r))));
  const svg = s("svg", { class: "chart", viewBox: `0 0 ${width} ${height}`, role: "img", "aria-label": "Gráfico por dia" });
  const plotW = width - pad.l - pad.r;
  const plotH = height - pad.t - pad.b;
  for (const f of [0, 0.5, 1]) {
    const y = pad.t + plotH * (1 - f);
    svg.append(s("line", { class: "gridline", x1: pad.l, x2: width - pad.r, y1: y, y2: y }));
    const t = s("text", { x: pad.l - 6, y: y + 4, "text-anchor": "end" });
    t.textContent = format(max >= 10 ? Math.round(max * f) : max * f);
    svg.append(t);
  }
  const step = plotW / Math.max(1, rows.length);
  const barW = Math.max(2, step - 2);
  const every = Math.ceil(rows.length / 6);
  rows.forEach((r, i) => {
    const v = num(value(r));
    const hgt = (plotH * v) / max;
    const x = pad.l + i * step + 1;
    svg.append(s("rect", { class: "bar", x, y: pad.t + plotH - hgt, width: barW, height: Math.max(0, hgt), rx: Math.min(3, barW / 2), fill: color }));
    if (i % every === 0) {
      const t = s("text", { x: x + barW / 2, y: height - 6, "text-anchor": "middle" });
      t.textContent = day(r.day);
      svg.append(t);
    }
    const hit = s("rect", { class: "hit", x: pad.l + i * step, y: pad.t, width: step, height: plotH });
    svg.append(hit);
    attachTip(hit, tip ? tip(r) : [day(r.day), format(v)]);
  });
  return svg;
}

/** Adoção de versões por dia: até 5 versões com cor própria e o resto em "Outras". A cor segue a versão. */
function adoptionChart(adoption) {
  const totals = new Map();
  for (const a of adoption) totals.set(a.version, (totals.get(a.version) ?? 0) + num(a.installs));
  const top = [...totals.keys()].sort(compareVersions).reverse().slice(0, 5);
  const colors = ["var(--s1)", "var(--s2)", "var(--s3)", "var(--s4)", "var(--s5)"];
  const colorOf = (v) => colors[top.indexOf(v)] ?? "var(--other)";
  const byDay = new Map();
  for (const a of adoption) {
    const key = top.includes(a.version) ? a.version : "Outras";
    const d = byDay.get(a.day) ?? {};
    d[key] = (d[key] ?? 0) + num(a.installs);
    byDay.set(a.day, d);
  }
  const days = [...byDay.keys()].sort();
  const series = [...top, ...(totals.size > top.length ? ["Outras"] : [])];
  const width = 420;
  const height = 150;
  const pad = { l: 36, r: 8, t: 8, b: 22 };
  const plotW = width - pad.l - pad.r;
  const plotH = height - pad.t - pad.b;
  const max = Math.max(1, ...days.map((d) => Object.values(byDay.get(d)).reduce((a, b) => a + b, 0)));
  const svg = s("svg", { class: "chart", viewBox: `0 0 ${width} ${height}`, role: "img", "aria-label": "Instalações por versão ao longo do tempo" });
  for (const f of [0, 0.5, 1]) {
    const y = pad.t + plotH * (1 - f);
    svg.append(s("line", { class: "gridline", x1: pad.l, x2: width - pad.r, y1: y, y2: y }));
    const t = s("text", { x: pad.l - 6, y: y + 4, "text-anchor": "end" });
    t.textContent = nf.format(Math.round(max * f));
    svg.append(t);
  }
  const step = plotW / Math.max(1, days.length);
  const barW = Math.max(2, step - 2);
  const every = Math.ceil(days.length / 6);
  days.forEach((d, i) => {
    const counts = byDay.get(d);
    let y = pad.t + plotH;
    const x = pad.l + i * step + 1;
    for (const v of series) {
      const c = counts[v] ?? 0;
      if (!c) continue;
      const hgt = (plotH * c) / max;
      // 1 px de superfície entre os segmentos.
      svg.append(s("rect", { x, y: y - hgt + 1, width: barW, height: Math.max(0, hgt - 1), fill: v === "Outras" ? "var(--other)" : colorOf(v) }));
      y -= hgt;
    }
    if (i % every === 0) {
      const t = s("text", { x: x + barW / 2, y: height - 6, "text-anchor": "middle" });
      t.textContent = day(d);
      svg.append(t);
    }
    const hit = s("rect", { class: "hit", x: pad.l + i * step, y: pad.t, width: step, height: plotH });
    svg.append(hit);
    attachTip(hit, [day(d), ...series.filter((v) => counts[v]).map((v) => `${v}: ${nf.format(counts[v])}`)]);
  });
  const legend = h("div", { class: "legend" }, series.map((v) => h("span", {}, h("i", { color: v === "Outras" ? "var(--other)" : colorOf(v) }), v)));
  return h("div", {}, svg, legend);
}

function table(headers, rows) {
  return h(
    "div",
    { class: "table-wrap" },
    h(
      "table",
      {},
      h("thead", {}, h("tr", {}, headers.map(([text, isNum]) => h("th", { class: isNum ? "num" : null, scope: "col" }, text)))),
      h("tbody", {}, rows.map((cells) => h("tr", {}, cells.map((c, i) => h("td", { class: headers[i][1] ? "num" : null }, c))))),
    ),
  );
}

/** Todo gráfico tem a mesma informação em tabela, para leitor de tela e para copiar. */
function withTable(chart, headers, rows) {
  return h("div", {}, chart, h("details", { class: "table-view" }, h("summary", {}, "Ver como tabela"), table(headers, rows)));
}

function panel(title, desc, ...content) {
  return h("section", { class: "panel" }, h("h2", {}, title), desc ? h("p", { class: "desc" }, desc) : null, ...content);
}

function tile(label, value, hint) {
  return h("div", { class: "tile" }, h("div", { class: "label" }, label), h("div", { class: "value" }, value), hint ? h("div", { class: "hint" }, hint) : null);
}

function empty(text) {
  return h("div", { class: "panel empty" }, h("h2", {}, "Ainda não há dados neste período"), h("p", {}, text));
}

// ---------------------------------------------------------------------------------------------------------------
// Seções
// ---------------------------------------------------------------------------------------------------------------

function totalsOf(r) {
  const t = r.totals;
  const launches = num(t.launches);
  return { installs: num(t.installs), launches, unclean: num(t.unclean_exits), errors: num(t.errors), events: num(t.events), batches: num(t.batches), withErrors: num(t.installs_with_errors) };
}

const NO_DATA =
  "Os números aparecem depois que pessoas usando uma versão do Prancha com telemetria aceitarem enviar estatísticas. Nada aqui é estimado: sem adesão, o painel fica vazio.";

function overview(r) {
  const t = totalsOf(r);
  if (t.installs === 0 && t.events === 0) return [empty(NO_DATA)];
  const latest = [...r.versions].sort((a, b) => compareVersions(b.version, a.version))[0];
  return [
    h(
      "div",
      { class: "tiles" },
      tile("Instalações ativas com telemetria", nf.format(t.installs), "Só quem aceitou. Não é o total de usuários do Prancha."),
      tile("Aberturas do app", nf.format(t.launches)),
      tile("Falhas registradas", nf.format(t.errors), `${nf.format(t.withErrors)} instalação(ões) com alguma falha`),
      tile("Falhas por abertura", fmtPct(ratio(t.errors, t.launches)), "Falhas ÷ aberturas no período"),
      tile("Saídas inesperadas", fmtPct(ratio(t.unclean, t.launches)), `${nf.format(t.unclean)} de ${nf.format(t.launches)} aberturas`),
      tile("Eventos recebidos", nf.format(t.events), `${nf.format(t.batches)} lote(s)`),
    ),
    h(
      "div",
      { class: "grid2" },
      panel(
        "Instalações ativas por dia",
        "Instalações com telemetria autorizada que abriram o app em cada dia (UTC).",
        withTable(columns(r.daily, { value: (d) => d.installs, tip: (d) => [day(d.day), `${nf.format(d.installs)} instalação(ões)`, `${nf.format(d.launches)} abertura(s)`] }), [["Dia"], ["Instalações", true], ["Aberturas", true]], r.daily.map((d) => [day(d.day), nf.format(d.installs), nf.format(d.launches)])),
      ),
      panel(
        "Estabilidade: falhas a cada 100 aberturas",
        "Quanto menor, melhor. Dias sem aberturas ficam em zero.",
        withTable(
          columns(r.daily, { value: (d) => (num(d.launches) ? (100 * num(d.errors)) / num(d.launches) : 0), format: (v) => nf.format(Math.round(v * 10) / 10), color: "var(--s2)", tip: (d) => [day(d.day), `${nf.format(d.errors)} falha(s)`, `${nf.format(d.launches)} abertura(s)`, `${nf.format(d.unclean_exits)} saída(s) inesperada(s)`] }),
          [["Dia"], ["Falhas", true], ["Aberturas", true], ["Saídas inesperadas", true]],
          r.daily.map((d) => [day(d.day), nf.format(d.errors), nf.format(d.launches), nf.format(d.unclean_exits)]),
        ),
      ),
      panel("Versões em uso", latest ? `Versão mais nova vista: ${latest.version}.` : null, hbars(r.versions, { label: (v) => v.version, value: (v) => v.installs, tip: (v) => [v.version, `${nf.format(v.installs)} instalação(ões)`, fmtPct(ratio(num(v.installs), t.installs))] })),
      panel("Plataformas", null, hbars(r.platforms, { label: (p) => PLATFORM_LABELS[p.platform] ?? p.platform, value: (p) => p.installs })),
      panel("Fontes mais usadas", "Consultas + obras abertas, por identificador técnico da fonte.", r.sources.length ? hbars(r.sources, { label: (x) => x.source, value: (x) => num(x.queries) + num(x.opens), tip: (x) => [x.source, `${nf.format(x.queries)} consulta(s)`, `${nf.format(x.opens)} obra(s) aberta(s)`] }) : h("p", { class: "muted" }, "Nenhuma fonte usada no período.")),
      panel("Tempos médios", "Média e pior caso no período.", r.performance.length ? table([["Medida"], ["Amostras", true], ["Média", true], ["Máximo", true]], r.performance.map((p) => [METRIC_LABELS[p.metric] ?? p.metric, nf.format(p.samples), `${nf.format(p.avg_ms)} ms`, `${nf.format(p.max_ms)} ms`])) : h("p", { class: "muted" }, "Sem medições no período.")),
    ),
  ];
}

function versions(r) {
  const t = totalsOf(r);
  if (!r.versions.length) return [empty(NO_DATA)];
  const sorted = [...r.versions].sort((a, b) => compareVersions(b.version, a.version));
  const newest = sorted[0].version.split(".").map(Number);
  // Antiga: atrás da versão mais nova vista em minor ou major.
  const isOld = (v) => {
    const p = v.split(".").map(Number);
    return p[0] < newest[0] || (p[0] === newest[0] && p[1] < newest[1]);
  };
  return [
    h("div", { class: "tiles" }, tile("Versões detectadas", nf.format(r.versions.length)), tile("Na versão mais nova", fmtPct(ratio(num(sorted[0].installs), t.installs)), sorted[0].version), tile("Em versões antigas", fmtPct(ratio(sorted.filter((v) => isOld(v.version)).reduce((a, v) => a + num(v.installs), 0), t.installs)), "Atrás da minor mais nova")),
    panel(
      "Instalações por versão",
      "Cada instalação conta na versão do último dia em que apareceu no período.",
      table([["Versão"], ["Instalações", true], ["Percentual", true]], sorted.map((v) => [h("span", {}, h("span", { class: "mono" }, v.version), isOld(v.version) ? h("span", { class: "badge old" }, "antiga") : null), nf.format(v.installs), fmtPct(ratio(num(v.installs), t.installs))])),
    ),
    panel("Adoção ao longo do tempo", "Instalações ativas por dia, separadas pela versão daquele dia.", r.adoption.length ? adoptionChart(r.adoption) : h("p", { class: "muted" }, "Sem dados no período.")),
    panel("Canais de distribuição", null, hbars(r.channels, { label: (c) => CHANNEL_LABELS[c.channel] ?? c.channel, value: (c) => c.installs })),
  ];
}

function sources(r) {
  if (!r.sources.length) return [empty("Nenhuma instalação com telemetria usou fontes de leitura no período.")];
  const rate = (x) => ratio(num(x.errors), num(x.queries) + num(x.opens) + num(x.chapters));
  return [
    panel(
      "Ranking das fontes",
      "Por identificador técnico (extensão/fonte). Taxa de erro = erros ÷ (consultas + obras + capítulos). Nenhum título ou busca é coletado.",
      table(
        [["Fonte"], ["Consultas", true], ["Obras abertas", true], ["Capítulos", true], ["Instalações", true], ["Dias com uso", true], ["Erros", true], ["Taxa de erro", true]],
        r.sources.map((x) => [h("span", { class: "mono" }, x.source), nf.format(x.queries), nf.format(x.opens), nf.format(x.chapters), nf.format(x.installs), nf.format(x.active_days), nf.format(x.errors), fmtPct(rate(x))]),
      ),
    ),
    h(
      "div",
      { class: "grid2" },
      panel("Consultas por fonte", null, hbars(r.sources, { label: (x) => x.source, value: (x) => x.queries })),
      panel("Taxa de erro por fonte", "Só fontes com 20 usos ou mais, para a taxa não enganar.", (() => {
        const rows = r.sources.filter((x) => num(x.queries) + num(x.opens) + num(x.chapters) >= 20).sort((a, b) => (rate(b) ?? 0) - (rate(a) ?? 0));
        return rows.length ? hbars(rows, { label: (x) => x.source, value: (x) => rate(x) ?? 0, format: (v) => pf.format(v), color: "var(--s2)" }) : h("p", { class: "muted" }, "Nenhuma fonte com uso suficiente.");
      })()),
    ),
  ];
}

function failures(r) {
  const t = totalsOf(r);
  if (t.errors === 0) return [empty("Nenhuma falha registrada neste período e com estes filtros.")];
  return [
    h("div", { class: "tiles" }, tile("Falhas", nf.format(t.errors)), tile("Falhas por abertura", fmtPct(ratio(t.errors, t.launches))), tile("Instalações afetadas", nf.format(t.withErrors), fmtPct(ratio(t.withErrors, t.installs))), tile("Saídas inesperadas", nf.format(t.unclean))),
    panel("Falhas por dia", null, columns(r.daily, { value: (d) => d.errors, color: "var(--s2)" })),
    h(
      "div",
      { class: "grid2" },
      panel("Por versão", null, hbars(r.errorsByVersion, { label: (x) => x.version, value: (x) => x.errors, color: "var(--s2)" })),
      panel("Por plataforma", null, hbars(r.errorsByPlatform, { label: (x) => PLATFORM_LABELS[x.platform] ?? x.platform, value: (x) => x.errors, color: "var(--s2)" })),
      panel("Por componente", null, hbars(r.errorsByComponent, { label: (x) => x.component, value: (x) => x.errors, color: "var(--s2)" })),
      panel("Por categoria", null, hbars(r.errorsByCategory, { label: (x) => CATEGORY_LABELS[x.category] ?? x.category, value: (x) => x.errors, color: "var(--s2)" })),
    ),
    panel(
      "Erros mais recorrentes",
      null,
      table(
        [["Categoria"], ["Componente"], ["Código"], ["HTTP", true], ["Ocorrências", true], ["Instalações", true], ["Versões"], ["Último dia"]],
        r.topErrors.map((e) => [CATEGORY_LABELS[e.category] ?? e.category, h("span", { class: "mono" }, e.component), h("span", { class: "mono" }, e.code), e.http_status ? String(e.http_status) : "—", nf.format(e.errors), nf.format(e.installs), e.first_version === e.last_version ? e.first_version : `${e.first_version} a ${e.last_version}`, day(e.last_day)]),
      ),
    ),
  ];
}

function render() {
  const out = $("panel");
  if (!report) return;
  const sections = { overview, versions, sources, failures };
  out.replaceChildren(...sections[tab](report));
}

// ---------------------------------------------------------------------------------------------------------------
// Sessão (Supabase Auth pela API REST, sem biblioteca)
// ---------------------------------------------------------------------------------------------------------------

function saveSession(value) {
  session = value;
  try {
    if (value) sessionStorage.setItem(SESSION_KEY, JSON.stringify(value));
    else sessionStorage.removeItem(SESSION_KEY);
  } catch {
    /* sem sessionStorage, a sessão dura até fechar a aba */
  }
}

function loadSession() {
  // Volta do link mágico: os tokens chegam no fragmento, que nunca vai ao servidor.
  if (location.hash.includes("access_token=")) {
    const p = new URLSearchParams(location.hash.slice(1));
    history.replaceState(null, "", location.pathname);
    const expiresIn = Number(p.get("expires_in") ?? 3600);
    return { access_token: p.get("access_token"), refresh_token: p.get("refresh_token"), expires_at: Date.now() / 1000 + expiresIn };
  }
  if (location.hash.includes("error")) {
    const p = new URLSearchParams(location.hash.slice(1));
    history.replaceState(null, "", location.pathname);
    $("login-note").textContent = p.get("error_description") ?? "O link expirou ou já foi usado. Peça outro.";
  }
  try {
    return JSON.parse(sessionStorage.getItem(SESSION_KEY) ?? "null");
  } catch {
    return null;
  }
}

async function auth(path, body, token) {
  const res = await fetch(`${config.supabaseUrl}/auth/v1/${path}`, {
    method: "POST",
    headers: { apikey: config.anonKey, "content-type": "application/json", ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: JSON.stringify(body ?? {}),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw Object.assign(new Error(data.msg ?? data.error_description ?? `HTTP ${res.status}`), { status: res.status });
  return data;
}

async function refreshSession() {
  if (!session?.refresh_token) return false;
  try {
    const data = await auth("token?grant_type=refresh_token", { refresh_token: session.refresh_token });
    saveSession({ access_token: data.access_token, refresh_token: data.refresh_token, expires_at: data.expires_at ?? Date.now() / 1000 + data.expires_in, email: data.user?.email ?? session.email });
    return true;
  } catch {
    saveSession(null);
    return false;
  }
}

function emailFromToken(token) {
  try {
    return JSON.parse(atob(token.split(".")[1].replace(/-/g, "+").replace(/_/g, "/"))).email ?? "";
  } catch {
    return "";
  }
}

// ---------------------------------------------------------------------------------------------------------------
// Dados
// ---------------------------------------------------------------------------------------------------------------

function query() {
  const days = Number($("f-period").value);
  const to = new Date();
  const from = new Date(to.getTime() - (days - 1) * 86_400_000);
  const q = new URLSearchParams({ from: iso(from), to: iso(to) });
  if ($("f-version").value) q.set("version", $("f-version").value);
  if ($("f-category").value) q.set("category", $("f-category").value);
  return q;
}

async function load(retry = true) {
  if (loading) return;
  loading = true;
  $("refresh").disabled = true;
  $("updated").textContent = "Carregando…";
  try {
    if (session.expires_at && session.expires_at - Date.now() / 1000 < 60) await refreshSession();
    if (!session) return toLogin();
    const res = await fetch(`/api/telemetry/report?${query()}`, { headers: { authorization: `Bearer ${session.access_token}` } });
    if (res.status === 401 && retry && (await refreshSession())) {
      loading = false;
      return load(false);
    }
    if (res.status === 401) return toLogin("Sua sessão terminou. Entre de novo.");
    if (res.status === 403) {
      return fatal("Esta conta não é administradora", "O login funcionou, mas este e-mail não está na lista de administradores da telemetria (tabela telemetry.admins). Peça a quem administra o Supabase do Prancha.");
    }
    if (res.status === 503) return fatal("Painel ainda não configurado", "Faltam as variáveis do Supabase na Vercel. Veja docs/TELEMETRY.md.");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    report = await res.json();
    fillVersions(report.allVersions ?? []);
    show("dash");
    render();
    $("updated").textContent = `Atualizado às ${new Date().toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" })}`;
  } catch {
    $("updated").textContent = "";
    $("panel").replaceChildren(h("div", { class: "panel error-box" }, h("h2", {}, "Não foi possível carregar os dados"), h("p", { class: "muted" }, "O servidor ou o Supabase não respondeu. Tente atualizar em instantes.")));
    show("dash");
  } finally {
    loading = false;
    $("refresh").disabled = false;
  }
}

function fillVersions(list) {
  const select = $("f-version");
  const current = select.value;
  const versions = [...list].sort(compareVersions).reverse();
  select.replaceChildren(h("option", { value: "" }, "Todas"), ...versions.map((v) => h("option", { value: v }, v)));
  select.value = versions.includes(current) ? current : "";
}

function toLogin(note) {
  saveSession(null);
  $("who").hidden = true;
  if (note) $("login-note").textContent = note;
  show("login");
}

// ---------------------------------------------------------------------------------------------------------------
// Início
// ---------------------------------------------------------------------------------------------------------------

async function start() {
  try {
    const res = await fetch("/api/telemetry/config");
    if (res.status === 503) return fatal("Painel ainda não configurado", "Faltam SUPABASE_URL e SUPABASE_ANON_KEY nas variáveis da Vercel. Veja docs/TELEMETRY.md.");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    config = await res.json();
  } catch {
    return fatal("Não foi possível abrir o painel", "O servidor não respondeu. Tente de novo em instantes.");
  }
  const restored = loadSession();
  if (!restored?.access_token) return toLogin();
  saveSession({ ...restored, email: restored.email ?? emailFromToken(restored.access_token) });
  $("who-email").textContent = session.email;
  $("who").hidden = false;
  await load();
}

$("login-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const email = $("email").value.trim();
  $("login-submit").disabled = true;
  $("login-note").textContent = "Enviando…";
  try {
    // create_user: false — o login não cria contas. Só quem já existe no Supabase Auth recebe o link.
    await auth(`otp?redirect_to=${encodeURIComponent(`${location.origin}/admin`)}`, { email, create_user: false });
  } catch {
    /* A resposta é a mesma com ou sem conta, para não revelar quem é administrador. */
  }
  $("login-note").textContent = "Se este e-mail for de um administrador, o link de acesso chega em instantes. Ele vale por pouco tempo.";
  $("login-submit").disabled = false;
});

$("logout").addEventListener("click", async () => {
  const token = session?.access_token;
  saveSession(null);
  report = null;
  if (token) await auth("logout", {}, token).catch(() => undefined);
  toLogin();
});

for (const id of ["f-period", "f-version", "f-category"]) $(id).addEventListener("change", () => load());
$("refresh").addEventListener("click", () => load());
$("fatal-retry").addEventListener("click", () => {
  show("booting");
  start();
});

for (const name of ["overview", "versions", "sources", "failures"]) {
  $(`tab-${name}`).addEventListener("click", () => {
    tab = name;
    for (const other of ["overview", "versions", "sources", "failures"]) $(`tab-${other}`).setAttribute("aria-selected", String(other === name));
    render();
  });
}

// Atualiza sozinho a cada 5 minutos com a aba à vista.
setInterval(() => {
  if (!document.hidden && session && report) load();
}, 5 * 60_000);

start();
