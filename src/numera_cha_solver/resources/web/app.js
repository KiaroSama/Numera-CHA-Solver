"use strict";
const $ = (selector) => document.querySelector(selector);
let catalog = [];
let currentTask = null;
const message = (node, text) => { node.textContent = text; };
async function api(path, options = {}) {
  const response = await fetch(path, { ...options, signal: AbortSignal.timeout(125000), headers: { "Content-Type": "application/json", ...options.headers } });
  const payload = await response.json();
  if (!response.ok) throw new Error(payload.error?.message || `Request failed (${response.status}).`);
  return payload;
}
function element(tag, text, className = "") {
  const node = document.createElement(tag); node.textContent = text; node.className = className; return node;
}
async function load() {
  try {
    const [inventory, models] = await Promise.all([api("/v1/capabilities"), api("/v1/models")]);
    catalog = inventory.capabilities;
    message($("#connection-state"), "Connected locally"); $("#connection-dot").classList.add("connected");
    message($("#connect"), "Session connected");
    const area = $("#model-status"); area.replaceChildren();
    for (const [label, value, note] of [["MODEL ASSETS", models.models[0].availability.replaceAll("_", " "), "Immutable assets; checksums validated"], ["CONFIGURED DEVICE", models.device.toUpperCase(), "Actual execution evidence appears in each result"], ["EXTERNAL INFERENCE", "Disabled", "No paid providers or third-party automation"]]) {
      const group = document.createElement("div"); group.append(element("small", label), element("strong", value), element("p", note)); area.append(group);
    }
    renderCoverage(); await tasks();
  } catch (error) { message($("#connection-state"), "Not connected"); message($("#form-error"), error.message); }
}
function renderCoverage() {
  const list = $("#coverage-list"); list.replaceChildren();
  const query = $("#filter").value.toLowerCase();
  const groups = new Map();
  for (const row of catalog) {
    if (query && !`${row.provider} ${row.subtype}`.toLowerCase().includes(query)) continue;
    if (!groups.has(row.catalog_id)) groups.set(row.catalog_id, []);
    groups.get(row.catalog_id).push(row);
  }
  for (const [id, rows] of groups) {
    const detail = document.createElement("details"), summary = document.createElement("summary");
    summary.append(element("span", `${id} / ${rows[0].provider}`), element("span", `${rows.length} subtypes · ${rows.every(row => row.implementation_state === "blocked") ? "BLOCKED" : "LIMITED EVIDENCE"}`));
    const content = element("div", "", "subtypes");
    for (const row of rows) {
      const item = element("div", "", "subtype"); item.append(element("strong", `${row.subtype} · ${row.implementation_state.replaceAll("_", " ")}`), element("p", `${row.mode} / ${row.availability}`));
      if (row.blockers.length) item.append(element("p", row.blockers.join(" ")));
      content.append(item);
    }
    detail.append(summary, content); list.append(detail);
  }
  if (!groups.size) list.append(element("p", "No matching capability. Try a provider or subtype name.", "muted"));
}
async function tasks() {
  const data = await api("/v1/tasks"), list = $("#task-list"); list.replaceChildren();
  if (!data.tasks.length) list.append(element("p", "No tasks yet. Run a recognition to create one.", "muted"));
  for (const task of data.tasks) {
    const row = element("div", "", "task-row"); row.append(element("code", task.id), element("span", task.outcome || task.state));
    if (task.state !== "terminal") {
      const cancel = element("button", "Cancel", "quiet"); cancel.addEventListener("click", async () => { cancel.disabled = true; try { await api(`/v1/tasks/${task.id}/cancel`, { method: "POST" }); await tasks(); } catch (error) { message($("#form-error"), error.message); cancel.disabled = false; } }); row.append(cancel);
    }
    list.append(row);
  }
}
function showResult(result) {
  message($("#outcome"), result.outcome.replaceAll("_", " ").toUpperCase());
  const area = $("#result"); area.replaceChildren();
  area.append(element("div", result.answer || result.error?.message || "Recognition abstained; supported input or user action is needed.", "answer"));
  area.append(element("p", `Device: ${result.device || "not executed"} · Mode: ${result.mode || "none"} · Server verified: ${result.server_verified === true ? "yes" : "no"}`, "result-meta"));
  const details = document.createElement("details"); details.append(element("summary", "Inspect execution evidence"), element("pre", JSON.stringify(result.evidence || {}, null, 2))); area.append(details);
}
$("#connect").addEventListener("click", () => { $("#auth-dialog").showModal(); });
$("#close-auth").addEventListener("click", () => $("#auth-dialog").close());
$("#auth-form").addEventListener("submit", async (event) => {
  event.preventDefault(); const token = $("#api-token").value;
  try { await api("/v1/session", { method: "POST", headers: { Authorization: `Bearer ${token}` } }); $("#api-token").value = ""; $("#auth-dialog").close(); message($("#auth-error"), ""); message($("#form-error"), ""); await load(); }
  catch (error) { message($("#auth-error"), error.message); }
});
for (const input of document.querySelectorAll("input[name=kind]")) input.addEventListener("change", () => { $("#text-input").hidden = input.value !== "text"; $("#image-input").hidden = input.value !== "image"; });
$("#recognition-form").addEventListener("submit", async (event) => {
  event.preventDefault(); message($("#form-error"), ""); const button = $("#recognize"); button.disabled = true; message(button, "Recognizing…");
  try {
    const kind = document.querySelector("input[name=kind]:checked").value;
    const payload = { kind };
    if (kind === "text") payload.text = $("#expression").value;
    else {
      const file = $("#image-file").files[0]; if (!file || file.size > 8 * 1024 * 1024) throw new Error("Choose a licensed local image up to 8 MB.");
      const bytes = new Uint8Array(await file.arrayBuffer()); let binary = "";
      for (let i = 0; i < bytes.length; i += 32768) binary += String.fromCharCode(...bytes.subarray(i, i + 32768));
      payload.asset_base64 = btoa(binary);
    }
    const task = await api("/v1/tasks", { method: "POST", body: JSON.stringify(payload), headers: { "Idempotency-Key": crypto.randomUUID() } }); currentTask = task.id;
    const deadline = performance.now() + 122000;
    while (performance.now() < deadline) {
      const status = await api(`/v1/tasks/${currentTask}`);
      if (status.state === "terminal") { showResult(status.result || { outcome: status.outcome }); await tasks(); return; }
      await new Promise(resolve => setTimeout(resolve, 400));
    }
    throw new Error("Task exceeded its deadline. Refresh task history to inspect its outcome.");
  } catch (error) { message($("#form-error"), error.message); }
  finally { button.disabled = false; message(button, "Run recognition →"); currentTask = null; }
});
$("#filter").addEventListener("input", renderCoverage);
$("#refresh").addEventListener("click", load);
$("#refresh-tasks").addEventListener("click", () => tasks().catch(error => message($("#form-error"), error.message)));
load();
