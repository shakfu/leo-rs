#!/bin/sh
# A stand-in for an agent that edits the outline, for demo/agent. Over the
# MCP server leo-rs starts for the run, it adds one child per word of its
# prompt to the prompt's node, then tries to add a node beside that node.
# It reaches only 127.0.0.1, and costs nothing. Needs curl.
#
#   $1  the MCP config file leo-rs wrote (outliner = sh outline-agent.sh {mcp})

if ! command -v curl >/dev/null; then
    echo "outline-agent: needs curl" >&2
    exit 2
fi
config=$(cat "$1")
url=$(printf '%s' "$config" | sed -n 's/.*"url":"\([^"]*\)".*/\1/p')
auth=$(printf '%s' "$config" | sed -n 's/.*"Authorization":"\([^"]*\)".*/\1/p')
prompt=$(cat)
# leo-rs ends the prompt with the gnx of the prompt's node.
gnx=$(printf '%s\n' "$prompt" | sed -n 's/.*from the node with gnx \([^ ]*\).*/\1/p' | sed 's/\.$//')
words=$(printf '%s\n' "$prompt" | head -n 1 | tr -cs 'A-Za-z0-9' ' ')

# tool NAME ARGUMENTS: call a tool; prints the JSON-RPC reply.
tool() {
    curl -s --max-time 30 -H "Authorization: $auth" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}" \
        "$url"
}

added=""
for w in $words; do
    reply=$(tool insert_node "{\"gnx\":\"$gnx\",\"place\":\"inside\",\"headline\":\"$w\"}")
    case $reply in *'"isError":false'*) added="$added $w" ;; esac
done
reply=$(tool insert_node "{\"gnx\":\"$gnx\",\"place\":\"after\",\"headline\":\"Added beside its node\"}")
case $reply in
    *'"isError":false'*) beside="added" ;;
    *) beside="refused: $(printf '%s' "$reply" | sed -n 's/.*"text":"\([^"]*\)".*/\1/p')" ;;
esac
echo "outline-agent added children:$added"
echo "beside its node: $beside"
