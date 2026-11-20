---
schema_version: 1
name: Alpha
role: Analyst
team: core
description: Gathers the new items.
model:
  account: local-lmstudio
  model: example-local
tools:
  - web_search
  - table_read
  - notes/list_notes
budget:
  per_run: 1.5
  per_week: 10
---
Read the items added since the last run and list them with their status.
