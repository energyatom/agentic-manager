// Spike page (E1 story 1): exchanges the launch code for a session, then calls
// the API and opens the event stream. The React + TypeScript UI replaces it.
"use strict";
(async () => {
  const status = document.getElementById("status");
  const params = new URLSearchParams(location.hash.slice(1));
  const code = params.get("code");
  history.replaceState(null, "", location.pathname); // strip the fragment
  if (!code) { status.textContent = "Open this page with `agentic serve`."; return; }
  const r = await fetch("/api/v1/session", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code }),
  });
  if (!r.ok) { status.textContent = "This link has expired. Run `agentic serve` again."; return; }
  const { csrf } = await r.json(); // held in page memory only
  const h = await fetch("/api/v1/health", { headers: { "x-am-csrf": csrf } });
  status.textContent = h.ok ? "Connected to the local server." : "Not connected.";
  const ws = new WebSocket(`ws://${location.host}/api/v1/events`, ["am.csrf." + csrf]);
  ws.onmessage = (e) => { status.textContent = "Connected. Event stream open: " + JSON.parse(e.data).type; };
})();
