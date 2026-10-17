const state = { managers: [], manager: null, sessions: [], session: null, cursor: null, timer: null };

async function getJSON(url) {
  const response = await fetch(url, { headers: { accept: "application/json" } });
  const body = await response.json().catch(() => ({ error: "invalid JSON" }));
  if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body;
}

function el(id) { return document.getElementById(id); }

function short(value) { return value === undefined || value === null || value === "" ? "—" : String(value); }

function renderManagers() {
  const list = el("manager-list");
  list.textContent = "";
  for (const manager of state.managers) {
    const li = document.createElement("li");
    li.className = manager.name === state.manager ? "selected" : "";
    const version = manager.ok ? `v${manager.readiness?.versions?.protocol ?? "?"}` : "unreachable";
    li.innerHTML = `<strong>${manager.name}</strong><br><span class="muted">${manager.address}</span><br>` +
      (manager.ok
        ? `<span class="good">${manager.readiness?.state ?? "?"}</span> · ${version} · ${manager.readiness?.sessions?.running ?? 0} running`
        : `<span class="bad">${manager.error}</span>`);
    li.onclick = () => { state.manager = manager.name; state.session = null; renderManagers(); loadSessions(); };
    list.appendChild(li);
  }
  el("summary").textContent = `${state.managers.filter(m => m.ok).length}/${state.managers.length} managers reachable`;
}

async function refreshManagers() {
  try { state.managers = (await getJSON("/api/managers")).managers; }
  catch (error) { state.managers = [{ name: "dashboard", address: "—", ok: false, error: String(error) }]; }
  renderManagers();
}

function renderSessions() {
  const rows = el("session-rows");
  rows.textContent = "";
  for (const session of state.sessions) {
    const tr = document.createElement("tr");
    tr.className = session.id === state.session ? "selected" : "";
    const view = session.view ? "up" : "down";
    tr.innerHTML = `<td>${session.id}</td><td>${short(session.label)}</td>` +
      `<td>${short(session.state)}</td><td>${view}</td><td>${short(session.scope)}</td>` +
      `<td>${session.resolution ?? "?"} @ ${session.scale ?? "?"}</td><td>${short(session.idle_since)}</td>`;
    tr.onclick = () => { state.session = session.id; renderSessions(); loadHistory(true); renderDetail(session); };
    rows.appendChild(tr);
  }
  el("sessions-for").textContent = state.manager ? `· ${state.manager}` : "";
  el("sessions-note").textContent = state.manager
    ? (state.sessions.length ? "" : "no sessions for this manager")
    : "pick a manager";
}

async function loadSessions() {
  if (!state.manager) { state.sessions = []; renderSessions(); return; }
  try { state.sessions = (await getJSON(`/api/sessions?manager=${encodeURIComponent(state.manager)}`)).sessions ?? []; }
  catch (error) { state.sessions = []; el("sessions-note").textContent = String(error); }
  renderSessions();
  if (state.session) {
    const session = state.sessions.find(s => s.id === state.session);
    if (session) renderDetail(session);
  }
}

function renderDetail(session) {
  el("detail-id").textContent = session.id;
  el("detail-body").innerHTML =
    `<div><strong>state</strong> ${short(session.state)}</div>` +
    `<div><strong>view</strong> ${session.view ? "up" : "down"} <span class="muted">(view down ≠ session dead)</span></div>` +
    `<div><strong>backend</strong> ${short(session.backend)} · <strong>scope</strong> ${short(session.scope)} · <strong>origin</strong> ${short(session.origin)}</div>` +
    `<div><strong>size</strong> ${session.resolution ?? "?"} @ ${session.scale ?? "?"} · <strong>renderer</strong> ${short(session.renderer)}</div>`;
  el("open-view").disabled = false;
  el("view-note").textContent = "";
}

async function loadHistory(reset) {
  if (!state.manager || !state.session) return;
  if (reset) { state.cursor = null; el("history").textContent = ""; }
  const query = new URLSearchParams({ manager: state.manager, session: state.session, limit: "50" });
  if (state.cursor) query.set("cursor", state.cursor);
  try {
    const page = await getJSON(`/api/history?${query}`);
    for (const record of page.records ?? []) {
      const li = document.createElement("li");
      li.innerHTML = `<span class="badge">${record.event}</span> <span class="muted">${record.at}</span><br>` +
        `<span class="muted">scope ${record.scope} · origin ${record.origin}${record.cause ? " · cause " + record.cause : ""}</span>`;
      el("history").appendChild(li);
    }
    state.cursor = page.next_cursor ?? null;
    el("older").disabled = !state.cursor;
  } catch (error) {
    const li = document.createElement("li");
    li.className = "bad";
    li.textContent = String(error);
    el("history").appendChild(li);
  }
}

async function openView() {
  if (!state.manager || !state.session) return;
  el("view-note").textContent = "starting…";
  try {
    const view = await (await fetch("/api/view", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ manager: state.manager, session: state.session }),
    })).json();
    if (view.error) throw new Error(view.error);
    const url = view.address?.includes("://") ? view.address : `http://${view.address}`;
    const target = view.credential ? `${url}${url.includes("?") ? "&" : "?"}credential=${encodeURIComponent(view.credential)}` : url;
    el("view-note").textContent = `generation ${view.generation ?? "?"} · opened`;
    window.open(target, "_blank", "noopener");
  } catch (error) {
    el("view-note").textContent = String(error);
  }
}

el("open-view").onclick = openView;
el("older").onclick = () => loadHistory(false);

async function tick() {
  await refreshManagers();
  if (state.manager) await loadSessions();
}
tick();
state.timer = setInterval(tick, 5000);
