---
schema_version: 1
name: Keeper
role: Writer
team: core
folders:
  - id: work
    path: work
    access: read_write
    include:
      - "**/*.txt"
    exclude:
      - "old/**"
text_fallback: true
---
Keep the notes in the work folder up to date.
