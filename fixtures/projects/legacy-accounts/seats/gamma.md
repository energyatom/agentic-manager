---
schema_version: 2
name: Gamma
role: Reviewer
team: core
description: Reviews the drafts.
model:
  account: same
  model: example-large
fallback:
  - account: clash
    model: example-large
---
Review the draft.
