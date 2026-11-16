#!/bin/sh
# Session setup for Workyard workers: make sure `workyard` is on PATH, and give the session a
# stable actor unless the orchestrator already set one. Ownership is enforced against the
# actor string, so it must not change between commands.
[ -n "$CLAUDE_ENV_FILE" ] || exit 0
# The desktop app and the orchestrator set WORKYARD_BIN (a directory holding the binary) when
# it is not installed on PATH.
if ! command -v workyard >/dev/null 2>&1 && [ -n "$WORKYARD_BIN" ]; then
  echo "export PATH=\"$WORKYARD_BIN:\$PATH\"" >> "$CLAUDE_ENV_FILE"
fi
[ -n "$WORKYARD_ACTOR" ] && exit 0
id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["session_id"][:8])' 2>/dev/null)
[ -n "$id" ] && echo "export WORKYARD_ACTOR=agent:claude-$id" >> "$CLAUDE_ENV_FILE"
exit 0
