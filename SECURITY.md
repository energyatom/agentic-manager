# Security policy

Agentic Manager is a local-first app: its server binds 127.0.0.1 only, keys
live only in the macOS Keychain, and every network request goes through one
egress client with allow-lists.

## Reporting a vulnerability

Please report security issues privately through GitHub's "Report a
vulnerability" (private security advisory) on this repository, not in a public
issue. Include the version or commit, the steps to reproduce and the impact.
We aim to acknowledge a report within 7 days.

## Supported versions

Pre-release (v0.x): only the latest commit on `main` is supported.
