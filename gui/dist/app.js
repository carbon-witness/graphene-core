"use strict";
const { invoke } = window.__TAURI__.core;
const { ask } = window.__TAURI__.dialog;
const $ = (id) => document.getElementById(id);

const MAX_LINES = 5000;
const SLOW_MS = 1500;
// Blocks fetched while syncing are hours or years old; only live blocks can be "slow"
const isSlow = (ms) => ms > SLOW_MS && ms < 3600 * 1000;
const GOT_BLOCK = /Got block: #(\d+) (\w+) time: (\S+) transaction\(s\): (\d+) latency: (-?\d+) ms from: (\S+)\s+irreversible: (\d+) \(-(\d+)\)/;

let status = null;
let lines = [];          // [seq, text], already masked by the app
let cursor = 0;
let mode = "raw";
let follow = true;       // keep the feed scrolled to the newest line
let unseen = 0;
const headSamples = [];  // [time ms, head block] over the last minute

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
const fmt = (n) => (n == null ? "—" : Number(n).toLocaleString("ru-RU"));
function human(s) {
  if (s == null) return "—";
  if (s < 120) return `${s} с`;
  if (s < 7200) return `${Math.floor(s / 60)} мин`;
  if (s < 172800) return `${Math.floor(s / 3600)} ч`;
  return `${Math.floor(s / 86400)} дн`;
}

function blocksPerMinute() {
  if (headSamples.length < 2) return null;
  const [t0, b0] = headSamples[0];
  const [t1, b1] = headSamples[headSamples.length - 1];
  return t1 > t0 ? Math.round(((b1 - b0) * 60000) / (t1 - t0)) : null;
}

function renderStatus(s) {
  $("dot").className = `dot ${s.color}`;
  $("summary").textContent = s.summary;
  const running = s.pid != null;
  $("btn-start").disabled = running || s.phase === "stopping";
  $("btn-stop").disabled = !running && !["waiting_restart", "failed"].includes(s.phase);
  $("btn-restart").disabled = !running;
  $("btn-kill").classList.toggle("hidden", !(running && (s.phase === "stop_timed_out" || !s.can_stop_cleanly)));

  const alert = $("alert");
  let msg = "";
  if (s.chain_id_mismatch) msg = `На ${s.rpc_endpoint} отвечает нода другой сети (chain ID не совпадает с логом нашей ноды).`;
  else if (s.phase === "failed") msg = `${s.last_exit || "Нода падает"}.\nПоследние строки лога — во вкладке «Журнал».`;
  else if (s.phase === "stop_timed_out") msg = "Нода не остановилась за 60 секунд. Можно подождать ещё или остановить принудительно (тогда при следующем запуске будет replay).";
  else if (running && !s.can_stop_cleanly) msg = "Нода запущена без события остановки: остановить её можно только принудительно.";
  if (msg) { alert.textContent = msg; alert.className = "alert error"; }
  else if (Date.now() > Number(alert.dataset.until || 0)) alert.className = "alert hidden";

  const c = s.chain;
  if (c && running) {
    const now = Date.now();
    headSamples.push([now, c.head_block]);
    while (headSamples.length && now - headSamples[0][0] > 60000) headSamples.shift();
  } else headSamples.length = 0;

  let pct = null, title = "Синхронизация";
  if (s.phase === "starting" && s.log.replay_percent != null) { pct = s.log.replay_percent; title = "Replay базы"; }
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
  $("proc").textContent = running ? `PID ${s.pid}${s.attached ? " (найдена запущенной)" : ""}` : "не запущена";
  $("lastexit").textContent = s.last_exit || "—";
}

$("btn-start").onclick = () => call("node_start");
$("btn-stop").onclick = () => call("node_stop");
$("btn-restart").onclick = () => call("node_restart");
$("btn-kill").onclick = async () => {
  if (await ask("Остановить ноду принудительно? При следующем запуске нода проведёт replay базы.", { title: "Graphene Node", kind: "warning" }))
    call("node_kill");
};
$("btn-copy").onclick = () => call("copy_rpc");
$("btn-open-data").onclick = () => call("open_data_dir");

// ---------- journal ----------
const feed = $("feed");
const blocksBody = document.querySelector("#blocks tbody");

function lineClass(t) {
  if (/exception|assert|error|failed/i.test(t)) return "err";
  if (/warn/i.test(t)) return "warn";
  const m = GOT_BLOCK.exec(t);
  if (m && isSlow(Number(m[5]))) return "warn";
  return "";
}

function visible(t) {
  const q = $("filter").value.trim().toLowerCase();
  if ($("hide-blocks").checked && t.includes("Got block:")) return false;
  return !q || t.toLowerCase().includes(q);
}

function lineEl(t) {
  const d = document.createElement("div");
  d.textContent = t.replace(/[ \t]{2,}/g, "  "); // the log pads columns with tabs and runs of spaces
  const c = lineClass(t);
  if (c) d.className = c;
  return d;
}

function blockRow(m) {
  const tr = document.createElement("tr");
  const cells = [fmt(m[1]), m[3].replace("T", " "), m[6], m[4], `${fmt(m[5])} мс`, m[8]];
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
    for (const [, t] of src) if (visible(t)) frag.appendChild(lineEl(t));
    feed.appendChild(frag);
    while (feed.childElementCount > MAX_LINES) feed.firstChild.remove();
    if (follow) feed.scrollTop = feed.scrollHeight;
  } else {
    if (full) blocksBody.replaceChildren();
    for (const [, t] of src) {
      const m = GOT_BLOCK.exec(t);
      if (m && visible(t)) blocksBody.prepend(blockRow(m)); // newest first
    }
  }
}

function renderJournalStatus() {
  for (let i = lines.length - 1; i >= 0; i--) {
    const m = GOT_BLOCK.exec(lines[i][1]);
    if (!m) continue;
    const rate = blocksPerMinute();
    $("jstatus").textContent = `Последний блок: #${fmt(m[1])} · ${m[3].replace("T", " ")} · от ${m[6]} · задержка ${fmt(m[5])} мс · до необратимого ${m[8]}` +
      (rate != null ? ` · ${fmt(rate)} блоков/мин` : "");
    return;
  }
}

async function pollLog() {
  const added = await invoke("get_log", { cursor }).catch(() => []);
  if (!added.length) return;
  cursor = added[added.length - 1][0];
  lines = lines.concat(added).slice(-MAX_LINES);
  if ($("journal").classList.contains("active")) {
    renderJournal(false, added);
    if (!follow) {
      unseen += added.filter(([, t]) => visible(t)).length;
      $("btn-latest").textContent = `↓ к последним (${unseen} новых)`;
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
$("hide-blocks").addEventListener("change", () => renderJournal(true));
$("btn-copy-log").onclick = () => {
  const text = lines.map(([, t]) => t).filter(visible).join("\n");
  navigator.clipboard.writeText(text).catch(showError);
};
$("btn-open-log").onclick = () => call("open_log");

// ---------- settings ----------
const form = $("settings-form");
function rpcIsLocal(ep) { return /^(127\.0\.0\.1|localhost):\d+$/.test(ep.trim()); }
async function loadSettings() {
  const s = await call("get_settings");
  form.node_exe.value = s.node_exe;
  form.data_dir.value = s.data_dir;
  form.rpc_endpoint.value = s.rpc_endpoint;
  form.start_node_with_app.checked = s.start_node_with_app;
  $("rpc-warning").classList.toggle("hidden", rpcIsLocal(s.rpc_endpoint));
}
form.rpc_endpoint.addEventListener("input", () => $("rpc-warning").classList.toggle("hidden", rpcIsLocal(form.rpc_endpoint.value)));
form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const settings = {
    node_exe: form.node_exe.value.trim(),
    data_dir: form.data_dir.value.trim(),
    rpc_endpoint: form.rpc_endpoint.value.trim(),
    start_node_with_app: form.start_node_with_app.checked,
  };
  await call("save_settings", { settings });
  $("saved").textContent = "Сохранено";
  setTimeout(() => ($("saved").textContent = ""), 3000);
  if (status && status.pid != null &&
      await ask("Настройки применятся после перезапуска ноды. Перезапустить сейчас?", { title: "Graphene Node", kind: "info" }))
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
loadSettings();
tick();
setInterval(tick, 1000);
