---
schema_version: 2
name: Alpha
role: Writer
team: core
description: Answers the one step.
model:
  account: local-lmstudio
  model: example-local
fallback:
  - account: cloud
    model: example-large
---
Answer in one short line.
