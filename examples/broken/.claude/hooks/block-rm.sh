#!/bin/bash
cmd=$(jq -r '.tool_input.command')
if echo "$cmd" | grep -q 'rm -rf'; then
  echo "Blocked: rm -rf" >&2
  exit 1
fi
exit 0
