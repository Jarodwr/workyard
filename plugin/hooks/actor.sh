#!/bin/sh
# Give this session a stable Workyard actor unless the orchestrator already set one.
# Ownership is enforced against this string, so it must not change between commands.
[ -n "$CLAUDE_ENV_FILE" ] || exit 0
[ -n "$WORKYARD_ACTOR" ] && exit 0
id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["session_id"][:8])' 2>/dev/null)
[ -n "$id" ] && echo "export WORKYARD_ACTOR=agent:claude-$id" >> "$CLAUDE_ENV_FILE"
exit 0
