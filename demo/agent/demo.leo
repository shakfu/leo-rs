<?xml version="1.0" encoding="utf-8"?>
<!-- Created by Leo: https://leo-editor.github.io/leo-editor/leo_toc.html -->
<leo_file xmlns:leo="https://leo-editor.github.io/leo-editor/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<globals/>
<preferences/>
<find_panel_settings/>
<vnodes>
<v t="demo.20261116120000.1"><vh>About this demo</vh></v>
<v t="demo.20261116120000.2"><vh>1. A reply below the cursor</vh></v>
<v t="demo.20261116120000.3"><vh>2. An empty body</vh>
<v t="demo.20261116120000.4"><vh>What to do</vh></v>
</v>
<v t="demo.20261116120000.5"><vh>3. Scope: outline text with the prompt</vh>
<v t="demo.20261116120000.6"><vh>Quicksort</vh></v>
<v t="demo.20261116120000.7"><vh>Merge sort</vh></v>
<v t="demo.20261116120000.8"><vh>Heapsort</vh></v>
</v>
<v t="demo.20261116120000.9"><vh>4. Models</vh></v>
<v t="demo.20261116120000.10"><vh>5. Waiting while you type</vh></v>
<v t="demo.20261116120000.11"><vh>6. A body edited during the run</vh></v>
<v t="demo.20261116120000.12"><vh>7. Cancel and timeout</vh></v>
<v t="demo.20261116120000.13"><vh>8. Failures and refusals</vh></v>
<v t="demo.20261116120000.14"><vh>9. A prompt that looks like a parameter</vh></v>
<v t="demo.20261116120000.15"><vh>10. A real agent</vh></v>
<v t="demo.20261116120000.16"><vh>11. Selected lines</vh></v>
<v t="demo.20261116120000.17"><vh>12. @agent nodes</vh>
<v t="demo.20261116120000.18"><vh>@agent agent=echo list three sort algorithms</vh></v>
<v t="demo.20261116120000.19"><vh>@agent agent=echo scope=1 reply=child compare the notes in this section</vh></v>
</v>
<v t="demo.20261116120000.20"><vh>13. @agent in the body</vh>
<v t="demo.20261116120000.21"><vh>Sorting notes</vh></v>
<v t="demo.20261116120000.22"><vh>Top 5 sort algorithms</vh></v>
</v>
<v t="demo.20261116120000.23"><vh>14. An agent that edits the outline</vh>
<v t="demo.20261116120000.24"><vh>@agent agent=outliner plan write test</vh></v>
</v>
</vnodes>
<tnodes>
<t tx="demo.20261116120000.1">Try :ask, which sends a prompt to an agent and inserts the reply
below the body cursor's line. Each node below is one thing to try; its
body says what to type and what should happen.

Setup:
- Build with the plugin: make agent-demo (GUI=1 for leogui)
- Add the lines of agents.toml, beside this file, to ~/.config/leo-rs/agents.toml.
  They define `echo`, an offline stand-in (echo-agent.sh) that replies with
  what it was sent, at no cost.
- The exercises use agent=echo. Drop it to ask claude, the default agent,
  which needs `claude` on PATH and costs money.

In every exercise: move to the body, put the cursor on a line, type the
command, and press Enter. u undoes a reply.
</t>
<t tx="demo.20261116120000.10">The `slow` provider makes the stand-in wait 5 seconds. Type:

    :ask agent=echo provider=slow hi

then press i and type something. The reply waits: the status line says
it goes in once you are in NORMAL. Press Escape, and it is inserted.
</t>
<t tx="demo.20261116120000.11">Type:

    :ask agent=echo provider=slow hi

and, within 5 seconds, change this body: add a line, say. The reply does
not go in, as the line it named may have moved. The status line says so,
and p puts the reply from the register.
</t>
<t tx="demo.20261116120000.12">Type:

    :ask agent=echo provider=slow hi
    :ask agent=echo hi

The second is refused while the first runs. Then:

    :agent-cancel

stops the first; nothing is inserted.

For the timeout, set `timeout = 2` in agents.toml and run the slow one
again: it is stopped after 2 seconds. The next :ask reads the edited file.
</t>
<t tx="demo.20261116120000.13">Each of these says why on the status line, and inserts nothing:

    :ask agent=echo provider=broken hi
        the agent fails: exit 3, with its stderr line

    :ask agent=echo provider=keyed hi
        LEO_DEMO_TOKEN is not set (export it, restart, and it runs)

    :ask agent=nobody hi
        no such agent; the message lists the ones agents.toml defines

    :ask agent=echo scoep=1 hi
        no such parameter

    :ask agent=echo reply=replace hi
        replace needs a selection: lines chosen in VISUAL

    :ask
        the usage line

And from the outline pane, not the body:

    :ask agent=echo hi
        refused: the reply goes below the body cursor
</t>
<t tx="demo.20261116120000.14">-- ends the parameters:

    :ask agent=echo -- scope=all is a phrase in this prompt

sends `scope=all is a phrase in this prompt` with scope 0.
</t>
<t tx="demo.20261116120000.15">Drop agent=echo to use claude:

    :ask list three sort algorithms
    :ask model=haiku scope=1 summarise this demo in two lines

To reach Claude through OpenRouter, add to agents.toml:

    claude.openrouter.env = ANTHROPIC_BASE_URL=https://openrouter.ai/api ANTHROPIC_AUTH_TOKEN=${OPENROUTER_API_KEY} ANTHROPIC_API_KEY=

export OPENROUTER_API_KEY, restart, and type:

    :ask provider=openrouter model=anthropic/claude-sonnet-4.5 hi
</t>
<t tx="demo.20261116120000.16">From VISUAL, :ask sends the selected lines with the prompt.

Put the cursor on the first of the three lines below, press V, then j
twice, and type:

    :ask agent=echo number these

The `:` line opens empty; it acts on the selection all the same. The
reply goes below the third line, quoting the three after the prompt.

apples
pears
plums

Then select the three again, and type:

    :ask agent=echo reply=replace number these

The reply takes their place. u puts them back.

lines=all acts on the whole body, as if it were all selected:

    :ask agent=echo lines=all count these
</t>
<t tx="demo.20261116120000.17">Each child here is an @agent node: its headline is the prompt, and its
body holds the replies. Select one, in the outline or in its body, and
type:

    :agent-run

The first run in this outline asks first: y sends it, n does not.

The first child appends each reply to its body, under a separator line,
`-- TIME echo --`, with the time in UTC. Run it twice: the replies stack.
u takes the last one out.

The second child sends this node and its children (scope=1), so it asks
every time. Each reply becomes a new child of it, headlined with the time
and the prompt. Its own body and children are never sent.

Then add reply=replace to the first child's headline, after @agent, and
run it twice: the body holds only the last reply. Edit the body and run
it again: it refuses, as the reply would overwrite your edit.
</t>
<t tx="demo.20261116120000.18"></t>
<t tx="demo.20261116120000.19"></t>
<t tx="demo.20261116120000.2">Put the cursor on this line, then type:

    :ask agent=echo list three sort algorithms

The reply goes below this line, and the cursor moves to its first line.
u takes it out in one step.

This line stays below the reply.
</t>
<t tx="demo.20261116120000.20">The prompt can live in the body instead of the headline, so the
headline stays a title.

`Sorting notes` holds a block: an @agent line, the prompt below it, and
@agent-end. The prose above the block stays; replies go after it.

`Top 5 sort algorithms` holds one line, `@agent prompt=headline`: its
headline is the prompt. This turns any headline into a task.

Run either with :agent-run, a double-click in leogui, or Enter on its
@agent line in the body, which is drawn as a link.

A node holds one prompt: a second @agent line, or an @agent headline
with an @agent line in its body, refuses.
</t>
<t tx="demo.20261116120000.21">Merge sort is stable; heapsort is not.

@agent agent=echo reply=replace
Say which of these sorts are stable,
in one line each.
@agent-end
</t>
<t tx="demo.20261116120000.22">@agent agent=echo prompt=headline
</t>
<t tx="demo.20261116120000.23">An agent whose command in agents.toml names {mcp} may read and edit
the outline during its run, over an MCP server of its own. `outliner` is
an offline stand-in (outline-agent.sh, which needs curl). It adds one
child per word of its prompt, then tries to add a node beside its own.

Select the child below and type:

    :agent-run

It asks every time, and names how many nodes it may edit. Answer y.
- Three children appear: plan, write and test.
- The reply, at the end of the child's body, says the node beside it was
  refused: scope=0 keeps the agent to its own node and its subtree.
- u takes out the children and the reply in one step.

Then add scope=1 after @agent and run it again. The question counts
the nodes of this whole section, and the node beside it goes in.
</t>
<t tx="demo.20261116120000.24"></t>
<t tx="demo.20261116120000.3"></t>
<t tx="demo.20261116120000.4">Select the parent node, `2. An empty body`, go to its empty body, and type:

    :ask agent=echo hello

The reply becomes the body, with no empty first line.
</t>
<t tx="demo.20261116120000.5">scope=1 sends this node and its children with the prompt. Put the
cursor here and type:

    :ask agent=echo scope=1 compare these algorithms in a table

The reply quotes what was sent: the prompt, then each node as a `*`
headline with its body. :messages says how many nodes and bytes went.

Then try the other scopes, from inside a child:
- scope=0 (the default): the prompt only.
- scope=1 from a child: this node's subtree again, the child's parent.
- scope=2 from a child: the same, as this node is top-level and scope
  never goes above the top-level node.
- scope=all: the whole outline.
</t>
<t tx="demo.20261116120000.6">Average n log n, worst n^2. In place.
</t>
<t tx="demo.20261116120000.7">Always n log n. Needs n extra space. Stable.
</t>
<t tx="demo.20261116120000.8">Always n log n. In place. Not stable.
</t>
<t tx="demo.20261116120000.9">echo.models in agents.toml allows `small` and `large`. Type:

    :ask agent=echo model=small hi

The reply's first line names the model. Then:

    :ask agent=echo model=huge hi

refuses: huge is not in the list. And:

    :ask agent=echo model=--verbose hi

refuses: a model may not start with -, so it cannot add a flag.
</t>
</tnodes>
</leo_file>
