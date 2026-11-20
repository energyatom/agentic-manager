---
schema_version: 1
name: Beta
role: Reviewer
team: support
reports_to: alpha
model:
  account: local-ollama
  model: example-local
fallback:
  - account: cloud
    model: example-large
capabilities:
  tables:
    on: true
tools:
  - table_write
  - write_document
---
Write a short summary of the gathered items.
