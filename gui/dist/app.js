"use strict";
const { invoke } = window.__TAURI__.core;
const { ask } = window.__TAURI__.dialog;
const $ = (id) => document.getElementById(id);

const MAX_LINES = 5000;
const SLOW_MS = 1500;
// Blocks fetched while syncing are hours or years old; only live blocks can be "slow"
const isSlow = (ms) => ms > SLOW_MS && ms < 3600 * 1000;
const GOT_BLOCK = /Got block: #(\d+) (\w+) time: (\S+) transaction\(s\): (\d+) latency: (-?\d+) ms from: (\S+)\s+irreversible: (\d+) \(-(\d+)\)/;
// The level the node writes before "]" (fc file appender); older logs have none
const LEVEL = / (debug|info|warn|error) +\] /;
// Continuation lines of a multi-line message (exception details) start without a timestamp
const STARTS_ENTRY = /^\d{4}-\d\d-\d\dT/;
const LEVEL_RANK = { debug: 0, info: 1, warn: 2, error: 3 };

// The tray's glyphs (glyphs.rs), same shapes and colours: a white disc with a grey rim and a coloured sign,
// with a yellow dot while the node starts, syncs or stops
const DISC = '<circle cx="12" cy="12" r="10.9" fill="#fff" stroke="#8c959f" stroke-width="1.2"/>';
const GLYPH_SVG = {
  play: `<svg viewBox="0 0 24 24">${DISC}<path d="M7.5 4.7v14.6L20 12z" fill="#2ea043"/></svg>`,
  pause: `<svg viewBox="0 0 24 24">${DISC}<path d="M6.4 5.4h4v13.2h-4zM13.6 5.4h4v13.2h-4z" fill="#e0a100"/></svg>`,
  busy: `<svg viewBox="0 0 24 24">${DISC}<circle cx="12" cy="12" r="7.3" fill="#e0a100"/></svg>`,
  cross: `<svg viewBox="0 0 24 24">${DISC}<path d="M6.3 6.3l11.4 11.4M17.7 6.3L6.3 17.7" stroke="#d1242f" stroke-width="3.2"/></svg>`,
  restart: `<svg viewBox="0 0 24 24">${DISC}<path d="M17.05 9.87A5.48 5.48 0 1 1 12.66 6.56" fill="none" stroke="#1c9bd6" stroke-width="2.96"/><path d="M13.93 11.19L20.17 8.55L15.05 7.45z" fill="#1c9bd6"/></svg>`,
};

let status = null;
let lines = [];          // [seq, text, level], already masked by the app
let cursor = 0;
let mode = "raw";
let follow = true;       // keep the feed scrolled to the newest line
let unseen = 0;
const headSamples = [];  // [time ms, head block] over the last minute, from fresh API answers only

// ---------- translations ----------
let dict = {}, fallback = {};
const t = (key, args = {}) =>
  (dict[key] ?? fallback[key] ?? key).replace(/\{(\w+)\}/g, (m, k) => (k in args ? args[k] : m));
const loadLocale = (lang) => fetch(`locales/${lang}.json`).then((r) => r.json());

async function setLanguage(lang) {
  if (!Object.keys(fallback).length) fallback = await loadLocale("en");
  dict = await loadLocale(lang).catch(() => fallback);
  document.documentElement.lang = lang;
  document.querySelectorAll("[data-i18n]").forEach((el) => (el.textContent = t(el.dataset.i18n)));
  document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => (el.placeholder = t(el.dataset.i18nPlaceholder)));
  if (status) renderStatus(status);
  renderJournalStatus();
  renderJournal(true);
}

const fmt = (n) => (n == null ? "—" : Number(n).toLocaleString(dict._number_locale || "en-US"));
function human(s) {
  if (s == null) return "—";
  if (s < 120) return t("dur.s", { n: s });
  if (s < 7200) return t("dur.min", { n: Math.floor(s / 60) });
  if (s < 172800) return t("dur.h", { n: Math.floor(s / 3600) });
  return t("dur.d", { n: Math.floor(s / 86400) });
}

// ---------- errors ----------
function showError(e) {
  const a = $("alert");
  a.textContent = String(e);
  a.className = "alert error";
  a.dataset.until = Date.now() + 8000; // keep it past the next status refresh
}

async function call(cmd, args) {
  try { return await invoke(cmd, args); } catch (e) { showError(e); throw e; }
}

// ---------- tabs ----------
document.querySelectorAll(".tab").forEach((b) => b.addEventListener("click", () => {
  document.querySelectorAll(".tab").forEach((x) => x.classList.toggle("active", x === b));
  document.querySelectorAll(".page").forEach((p) => p.classList.toggle("active", p.id === b.dataset.tab));
  if (b.dataset.tab === "journal") renderJournal(true);
}));

// ---------- dashboard ----------
function blocksPerMinute() {
  if (headSamples.length < 2) return null;
  const [t0, b0] = headSamples[0];
  const [t1, b1] = headSamples[headSamples.length - 1];
  return t1 - t0 >= 5000 ? Math.round(((b1 - b0) * 60000) / (t1 - t0)) : null;
}

function renderStatus(s) {
  if ($("dot").dataset.glyph !== s.glyph) {
    $("dot").dataset.glyph = s.glyph;
    $("dot").innerHTML = GLYPH_SVG[s.glyph] || GLYPH_SVG.pause; // fixed markup below, no data in it
  }
  $("summary").textContent = s.summary;
  const running = s.pid != null;
  $("btn-start").disabled = running || s.phase === "stopping";
  $("btn-stop").disabled = !running && !["waiting_restart", "failed"].includes(s.phase);
  $("btn-restart").disabled = !running;
  $("btn-kill").classList.toggle("hidden", !(running && (s.phase === "stop_timed_out" || !s.can_stop_cleanly)));

  const alert = $("alert");
  let msg = "";
  if (s.chain_id_mismatch) msg = t("alert.mismatch", { endpoint: s.rpc_endpoint });
  else if (s.phase === "failed") msg = t("alert.failed", { what: s.last_exit || "" });
  else if (s.phase === "stop_timed_out") msg = t("alert.stop_timeout");
  else if (running && !s.can_stop_cleanly) msg = t("alert.no_event");
  else if (running && s.api_stale_seconds != null) msg = t("alert.api_stale", { s: s.api_stale_seconds });
  if (msg) { alert.textContent = msg; alert.className = "alert error"; }
  else if (Date.now() > Number(alert.dataset.until || 0)) alert.className = "alert hidden";

  const c = s.chain;
  if (c && running && s.api_stale_seconds == null) {
    const now = Date.now();
    headSamples.push([now, c.head_block]);
    while (headSamples.length && now - headSamples[0][0] > 60000) headSamples.shift();
  } else if (!running) headSamples.length = 0;

  let pct = null, title = t("dash.sync");
  if (s.phase === "starting" && s.log.replay_percent != null) { pct = s.log.replay_percent; title = t("dash.replay"); }
  else if (c && running) pct = s.sync_percent;
  $("progress-title").textContent = title;
  $("progress-value").textContent = pct == null ? "—" : `${pct.toFixed(1)}%`;
  $("progress-bar").style.width = `${pct || 0}%`;

  $("head").textContent = fmt(c ? c.head_block : s.log.last_block);
  $("irr").textContent = fmt(c && c.irreversible_block);
  $("lag").textContent = c && running ? human(s.lag_seconds) : "—";
  $("rate").textContent = fmt(blocksPerMinute());
  $("chain").textContent = (c && c.chain_id) || s.log.chain_id || "—";
  $("rpc").textContent = `ws://${s.rpc_endpoint}`;
  $("datadir").textContent = s.data_dir;
  $("proc").textContent = running ? t(s.attached ? "proc.attached" : "proc.running", { pid: s.pid }) : t("proc.not_running");
  $("lastexit").textContent = s.last_exit || "—";
  $("peer-count").textContent = s.peers ? fmt(s.peers.length) : (running && s.peers_note) || "—";
  renderPeers(s);
}

// ---------- peers ----------
function bytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} MB`;
  return `${(n / 1073741824).toFixed(2)} GB`;
}

function renderPeers(s) {
  const running = s.pid != null;
  const peers = s.peers || [];
  let note;
  if (!running) note = t("peers.not_running");
  else if (!s.peers) note = s.peers_note || t("peers.waiting");
  else if (!peers.length) note = t("peers.none");
  else note = t("peers.count", { n: peers.length });
  $("peers-status").textContent = note;
  $("peers-status").classList.toggle("warn", running && !!s.peers && !peers.length);
  const now = Date.now() / 1000;
  const ago = (ts) => (ts ? human(Math.max(0, Math.round(now - ts))) : "—");
  const body = $("peer-table").tBodies[0];
  body.replaceChildren(...[...peers].sort((a, b) => a.connected_since - b.connected_since).map((p) => {
    const tr = document.createElement("tr");
    for (const v of [p.addr, t(p.inbound ? "peers.inbound" : "peers.outbound"), p.user_agent || "—",
                     p.platform || "—", fmt(p.head_block), ago(p.connected_since), ago(p.last_received),
                     bytes(p.bytes_received), bytes(p.bytes_sent)]) {
      const td = document.createElement("td");
      td.textContent = v;
      tr.append(td);
    }
    return tr;
  }));
  $("peer-table").classList.toggle("hidden", !peers.length);
}

$("btn-start").onclick = () => invoke("node_start").catch(() => {}); // a failure opens an error box
$("btn-stop").onclick = () => call("node_stop");
$("btn-restart").onclick = () => invoke("node_restart").catch(() => {}); // a failure opens an error box
$("btn-kill").onclick = async () => {
  if (await ask(t("dlg.kill"), { title: "Graphene Node", kind: "warning" })) call("node_kill");
};
$("btn-copy").onclick = () => call("copy_rpc");
$("btn-open-data").onclick = () => call("open_data_dir");

// ---------- journal ----------
const feed = $("feed");
const blocksBody = document.querySelector("#blocks tbody");

function levelOf(text, previous) {
  const m = LEVEL.exec(text);
  if (m) return m[1];
  return STARTS_ENTRY.test(text) ? "info" : previous; // details of the entry above
}

function visible([, text, level]) {
  if (LEVEL_RANK[level] < ({ all: 0, warn: 2, error: 3 })[$("level").value]) return false;
  if ($("hide-blocks").checked && text.includes("Got block:")) return false;
  const q = $("filter").value.trim().toLowerCase();
  return !q || text.toLowerCase().includes(q);
}

function lineEl([, text, level]) {
  const d = document.createElement("div");
  d.textContent = text.replace(/[ \t]{2,}/g, "  "); // the log pads columns with tabs and runs of spaces
  if (level !== "info") d.className = level;
  return d;
}

function blockRow(m) {
  const tr = document.createElement("tr");
  const cells = [fmt(m[1]), m[3].replace("T", " "), m[6], m[4], t("unit.ms", { n: fmt(m[5]) }), m[8]];
  cells.forEach((v, i) => {
    const td = document.createElement("td");
    td.textContent = v;
    if (i === 4 && isSlow(Number(m[5]))) td.className = "slow";
    tr.appendChild(td);
  });
  return tr;
}

function renderJournal(full, added = []) {
  const src = full ? lines : added;
  if (mode === "raw") {
    if (full) feed.replaceChildren();
    const frag = document.createDocumentFragment();
    for (const l of src) if (visible(l)) frag.appendChild(lineEl(l));
    feed.appendChild(frag);
    while (feed.childElementCount > MAX_LINES) feed.firstChild.remove();
    if (follow) feed.scrollTop = feed.scrollHeight;
  } else {
    if (full) blocksBody.replaceChildren();
    for (const l of src) {
      const m = GOT_BLOCK.exec(l[1]);
      if (m && visible(l)) blocksBody.prepend(blockRow(m)); // newest first
    }
    // Live, the node logs a block every few seconds: keep the table as bounded as the raw feed
    while (blocksBody.childElementCount > MAX_LINES) blocksBody.lastChild.remove();
  }
}

function renderJournalStatus() {
  for (let i = lines.length - 1; i >= 0; i--) {
    const m = GOT_BLOCK.exec(lines[i][1]);
    if (!m) continue;
    const rate = blocksPerMinute();
    $("jstatus").textContent =
      t("journal.status", { block: fmt(m[1]), time: m[3].replace("T", " "), witness: m[6], latency: fmt(m[5]), irr: m[8] }) +
      (rate != null ? t("journal.rate", { rate: fmt(rate) }) : "");
    return;
  }
  $("jstatus").textContent = t("journal.none");
}

async function pollLog() {
  const fresh = await invoke("get_log", { cursor }).catch(() => []);
  if (!fresh.length) return;
  cursor = fresh[fresh.length - 1][0];
  let prev = lines.length ? lines[lines.length - 1][2] : "info";
  const added = fresh.map(([seq, text]) => (prev = levelOf(text, prev), [seq, text, prev]));
  lines = lines.concat(added).slice(-MAX_LINES);
  if ($("journal").classList.contains("active")) {
    renderJournal(false, added);
    if (!follow) {
      unseen += added.filter(visible).length;
      $("btn-latest").textContent = t("journal.latest", { n: unseen });
    }
  }
  renderJournalStatus();
}

feed.addEventListener("scroll", () => {
  const atBottom = feed.scrollHeight - feed.scrollTop - feed.clientHeight < 30;
  follow = atBottom;
  if (atBottom) unseen = 0;
  $("btn-latest").classList.toggle("hidden", atBottom || mode !== "raw");
});
$("btn-latest").onclick = () => { follow = true; unseen = 0; feed.scrollTop = feed.scrollHeight; $("btn-latest").classList.add("hidden"); };
document.querySelectorAll(".mode").forEach((b) => b.addEventListener("click", () => {
  mode = b.dataset.mode;
  document.querySelectorAll(".mode").forEach((x) => x.classList.toggle("active", x === b));
  feed.classList.toggle("hidden", mode !== "raw");
  $("blocks").classList.toggle("hidden", mode !== "blocks");
  $("btn-latest").classList.add("hidden");
  renderJournal(true);
}));
$("filter").addEventListener("input", () => renderJournal(true));
$("level").addEventListener("change", () => renderJournal(true));
$("hide-blocks").addEventListener("change", () => renderJournal(true));
$("btn-copy-log").onclick = () => {
  const text = lines.filter(visible).map(([, l]) => l).join("\n");
  navigator.clipboard.writeText(text).catch(showError);
};
$("btn-open-log").onclick = () => call("open_log");

// ---------- settings ----------
const form = $("settings-form");
function rpcIsLocal(ep) { return /^(127\.0\.0\.1|localhost):\d+$/.test(ep.trim()); }
async function loadSettings() {
  const [s, langs, autostart] = await Promise.all([call("get_settings"), call("get_languages"), call("get_autostart")]);
  form.autostart.checked = autostart;
  form.language.replaceChildren(...langs.map(([code, name]) => new Option(name, code, false, code === s.language)));
  form.node_exe.value = s.node_exe;
  form.data_dir.value = s.data_dir;
  form.rpc_endpoint.value = s.rpc_endpoint;
  form.start_node_with_app.checked = s.start_node_with_app;
  $("rpc-warning").classList.toggle("hidden", rpcIsLocal(s.rpc_endpoint));
  return s;
}
form.rpc_endpoint.addEventListener("input", () => $("rpc-warning").classList.toggle("hidden", rpcIsLocal(form.rpc_endpoint.value)));
form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const before = await call("get_settings");
  const settings = {
    node_exe: form.node_exe.value.trim(),
    data_dir: form.data_dir.value.trim(),
    rpc_endpoint: form.rpc_endpoint.value.trim(),
    start_node_with_app: form.start_node_with_app.checked,
    language: form.language.value,
  };
  await call("save_settings", { settings });
  // The startup entry lives in the Windows registry, not in the settings file
  if (form.autostart.checked !== (await call("get_autostart"))) await call("set_autostart", { enabled: form.autostart.checked });
  if (settings.language !== before.language) await setLanguage(settings.language);
  $("saved").textContent = t("settings.saved");
  setTimeout(() => ($("saved").textContent = ""), 3000);
  const nodeOptionsChanged = ["node_exe", "data_dir", "rpc_endpoint"].some((k) => settings[k] !== before[k]);
  if (nodeOptionsChanged && status && status.pid != null &&
      await ask(t("dlg.restart_after_save"), { title: "Graphene Node", kind: "info" }))
    call("node_restart");
});

// ---------- loop ----------
async function tick() {
  try {
    status = await invoke("get_status");
    renderStatus(status);
  } catch (e) { showError(e); }
  await pollLog();
}
(async () => {
  const s = await loadSettings().catch(() => ({ language: "en" }));
  await setLanguage(s.language);
  tick();
  setInterval(tick, 1000);
})();
