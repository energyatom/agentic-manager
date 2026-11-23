// Spike page (E1 story 1; story 3.2 adds the pairing code): exchanges the
// launch code from the URL fragment, or the pairing code `agentic serve`
// prints in its terminal, for a session, then calls the API and opens the
// event stream. The React + TypeScript UI replaces it.
"use strict";
(async () => {
  const status = document.getElementById("status");
  const params = new URLSearchParams(location.hash.slice(1));
  const code = params.get("code");
  history.replaceState(null, "", location.pathname); // strip the fragment
  const exchange = (path, value) => fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code: value }),
  });
  let r;
  // Only a well-formed launch code (64 lowercase hex digits) is exchanged:
  // any failed exchange invalidates every outstanding code (AD-13), so a
  // page that sends the browser here with a made-up fragment gets the
  // pairing prompt instead.
  if (code !== null && /^[0-9a-f]{64}$/.test(code)) {
    r = await exchange("/api/v1/session", code);
  } else {
    const typed = window.prompt("Enter the pairing code `agentic serve` printed in its terminal.");
    if (!typed) { status.textContent = "Open this page with `agentic serve`, or reload it and enter its pairing code."; return; }
    r = await exchange("/api/v1/pair", typed.trim().toUpperCase());
  }
  if (!r.ok) {
    const e = await r.json().catch(() => ({}));
    status.textContent = (e.message || "Not signed in.") + (e.fix ? " " + e.fix : "");
    return;
  }
  const { csrf } = await r.json(); // held in page memory only
  const h = await fetch("/api/v1/health", { headers: { "x-am-csrf": csrf } });
  status.textContent = h.ok ? "Connected to the local server." : "Not connected.";
  const ws = new WebSocket(`ws://${location.host}/api/v1/events`, ["am.csrf." + csrf]);
  ws.onmessage = (e) => { status.textContent = "Connected. Last event: " + JSON.parse(e.data).kind; };
})();
