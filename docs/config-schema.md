# Config schema

What `agentic validate` reads in a project folder, and what it accepts. Every
problem is printed as `file:line: message` with a `fix:` line; errors make the
command exit 1, warnings never do. Every YAML file (and a seat's frontmatter)
starts with `schema_version: 1`. Ids are kebab-case and equal the file or
folder name. Unknown keys are errors with a "did you mean" suggestion. YAML
comments are warnings, because the app does not keep them when it saves a
file: put notes in `description:` fields instead. Secret-looking values are
errors: keys live in the Keychain and a file names an Account.

This page covers the seat anatomy, skills, Accounts and Sources (story 1.12).
The other files (`company.yaml`, `tools.yaml`, `models/prices.yaml`,
`models/capabilities.yaml`, workflows, tables, schemas, dashboards) are read
as the field list in the story 1.4 plan records.

## The project id and the runtime folder (story 1.13)

`company.yaml` may hold `project_id: <id>` directly after `schema_version`:
the project's id (a lowercase UUIDv7). You never write it yourself: the app
inserts that one line on the project's first open and leaves every other byte
of the file as it was. The project's runtime state (its database, lock,
backups, logs and reference files) lives outside the project, in
`~/Library/Application Support/agentic-manager-projects/<project-id>/`; the
only runtime file in the project is `.agentic-open`, which says which Mac has
it open and which a project's `.gitignore` should list. A `project_id` that
is not a project id is an error. A template or fixture carries none:
`agentic validate --template` (what `just check` runs over `templates/` and
`fixtures/`) refuses one, naming the line.

`agentic validate` on a project never writes an id. It checks reference files
(`knowledge`, below) against the project's runtime folder on this Mac, and
skips that check when the project has none here yet. With `--template` it
checks them against the folder's `runtime-seed/knowledge/<seat-id>/` (or
`runtime-seed/knowledge/team-<team-id>/`), where templates ship their seed
files.

## Accounts: `models/accounts.yaml`

```yaml
schema_version: 1
accounts:
  - id: search
    provider: tavily
    secret_ref: search
  - id: crawl
    provider: firecrawl
    base_url: https://crawl.example.org
```

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | kebab-case; `local-lmstudio` and `local-ollama` are reserved and always exist |
| `provider` | yes | decides the Account's kind (below) |
| `name`, `description` | no | text |
| `base_url` | for `openai-compatible`, `searxng-remote`, `firecrawl` | `https://…`, or `http://` only to `127.0.0.1` or `localhost`; never a user name or password |
| `secret_ref` | no | the Keychain entry that holds the key; never the key |

| Kind | Providers |
|---|---|
| model | `anthropic`, `openai`, `openai-compatible` |
| search | `brave`, `tavily`, `exa`, `serper`, `searxng-remote` |
| crawl service | `firecrawl` (any Firecrawl-compatible endpoint) |

A seat's `model` and `fallback` name model Accounts; `tools.yaml`
`search.provider` is `builtin` or a search Account.

## Seats: `seats/<id>.md`

YAML frontmatter between two `---` lines, then the seat's instructions in
Markdown.

```yaml
---
schema_version: 1
name: Example
role: Researcher
team: core
goal: Keep the reading list current.
backstory: Has followed the field for years.
avatar:
  tint: sage
model:
  account: local-lmstudio
  model: example-local
capabilities:
  web_search:
    on: true
    provider: builtin
  tables:
    on: true
folders:
  - id: work
    path: work
    access: read_write
    include: ["**/*.txt"]
text_fallback: true
skills: [brief]
knowledge:
  - file: guide.md
    note: How the list is laid out.
links:
  - title: Home
    url: https://example.org/
    group: Sites
tools: [write_document]
---
Find new items and add them to the list.
```

| Field | Required | Meaning |
|---|---|---|
| `name`, `role` | yes | text |
| `team` | yes | a team id from `company.yaml` |
| `description` | no | text |
| `reports_to` | no | a seat id; no loops |
| `goal` | no | plain text, at most 500 characters |
| `backstory` | no | plain text, at most 2,000 characters |
| `avatar.tint` | no | `slate`, `steel`, `sage`, `stone`, `plum` or `clay` |
| `model`, `fallback` | no | `{ account, model }`; the account is a model Account |
| `capabilities` | no | below |
| `folders` | no | linked folders, below |
| `text_fallback` | no | `true` serves text-readable files (txt, md, csv, tsv, json, a PDF's text) as text to a model that cannot read them natively; default `false` |
| `skills` | no | ids of folders under `skills/` |
| `knowledge` | no | `{ file, note? }`: reference files in the runtime folder's `knowledge/<seat-id>/` (a template's `runtime-seed/knowledge/<seat-id>/`); `file` is relative, without `..`; a file missing there is a warning, because the runtime folder does not travel with the project |
| `links` | no | `{ title, url, group? }`: `url` is http or https; a host not on the seat's allow-list is a warning |
| `tools` | no | built-in tools of no capability (`write_document`, `draft_message`, `propose_change`) and MCP tools as `<server-id>/<tool-name>`; `read_skill_file` comes with an attached skill and is an error when listed without one |
| `budget` | no | `{ per_run, per_week }` in the company currency |

### Capabilities

Each entry is `{ on: true | false, provider: builtin | <account-id> }`;
`on` is required in an entry and `provider` defaults to `builtin`. A
capability the file does not mention takes its default: `folder_access` is on,
every other one is off. The old key `documents` is read as `folder_access`;
writing both is an error.

| Capability | Grants | Provider |
|---|---|---|
| `web_search` | `web_search` | `builtin` (Local search) or a search Account |
| `browse` | `read_website`, `read_url`, `arxiv` | `builtin` or a crawl-service Account |
| `crawl` | no tool; the seat may own crawler Sources | `builtin` or a crawl-service Account |
| `folder_access` | `list_files`, `read_file`, `search_knowledge`; `write_file` only while a folder is `read_write` | `builtin` |
| `files` | `workspace_read`, `workspace_write` | `builtin` |
| `tables` | `table_read`, `table_write` | `builtin` |
| `memory` | `remember` | `builtin` |
| `handoff_ask` | `ask_seat` | `builtin` |
| `calculate` | `calculate`, `stats`, `query_table` | `builtin` |

A seat's grants are this expansion, plus `read_skill_file` when it has a
skill attached, plus the built-in tools of no capability and the MCP tools
it lists in `tools:`. Listing a built-in tool whose capability is off is an
error ("switch on browse"), as is `write_file` without a `read_write` folder,
and a provider Account of the wrong kind. A skill never adds a grant.

The hosts a seat may reach are the project's `tools.yaml` `egress_hosts`
(the same list for every seat, for now).

### Linked folders

`{ id, path, access?, include?, exclude? }`. `id` is kebab-case and unique in
the seat. `access` is `read` (default) or `read_write`. `include` and
`exclude` are path globs (`*` within a folder, `**` across folders) and must
compile.

`path` is relative to the project folder, or to your home folder when it
starts with `~/`; it is resolved by its text, without following links.
Refused: an absolute path, `/`, `~` itself, `~/Library` or anything under it
(the app's data folder and the runtime folders live there), any path with a
`.agentic` part (in any letter case: an older app's in-project runtime
folder), the project folder itself or any folder that holds it, and a path
whose `..` steps lead to one of these. A path that does not exist yet is a warning; one that
names a file is an error.

## Skills: `skills/<id>/`

An Agent Skills folder: instructions, never access.

```text
skills/brief/
  SKILL.md
  references/
    style.md
```

`SKILL.md` frontmatter (no `schema_version`):

| Field | Required | Meaning |
|---|---|---|
| `name` | yes | equal to the folder name |
| `description` | yes | text |
| `license` | no | text |
| `metadata` | no | `key: value` text pairs |
| `allowed-tools` | no | a list, or words separated by spaces or commas; kept as "asks for" only and never granted |

The body after the frontmatter is at most 64 KB. `references/` holds at
most 20 files, each `md`, `txt`, `csv`, `json`, `yaml` or `yml` and at most
256 KB, with no folders inside. Anything else in a skill folder is an error:
a `scripts/` folder, any other file or folder (hidden ones included; only
`.DS_Store` is skipped), a link, or a file with an executable bit. Every skill file counts in the snapshot hash and is checked
for secret-looking values (yaml, yml and json references by key name too);
SKILL.md's frontmatter and yaml references are checked for comments.

## Sources: `sources/<id>.yaml`

A configured connector instance.

```yaml
schema_version: 1
name: Documentation pages
type: crawler
owner_seat: example
account: crawl
scope:
  seeds: [https://example.org/docs/]
  include: ["/docs/**"]
  exclude: ["/docs/old/**"]
  allowed_hosts: [example.org]
  max_depth: 2
trigger:
  schedule:
    cron: "0 6 * * 1"
limits:
  max_items_per_load: 100
  max_bytes_per_load: 10000000
  max_loads_per_day: 4
  max_spend_per_load: 0.5
  rate_per_host: 2
output:
  table: pages
  key: ref
  mapping:
    ref: url
    title: title
render: true
terms_checked: 2026-11-01
```

| Field | Required | Meaning |
|---|---|---|
| `type` | yes | `web-search`, `http-reader`, `crawler`, `api`, `feed`, `local-repository`, `knowledge`, `database`, `mcp` or `action` |
| `owner_seat` | yes | an existing seat; its allow-list, budget and Agent card apply; a crawler's owner must have `crawl` on |
| `account` | no | crawler and http-reader take a crawl-service Account, web-search a search Account, api any Account, the other types none; no type needs one |
| `name`, `description` | no | text |
| `scope.seeds`, `scope.url` | no | http or https URLs |
| `scope.include`, `scope.exclude` | no | path globs that must compile |
| `scope.allowed_hosts` | no | host names |
| `scope.max_depth` | no | 0 to 10 |
| `trigger` | no | one trigger in the workflow shape: `schedule: { cron, timezone?, catch_up? }` or `event: { on, only_if? }` |
| `limits.max_items_per_load` | no | 1 to 10,000 |
| `limits.max_bytes_per_load` | no | 1 byte to 1 GB (1,000,000,000 bytes) |
| `limits.max_loads_per_day` | no | 1 to 96 |
| `limits.max_spend_per_load` | no | from 0, in the company currency |
| `limits.rate_per_host` | no | seconds between requests to one host, at least 1 |
| `output` | yes | exactly one of `table` (with `key` and `mapping: { <column>: <source field> }`, every column in that table), `records: true`, or `knowledge: seat` / `team` |
| `render` | no | `true` only on http-reader and crawler |
| `terms_checked` | website types | the date (`YYYY-MM-DD`) you checked that the site's terms allow automated access; on http-reader, crawler and feed Sources a missing or empty one is a warning |

A scope host that is not on the owner seat's allow-list is a warning: the
app asks before it adds a host.

## Workflow steps

Besides `id`, `seat`, `instruction`, `description`, `output_schema` and
`max_tokens`, a step may carry the run-graph keys `seats`, `join`,
`over_folder`, `routes`, `files`, `handoff`, `review` and `max_parallel`.
They are accepted and not yet checked; a step with `seats` or `over_folder`
may leave out `seat`.

## Templates

Every template under `templates/` is a minimal project, so `agentic validate
--template` checks it as it is (`just check` runs it over each one); it
carries no `project_id`, and its seed reference files live under
`runtime-seed/`, never `.agentic/`. The Blank department and the sample
company ship a `.gitignore` that lists `.agentic-open`. Installing a template
copies only part of it into the owner's project.

| Template | Where | What installing it copies |
|---|---|---|
| Blank department | `templates/departments/blank/` | its one team from `company.yaml` (no seats, tables or workflows) |
| Role templates: Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist, Human seat | `templates/roles/<role>/` (the Human seat is `human/`) | `seats/<role>.md` into the chosen team; for the Coordinator also `workflows/learning.yaml`, its learning loop, which has no trigger and runs only when started by hand |
| Sample company (two departments) | `templates/samples/<sample>/` | the whole project: `company.yaml`, `models/accounts.yaml`, `tools.yaml`, seats, tables, workflows, skills, Sources, the synthetic files under `files/`; the team Knowledge notes under `runtime-seed/knowledge/team-<team-id>/` are copied into the new project's runtime folder, never into the project |

Each role template's seat carries its instructions, a `goal` and `backstory`,
an avatar tint, default `tools` and `budget`, its `capabilities` (Folder
access on, plus what the role needs), a local `model` with an ordered
`fallback`, and empty `skills`, `knowledge` and `links`. A seat with no
`model` is a human seat. The wrapper `company.yaml` of a role template only
lets it validate: its team id, time zone and currency are not copied, and the
seat's budget is read in the installing company's currency.
