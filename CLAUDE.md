# Claude Code adapter

This file is the thin Claude-Code-specific adapter for `tastile-cli`. The
canonical contract for this workspace is `AGENTS.md`. Read it before
opening any tool.

Claude Code specific settings live in `.claude/settings.json`, hooks in
`.claude/hooks/`, and thin Skill adapters in `.claude/skills/`. Do not
duplicate project-wide rules here.

## Adapter-specific notes

- The TUI runs in the foreground; press `q` to quit, `r` to refresh.
- `tastile auth login` will spawn a real browser if `--print-url` is not
  passed. In a headless / sandbox environment, always pass `--print-url`.
- The keyring backend is platform-dependent. On Linux without
  `org.freedesktop.secrets`, `tastile doctor` reports the keyring
  unavailability but the CLI still proceeds (the keyring is only used for
  auth state, not for API requests).
