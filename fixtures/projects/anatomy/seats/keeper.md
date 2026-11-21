---
schema_version: 2
name: Keeper
role: Writer
team: core
folders:
  - id: work
    path: work
    access: read
    include:
      - "**/*.txt"
    exclude:
      - "old/**"
text_fallback: true
---
Keep the notes in the work folder up to date.
