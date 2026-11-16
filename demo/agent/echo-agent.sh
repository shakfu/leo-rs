#!/bin/sh
# A stand-in agent for demo/agent: it replies with what it was sent, with no
# network and no cost. leo-rs writes the prompt to its stdin and reads the
# reply from its stdout.
#
#   $1           the model, when the node asks for one (echo.model = {model})
#   AGENT_DELAY  seconds to wait first, to try waiting, editing and cancelling
#   AGENT_FAIL   set to exit 3 with a message on stderr

sleep "${AGENT_DELAY:-0}"
if [ -n "$AGENT_FAIL" ]; then
    echo "echo-agent: asked to fail" >&2
    exit 3
fi
prompt=$(cat)
echo "echo-agent (model: ${1:-none}) got $(printf '%s\n' "$prompt" | wc -l | tr -d ' ') lines:"
printf '%s\n' "$prompt" | sed 's/^/> /'
