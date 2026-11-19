# Agentic Manager — Glossary

Terms the initiative, its epics and the product strings use. From the PRD Glossary, the party record, the reference-company direction of 2026-11-18 and EXPERIENCE.md. Product chrome uses only these words; department nouns (in the sample company only abstract names: Document A, Table T1, Report R1) come from template data (RULE-2).

| Term | Meaning |
|---|---|
| Seat | An org-chart position with role, instructions, tools, budget and one occupant: an AI model or the owner. One person can hold several seats. An AI seat is shown as its Agent card. |
| Agent card | The seat sheet of an AI seat, in nine sections: Identity (name, role, goal, backstory, avatar), Brain (Account › Model, fallback, chips), Skills, Tools & connectors, Knowledge & inbox, Tasks, Memory notes, Budget, What this seat can touch (R58). |
| Skill | A folder in the open Agent Skills format (`SKILL.md` with name, description and instructions, plus optional text reference files) attached to one or more seats; it adds instructions only and never grants a tool, host or cap; no scripts in v1 (arch AD-38). |
| Hat | A human seat's role: CEO (sets goals injected into every seat below), Vision Creator (adds expiring guidance notes), Reviewer (answers gates), Developer (approves tool, model and prompt changes in its subtree), or custom. Written "You (Reviewer)" in history and provenance. |
| Team | Seats under a head seat; the company is the root team. |
| Department | A team with its own workflows, tables and documents; each department is a trust boundary that work crosses only through grants (read) and declared hand-offs. |
| Sample company | The optional, clearly labelled "Sample company: Company A (two departments)" (an Owner, D1 and D2) the wizard can load to explore; abstract and synthetic (reference-workload.md). |
| Workflow | Ordered steps owned by a team; each step names a seat (or, for a fan-out, several seats), inputs, expected output (optionally a schema) and a hand-off. A step may be assigned to a seat of another department only through a declared cross-department hand-off. |
| Hand-off | Next; loop-back (bounded return to an earlier step with reasons, default maximum 2); gate (wait for a human seat or a reviewing seat); fan-out; or fan-in. |
| Cross-department hand-off | A declared hand-off that passes a step's output (a document version or a snapshot of table rows) to a step or workflow owned by another department, which must accept it; its taint and provenance travel with it; adding one is privilege-widening (arch AD-41). |
| Fan-out | A step that gives the same input to several seats at once; each seat's work is a **branch**, labelled by seat, with its own budget reservation and cancellation (arch AD-40). |
| Fan-in | A consolidating step that waits for every branch, or for its quorum or timeout, then receives every finished branch's output labelled by seat; when a branch fails it fails the run, continues at quorum, or asks in the Inbox, as configured. |
| Review gate | A gate whose reviewer is a seat, AI or human, possibly in another department: approve, return with note (loop-back, bounded) or reject; the verdict is recorded with the actor and hat (arch AD-42). |
| Lineage | The contribution chain behind a document or answer: each seat, step, branch, model, input (with taint), output version, tokens, time and cost, derived from the run journal; opened from the Inbox and Documents and exportable as JSON or Markdown (arch AD-43). |
| Trigger chain | Runs started by event triggers one after another (a document updated, a step or workflow completed); bounded by a maximum depth and refused on a cycle (arch AD-44). |
| Trigger | A schedule (cron + timezone, with a catch-up policy), a manual start, or an event in the project (a document updated, a step completed, a workflow completed) that starts a workflow in any department. File-drop, webhook and email triggers are v1.x. |
| Run | One execution of a workflow. A dry-run substitutes cheap or local models and is labelled as such. |
| Table | Config-defined data owned by one team; agents write via tools with provenance, humans edit and comment. |
| Grant | A read-only permission for another team to read a table or document; runs read a snapshot taken at run start. |
| Assistant | Read-only chat over tables, runs, documents and config; answers with citations and makes proposals. |
| Proposal | A config diff (or "Run now?" card) the owner applies, edits or dismisses; privilege-widening proposals need a second confirmation; a Proposal raised by a step that read untrusted content is **tainted-origin** and never approvable: the owner may dismiss it or start an own edit from it (2026-11-19, arch AD-10). |
| Account | A provider connection: kind, base URL, keychain credential reference, friendly name, health, discovered models and pinned egress host. Local servers on loopback are Accounts badged "On this Mac". |
| Model profile | A model under an Account plus parameters (temperature, max output tokens, context window) and price. |
| Capability chip | Tools, Structured output, Context, Cost; each marked known (shipped metadata) or tested (Test this model probe passed). |
| Budget | A spend cap per seat or workflow, per run, week or month; reserved before each call, settled after. |
| Estimate | A pre-run cost range with its basis ("rough · no history", then "based on your last N runs", p50–p90). Never a single number. |
| Meter | Weekly company spend (settled plus reserved) against budget, beside the company name. |
| Inbox item | Review (gate), Approval (external effect), Draft, Question, Budget ask (breach), Failure, or Proposal. |
| Document | A versioned run output such as an advice note; every version reaching a human carries its lineage. |
| Drop folder | The watched folder of a Local Repository Source (a seat's inbox); each file or link list (links.txt, .url, .webloc, pasted URLs) is loaded once by content hash, then deleted (default), archived or kept only after the load committed. |
| Reference files | A seat's or team's persistent files of any type in `.agentic/knowledge/`, read on demand through Folder access or keyword-searched (text-readable files) as untrusted content and never deleted or moved by the app; together they are that seat's or team's Knowledge. |
| Saved links | A seat's titled URLs in its seat file; it may open them as written, within its allow-list; their pages are untrusted content. |
| Guidance note | Vision Creator direction on a team or seat, optionally expiring, injected into prompts below. |
| Memory note | Learned, owner-editable seat guidance; additions from the learning loop (shipped with the Coordinator role template) arrive as proposals citing their sources. |
| Tool | A built-in or MCP capability a model may call, classed read, local-write or external-effect, with a per-step use count. |
| Connector | A generic, typed building block for getting data in or out: Web Search, HTTP Reader, Crawler, API, Feed, Local Repository, Knowledge, Database, MCP or Actions. Never named after a department's source (arch AD-39). An HTTP Reader or Crawler may render pages built by scripts in a browser installed on the owner's computer (a mode of the connector, not a new entity; added 2026-11-18). |
| Source | A configured instance of a connector, owned by a seat: Account, scope, schedule, limits, output (a table, documents or knowledge) and incremental state. Department sources ship only as Source files in templates. |
| Load run | One run of a Source: no model, requests built only from its configuration; it acquires, extracts, maps declared fields, de-duplicates by hash and key, loads and, for a Local Repository, cleans up after the commit. |
| Source record | One item a load stored (page, feed or API item, file text or row) with its provenance; always untrusted content. |
| Ingest report | A load's counts: items seen, new, changed, unchanged, duplicates by hash and by key, rejected with reasons, files cleaned up, URLs skipped by robots or scope, spend and time. |
| Knowledge | A seat's or team's reference files, kept forever and searchable by keyword with the search-knowledge tool; semantic search is v1.x. |
| Untrusted content | Any web, document, MCP, Source record (rendered pages included), file read from a linked folder (2026-11-19), knowledge search result or free-text in table rows (such as a row's notes), including any of these carried by a hand-off; fenced, shown as plain text, never placed in instructions. |
| Stop everything | Toolbar button, ⌘. and `agentic stop`: cancels running steps at the next safe point and pauses all schedules. |
| Doctor | The health screen: accounts, keys, MCP servers, Sources, disk, schedules, search (Local search status), file reading (types a seat's model cannot read; untested file chips; 2026-11-19), blocked egress, keys in files. |
| Role template | A generic starting seat: Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist or Human seat; each brings instructions, goal, backstory, capabilities, default tools, budget and a Recommended model. The Coordinator template carries the learning loop. "Chief of Staff" and "Manager" are not template names (2026-11-18). |
| Head seat | The seat at the top of a team or department (D1-Head, D2-Head in the sample); reports to the Owner or to another head seat. |
| Ask a seat | A one-line field at the Inbox foot that turns a question into a Question thread addressed to any seat the owner picks; by default the top Coordinator seat, else the top head seat, and for an item from a department that department's head seat; the seat answers in a one-step run under its caps. |
| Capability | One switch on an Agent card that grants a group of built-in tools: Web search, Browse & read pages, Crawl, Folder access (named Documents & knowledge until 2026-11-19), Files workspace, Tables, Memory, Hand-off & ask, Calculate & analyse; off by default except Folder access; turning one on is privilege-widening; a view over tool grants, not a new concept (arch AD-46). |
| Provider | What serves a capability for a seat: Built-in (default, shipped in the install, no key) or an Account (a keyed search API, a crawl service, a model) the owner picks; its spend is metered (arch AD-46). |
| Local search | The built-in Web search provider: a pinned SearXNG bundled in the install, supervised by the app, reachable only by it, sending each query to the selected public search engines; free, no key (arch AD-45). |
| Open by default | RULE-20: every capability works with free, local, key-free components; paid or keyed services are optional upgrades, never required or pre-selected. |
| Trust gate | The release and process rules gating v0.9-rc and v1.0 (trust-gate.md). |
| Tainted-read rule | In a step that read untrusted content, read tools fetch only URLs recorded for the step, used as-is, and search uses only words from the step's instructions; anything else waits for approval (arch AD-11). |
| CLI class / UI class | Who may decide: the CLI token allows reads, stop and a few TTY decisions; applying, external-effect approval and privilege-widening need the app's session and a confirm token sent over its live WebSocket (arch AD-15). |
| Login item | `agentic service install`: the server starts when the owner logs in (a macOS LaunchAgent; v1 is macOS only, 2026-11-19), so schedules fire without a terminal; `agentic open` opens the app signed in. |
| Privilege-widening | Any change that lets a seat, department or bundled service touch more: caps raised, tools, hosts, MCP servers, Accounts, capabilities switched on or moved to an Account, hand-offs, `accepts`, cross-department triggers, grants, skill imports, the Local search engine list, or a human review gate replaced by an AI reviewer; decided by one classifier and confirmed in two steps (arch AD-15; 2026-11-19). |
| Team workspace | The v1.1 Team release's multi-user workspace with access scoped by group, over LAN with its own sign-in and TLS (renamed 2026-11-19 from the owner's original "rep workspace"). |
| App Sandbox helper | A separately signed helper program in the Mac app (the text extractor and Local search's Python) that macOS confines so it cannot read the app's data, projects or keychain (arch AD-13; 2026-11-19). |
| Folder access | The capability that links folders to a seat, read-only by default (write is a separate switch that widens access): the seat lists and filters their files and reads each one by attaching it to its model when the model reads that type, as plain text through the optional text fallback, or gets a clear refusal; files sent to a cloud model leave this Mac (arch AD-30; 2026-11-19). |
| Linked folder | A folder on the owner's Mac granted to a seat by Folder access; a grant root the seat cannot step outside of; never deleted from or reorganised by the app (2026-11-19). |
| File chip | A model capability chip saying which file types the model reads natively: reads PDF, images, office files or audio; known from shipped data or tested by Test this model (arch AD-31; 2026-11-19). |
| Text fallback | A per-seat switch, off by default, that gives a text-only model the plain text of text files and of a PDF's text layer; it never converts office files or images (2026-11-19). |
| Folder fan-out | A fan-out step over a linked folder whose file patterns route files to different seats, each with a model that reads that type, gathered by one seat at the fan-in (arch AD-40; 2026-11-19). |
