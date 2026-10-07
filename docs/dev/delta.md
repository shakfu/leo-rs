# leo-editor and leotui: the delta

What leo-editor has that leotui does not, and the other way round. This is a record, not a plan: an entry here is not a commitment to build it. `TODO.md` holds what is agreed. Deliberate differences in behaviour are in `porting-notes.md` and are not repeated here.

## Method

- Measured 2026-09-28, against leo-editor `e3b3841f64` (2026-09-15) and leo-rs `dcf413a` with that day's uncommitted work.

- A Leo command is a function decorated with `@g.command`, `@g.commander_command` or `@cmd` in `leo/core` or `leo/commands`. Names stacked on one function are one command with aliases: 653 commands, 721 names.

- Not counted: the 11 camelCase compatibility names from `g.command_alias`, names registered at run time (`@command` and `@button` nodes, chapters), and plugins, which have their own section.

- leotui's commands are `COMMANDS` in `commands.rs`, the names `app/ex.rs` handles, and `ALIASES` in `minibuffer.rs`.

- "Same name" is a Leo name leotui defines. "Equivalent" is leotui doing the job under another name or key, judged by hand. A same name can still behave differently; see the section of that name.

- To regenerate: walk `leo/core` and `leo/commands` with Python's `ast`, collect each function's decorator names and the first sentence of its docstring, and set them against the names above.

## Summary

Commands, by area:

| area | Leo commands | same name | equivalent | not implemented |
|-|-|-|-|-|
| Outline | 124 | 60 | 3 | 61 |
| Files | 75 | 11 | 5 | 59 |
| Find | 44 | 6 | 7 | 31 |
| Body editing | 200 | 3 | 93 | 104 |
| Help, keys and settings | 57 | 2 | 4 | 51 |
| Scripting and external tools | 88 | 0 | 1 | 87 |
| The application and its GUI | 65 | 3 | 11 | 51 |
| total | 653 | 85 | 124 | 444 |

Directives and node kinds are not commands, and are counted apart. A body directive is an `@word` line in a body; a node kind is an `@word` headline Leo gives meaning to.

| kind | in Leo | full | partly | recognised only | no behaviour in Leo |
|-|-|-|-|-|-|
| body directives (`globalDirectiveList`) | 36 | 21 | 2 | 6 | 7 |

| kind | in Leo | as in Leo | partly | not |
|-|-|-|-|-|
| `@<file>` kinds | 19 | 16 | 0 | 3 (`@auto-rst`, `@jupytext`, `@shadow`) |
| other headline kinds | 10 | 0 | 0 | 10 |

leolib recognises every body directive Leo does, so a sentinel file writes each as a directive and never as text. "Full" means the directive also has its effect. Scripting, plugins and the settings tree are covered under the last section.

## Outline

Tree structure, navigation, folding, marks, clones, and node data.

124 commands, in `commanderOutlineCommands.py`, `editCommands.py`, `leoUndo.py`. 60 by the same name, 3 by an equivalent, 61 not at all.

Same name: `redo`, `undo`, `copy-node`, `cut-node`, `paste-node`, `contract-all`, `contract-all-other-nodes`, `contract-node`, `contract-or-go-left`, `expand-all`, `expand-to-level-1`, `expand-to-level-2`, `expand-to-level-3`, `expand-to-level-4`, `expand-to-level-5`, `expand-to-level-6`, `expand-to-level-7`, `expand-to-level-8`, `expand-to-level-9`, `expand-next-level`, `expand-node`, `expand-and-go-right`, `expand-prev-level`, `go-forward`, `go-back`, `goto-first-visible-node`, `goto-last-visible-node`, `goto-next-clone`, `goto-next-marked`, `goto-next-sibling`, `goto-parent`, `goto-prev-marked`, `goto-prev-sibling`, `goto-prev-visible`, `goto-next-visible`, `dehoist`, `clear-all-hoists`, `hoist`, `clone-node`, `delete-node`, `insert-child`, `insert-node`, `insert-node-before`, `clone-marked-nodes`, `copy-marked-nodes`, `delete-marked-nodes`, `mark`, `mark-subheads`, `unmark-all`, `demote`, `move-outline-down`, `move-outline-left`, `move-outline-right`, `move-outline-up`, `promote`, `sort-children`, `sort-siblings`, `mark-node-and-parents`, `unmark-node-and-parents`, `move-marked-nodes`.

| command | Leo's summary | leotui |
|-|-|-|
| `copy-node-as-json` | Copy the selected outline as JSON to the clipboard. |  |
| `paste-retaining-clones` | Paste an outline into the present outline from the clipboard. |  |
| `paste-as-template` | Paste as template clones only nodes that were already clones. |  |
| `dump-outline` | Dump all nodes in the outline. |  |
| `contract-all-subheads` | Contract all children of the presently selected node. |  |
| `contract-parent` | Contract the parent of the presently selected node. |  |
| `expand-all-subheads` | Expand all children of the presently selected node. |  |
| `expand-or-go-right` | Simulate the Right Arrow Key in folder of Windows Explorer. |  |
| `expand-ancestors-only` | Contract all nodes except ancestors of the selected node. |  |
| `check-outline` | Do a full check of the consistency of a .leo file. |  |
| `find-next-clone` | Select the next cloned node. |  |
| `goto-first-node` | Select the first node of the entire outline. | `gg`, `goto-first-visible-node` |
| `goto-first-sibling` | Select the first sibling of the selected node. |  |
| `goto-last-node` | Select the last node in the entire tree. |  |
| `goto-last-sibling` | Select the last sibling of the selected node. |  |
| `goto-next-changed` | Select the node that is marked as changed. |  |
| `goto-prev-node` | Select the node preceding the selected node in outline order. |  |
| `goto-next-node` | Select the node following the selected node in outline order. |  |
| `clone-to-at-spot` | Create a clone of the selected node and move it to the last @spot node of the outline. |  |
| `clone-node-to-last-node` | Clone the selected node and move it to the last node. |  |
| `insert-as-first-child` | Insert a node as the first child of the previous node. |  |
| `insert-as-last-child` | Insert a node as the last child of the previous node. |  |
| `mark-changed-items` / `mark-changed-nodes` | Mark all nodes that have been changed. |  |
| `move-outline-to-first-child` | Move the selected node so that it is the first child of its parent. |  |
| `move-outline-to-last-child` | Move the selected node so that it is the last child of its parent. |  |
| `toggle-sparse-move` | Toggle whether moves collapse the outline. |  |
| `reverse-sort-children` | Sort the children of a node in reverse order. |  |
| `reverse-sort-siblings` | Sort the siblings of a node in reverse order. |  |
| `count-children` | Print out the number of children for the currently selected node. |  |
| `promote-section-definition` | c.p must be a section definition node and an ancestor must contain a reference. |  |
| `merge-node-with-next-node` | Merge p.b into p.next().b and delete p, *provided* that p has no children. |  |
| `merge-node-with-prev-node` | Merge p.b into p.back().b and delete p, *provided* that p has no children. |  |
| `promote-bodies` | Copy the body text of all descendants to the parent's body text. |  |
| `promote-headlines` | Copy the headlines of all descendants to the parent's body text. |  |
| `show-clone-ancestors` | Display links to all ancestor nodes of the node c.p. |  |
| `show-node-files` | Display the headlines of all @<file> nodes containing this node. |  |
| `show-clone-parents` | Display links to all parent nodes of the node c.p. |  |
| `clear-all-caches` / `clear-cache` | Clear all of Leo's file caches. |  |
| `dump-caches` | Dump, all of Leo's file caches. |  |
| `insert-headline-time` | Insert a date/time stamp in the headline of the selected node. |  |
| `capitalize-headline` | Capitalize all words in the headline of the selected node. |  |
| `goto-any-clone` | Select then next cloned node, regardless of whether c.p is a clone. |  |
| `hn-add-all` / `headline-number-add-all` / `add-all-headline-numbers` | Add headline numbers to all nodes of the outline *except*: - @<file> nodes and their descendants. |  |
| `hn-add-subtree` / `headline-number-add-subtree` / `add-subtree-headline-numbers` | Add headline numbers to *all* children of c.p, *including*: - @<file> nodes and their descendants. |  |
| `hn-delete-all` / `headline-number-delete-all` / `delete-all-headline-numbers` | Delete all headline numbers in the entire outline. |  |
| `hn-delete-subtree` / `headline-number-delete-subtree` / `delete-subtree-headline-numbers` | Delete all headline numbers in c.p's subtree. |  |
| `delete-all-icons` | Delete all icons in the outline. |  |
| `delete-first-icon` | Delete the first icon in the selected node's icon list. |  |
| `delete-last-icon` | Delete the first icon in the selected node's icon list. |  |
| `delete-node-icons` | Delete all of the selected node's icons. |  |
| `insert-icon` | Prompt for an icon, and insert it into the node's icon list. |  |
| `copy-gnx` / `gnx-show` / `show-gnx` | Copy c.p.gnx to the clipboard and display a gnx-oriented unl in the status area. |  |
| `move-lines-to-next-node` | Move one or *trailing* lines to the start of the next node. |  |
| `scroll-outline-down-line` | Scroll the outline pane down one line. |  |
| `scroll-outline-down-page` | Scroll the outline pane down one page. | `PageDown`, `Ctrl-f` in the outline |
| `scroll-outline-up-line` | Scroll the outline pane up one line. |  |
| `scroll-outline-up-page` | Scroll the outline pane up one page. | `PageUp`, `Ctrl-b` in the outline |
| `scroll-outline-left` | Scroll the outline left. |  |
| `scroll-outline-right` | Scroll the outline left. |  |
| `clear-node-uas` | Clear the uA's in the selected VNode. |  |
| `clear-all-uas` | Clear all uAs in the entire outline. |  |
| `show-all-uas` | Print all uA's in the outline. |  |
| `show-node-uas` | Print the uA's in the selected node. |  |
| `set-ua` | Prompt for the name and value of a uA, then set the uA in the present node. |  |

## Files

Reading and writing `.leo` and external files, imports, exports, and the file-line mapping.

75 commands, in `commanderFileCommands.py`, `editCommands.py`, `editFileCommands.py`, `gotoCommands.py`, `leoAtFile.py`, `leoFileCommands.py`, `leoImport.py`. 11 by the same name, 5 by an equivalent, 59 not at all.

Same name: `write-at-file-nodes`, `write-dirty-at-file-nodes`, `write-outline-only`, `refresh-from-disk`, `save`, `save-as`, `save-to`, `revert`, `read-at-file-nodes`, `goto-global-line`, `show-file-line`.

| command | Leo's summary | leotui |
|-|-|-|
| `check-external-file` | Make sure an external file written by Leo may be read properly. |  |
| `open-at-leo-file` | Open the outline given by the @leo node at c.p. |  |
| `write-at-auto-nodes` | Write all @auto nodes in the selected outline. |  |
| `write-dirty-at-auto-nodes` | Write all dirty @auto nodes in the selected outline. | `w`, which writes every dirty `@<file>` kind |
| `write-at-shadow-nodes` | Write all @shadow nodes in the selected outline. |  |
| `write-dirty-at-shadow-nodes` | Write all dirty @shadow nodes in the selected outline. |  |
| `dump-clone-parents` | Print the parent vnodes of all cloned vnodes. |  |
| `dump-gnx-dict` | Dump c.fileCommands.gnxDict. |  |
| `write-missing-at-file-nodes` | Write all @file nodes for which the corresponding external file does not exist. |  |
| `write-zip-archive` | Write a .zip file containing this .leo file and all external files. |  |
| `import-free-mind-files` | Prompt for free-mind files and import them. |  |
| `import-legacy-external-files` | Prompt for legacy external files and import them. |  |
| `import-mind-jet-files` | Prompt for mind-jet files and import them. |  |
| `import-MORE-files` | Prompt for MORE files and import them. |  |
| `import-tabbed-files` | Prompt for tabbed files and import them. |  |
| `import-todo-text-files` | Prompt for todo.txt files and import them. |  |
| `import-zim-folder` | Import a zim folder, http://zim-wiki.org/, as the last top-level node of the outline. |  |
| `parse-body` | Parse p.b as source code, creating a tree of descendant nodes. |  |
| `reload-settings` | Reload settings in all commanders, saving all existing opened files first. |  |
| `restart-leo` | Restart Leo, reloading all presently open outlines. |  |
| `close-window` | Close the Leo window, prompting to save it if it has been changed. | `:q` |
| `import-any-file` / `import-file` | Import one or more files. |  |
| `import-text-file` | Import the .txt file into a new node. |  |
| `open-second-view` | Open a second view of this outline. |  |
| `file-new` / `new` | Create a new Leo window. |  |
| `open-file` / `open-outline` | Open a Leo window containing the contents of a .leo file. | `:e path` |
| `pwd` | Print the current working directory. |  |
| `save-all` | Save all open tabs windows/tabs. |  |
| `file-save-as-leojs` / `save-file-as-leojs` | Save a copy of the Leo outline as a JSON (.leojs) file with a new file name. |  |
| `file-save-as-db` / `save-file-as-db` | Save a copy of the Leo outline as a SQLite (.db) file with a new file name. |  |
| `file-save-as-xml` / `save-file-as-xml` | Save a copy of the Leo outline as an XML .leo file with a new file name. | `:w path`, `save-to` |
| `save-node-as-xml` | Save a node with its subtree as an XML .leo outline file. |  |
| `export-headlines` | Export headlines for c.p and its subtree to an external file. |  |
| `flatten-outline` | Export the selected outline to an external file, flattened. |  |
| `flatten-outline-to-node` | Append the bodies of all descendants to a new last top-level node. |  |
| `outline-to-cweb` | Export the selected outline to a CWEB file. |  |
| `outline-to-noweb` | Export the selected outline to a noweb file. |  |
| `remove-sentinels` | Rewrite files without their sentinels. |  |
| `weave` | Simulate a literate-programming weave operation by writing the outline to a text file. |  |
| `read-at-auto-nodes` | Read all @auto nodes in the presently selected outline. | `:read-at-file-nodes`, which reads every `@<file>` kind |
| `read-at-shadow-nodes` | Read all @shadow nodes in the presently selected outline. |  |
| `read-file-into-node` | Read a file into a single node. |  |
| `write-file-from-node` | If node starts with @read-file-into-node, use the full path name in the headline. |  |
| `write-file-from-subtree` | Write the entire tree from the selected node as text to a file. |  |
| `clean-recent-files` | Remove items from the recent files list that no longer exist. |  |
| `clear-recent-files` | Clear the recent files list, then add the present file. |  |
| `edit-recent-files` | Opens recent files list in a new node for editing. |  |
| `sort-recent-files` | Sort the recent files list. |  |
| `write-edited-recent-files` | Write the edited recent-files node back as the recent-files list. |  |
| `open-theme-file` | Open a theme file in a new session and apply the theme. |  |
| `convert-at-root` | Convert @root to @clean throughout the outline. |  |
| `clean-at-clean-files` | Adjust whitespace in all @clean files. |  |
| `clean-at-clean-tree` | Clean whitespace in the nearest @clean tree. |  |
| `file-compare-two-leo-files` / `compare-two-leo-files` | Compare two files. |  |
| `file-delete` | Prompt for the name of a file and delete it. |  |
| `file-diff-files` | Creates a node and puts the diff between 2 files into it. |  |
| `git-diff` / `gd` | Produce a Leonine git diff. |  |
| `git-diff-pull-request` / `git-diff-pr` | Produce a Leonine diff of pull request in the current branch. |  |
| `clone-diff-pr` | Show the added, deleted and changed nodes (without diffs) of the given PR. |  |
| `file-insert` | Prompt for a file and insert its contents at the cursor. |  |
| `directory-make` | Prompt for the name of a directory and create it. |  |
| `directory-remove` | Prompt for the name of a directory and delete it. |  |
| `file-save-by-name` / `save-file-by-name` | Prompt for the name of a file and put the body text of the selected node into it. |  |
| `toggle-at-auto-at-edit` | Toggle between @auto and @edit, preserving insert point, etc. |  |

## Find

Leo's Find tab and its minibuffer commands. leotui has `/`, `?`, `n`, `N`, `:s`, `:bufdo`, `:cfa` and `:cff`; the pattern is always a regex with smartcase.

44 commands, in `leoFind.py`. 6 by the same name, 7 by an equivalent, 31 not at all.

Same name: `find-next`, `find-prev`, `clone-find-all`, `clone-find-all-flattened`, `search-backward`, `search-forward`.

| command | Leo's summary | leotui |
|-|-|-|
| `replace-then-find` / `change-then-find` | Handle the replace-then-find command. |  |
| `clone-find-all-marked` / `cfam` | clone-find-all-marked, aka cfam. |  |
| `clone-find-all-flattened-marked` / `cffm` | clone-find-all-flattened-marked, aka cffm. |  |
| `clone-find-parents` | Clone all parents of the selected clone under an organizer node. |  |
| `find-def` / `find-var` | Find the class, def or assignment to var of the word under the cursor. |  |
| `find-tab-hide` | Hide the Find tab. |  |
| `find-tab-open` | Open the Find tab in the log pane. |  |
| `focus-to-find` | Show the Find Tab and put the focus there or in the minibuffer. |  |
| `replace` / `change` | Replace the selected text with the replacement text. |  |
| `set-find-everywhere` | Set the 'Entire Outline' radio button in the Find tab. |  |
| `set-find-node-only` | Set the 'Node Only' radio button in the Find tab. |  |
| `set-find-file-only` | Set the 'File Only' radio button in the Find tab. |  |
| `set-find-suboutline-only` | Set the 'Suboutline Only' radio button in the Find tab. |  |
| `show-find-options` | Show the present find options in the status line. |  |
| `toggle-find-collapses-nodes` | Toggle the 'Collapse Nodes' checkbox in the find tab. |  |
| `toggle-find-ignore-case-option` | Toggle the 'Ignore Case' checkbox in the Find tab. |  |
| `toggle-find-mark-changes-option` | Toggle the 'Mark Changes' checkbox in the Find tab. |  |
| `toggle-find-mark-finds-option` | Toggle the 'Mark Finds' checkbox in the Find tab. |  |
| `toggle-find-regex-option` | Toggle the 'Regexp' checkbox in the Find tab. |  |
| `toggle-find-in-body-option` | Set the 'Search Body' checkbox in the Find tab. | `:set search=headlines` / `search=all` |
| `toggle-find-in-headline-option` | Toggle the 'Search Headline' checkbox in the Find tab. | `:set search=headlines` / `search=all` (a headline is always searched) |
| `toggle-find-word-option` | Toggle the 'Whole Word' checkbox in the Find tab. |  |
| `change-all` / `replace-all` | Replace all instances of the search string with the replacement string. | `:%s`, and `:bufdo %s` for every node |
| `clone-find-tag` / `find-clone-tag` / `cft` | clone-find-tag (aka find-clone-tag and cft). |  |
| `find-all` | Create a summary node describing all matches of the search string. |  |
| `find-source-for-command` | Leo's docstring copies find-all's. |  |
| `re-search` / `re-search-forward` | Same as start-find, with regex. | `/` |
| `re-search-backward` | Same as start-find, but with regex and in reverse. | `?` |
| `summarize` | Prompt for a regex and list all matches in a new top-level node. |  |
| `tag-children` | Prompt for a tag and add it to all children of c.p. |  |
| `tag-node` | Prompt for a tag and add it to c.p. |  |
| `word-search` / `word-search-forward` | Same as start-search, with whole_word setting. |  |
| `word-search-backward` | Same as word-search, but in reverse. |  |
| `isearch-forward` | Begin a forward incremental search. |  |
| `isearch-backward` | Begin a backward incremental search. |  |
| `isearch-forward-regexp` | Begin a forward incremental regexp search. | `/`, which moves as the pattern is typed |
| `isearch-backward-regexp` | Begin a backward incremental regexp search. | `?` |
| `isearch-with-present-options` | Begin an incremental search using find panel options. |  |

## Body editing

Text editing in the body. Leo's are Emacs-style commands; leotui's body is a vim grammar, so a command is covered by keys rather than by a name. The table gives those keys; the commands after it have no leotui equivalent.

200 commands, in `abbrevCommands.py`, `commanderEditCommands.py`, `editCommands.py`, `killBufferCommands.py`, `rectangleCommands.py`, `spellCommands.py`. 3 by the same name, 93 by an equivalent, 104 not at all.

Same name: `edit-headline`, `extract`, `reformat-paragraph`.

| command | Leo's summary | leotui |
|-|-|-|
| `unindent-region` | Remove one tab's worth of indentation from all presently selected lines. | `<` |
| `match-brackets` / `select-to-matching-bracket` | Select the text between matching brackets. | `%`, and `v%` to select |
| `always-indent-region` | Indent each selected line by @tabwidth. | `>` |
| `indent-region` | Indent each selected line; insert a tab when nothing is selected. | `>` in NORMAL or VISUAL |
| `downcase-word` | Convert all characters of the word at the cursor to lower case. | `guw` |
| `upcase-word` | Convert all characters of the word at the cursor to UPPER CASE. | `gUw` |
| `backward-find-character` | Search backwards for a character. | `F` |
| `backward-find-character-extend-selection` | Search backward for a character, extending the selection. | `vF` |
| `find-character` | Search for a character. | `f` |
| `find-character-extend-selection` | Search for a character, extending the selection. | `vf` |
| `goto-line` | Put the cursor at the n'th line of the buffer. | `Ngg` |
| `add-tab-to-lines` | Add a tab to start of all lines, or all selected lines. | `>` |
| `remove-tab-from-lines` | Remove a tab from start of all lines, or all selected lines. | `<` |
| `backward-delete-char` | Delete the character to the left of the cursor. | `X` |
| `clear-selected-text` | Delete the selected text. | `d` in VISUAL |
| `delete-word` | Delete the word at the cursor. | `dw` |
| `backward-delete-word` | Delete the word in front of the cursor. | `db` |
| `delete-word-smart` | Delete the word at the cursor, treating whitespace and symbols smartly. | `dw` |
| `backward-delete-word-smart` | Delete the word in front of the cursor, treating whitespace and symbols smartly. | `db` |
| `delete-char` | Delete the character to the right of the cursor. | `x` |
| `insert-newline` | Insert a newline at the cursor. | `o`, or `Enter` in INSERT |
| `replace-current-character` | Replace the current character with the next character typed. | `r` |
| `back-to-home` | Smart home: Position the point at the first non-blank character on the line, or the start of the line if... | `^` |
| `back-to-home-extend-selection` | Smart home, extending selection. | `v^` |
| `back-to-indentation` | Position the point at the first non-blank character on the line. | `^` |
| `back-word` | Move the cursor to the previous word. | `b` |
| `back-word-extend-selection` | Extend the selection by moving the cursor to the previous word. | `vb` |
| `back-word-smart` | Move the cursor to the beginning of the current or the end of the previous word. | `b` |
| `back-word-smart-extend-selection` | Extend the selection by moving the cursor to the beginning of the current or the end of the previous word. | `vb` |
| `beginning-of-line` | Move the cursor to the first character of the line. | `0` |
| `beginning-of-line-extend-selection` | Extend the selection by moving the cursor to the first character of the line. | `v0` |
| `next-line` | Move the cursor down, extending the selection if in extend mode. | `j` |
| `next-line-extend-selection` | Extend the selection by moving the cursor down. | `vj` |
| `previous-line` | Move the cursor up, extending the selection if in extend mode. | `k` |
| `previous-line-extend-selection` | Extend the selection by moving the cursor up. | `vk` |
| `beginning-of-buffer` | Move the cursor to the start of the body text. | `gg` |
| `beginning-of-buffer-extend-selection` | Extend the text selection by moving the cursor to the start of the body text. | `vgg` |
| `end-of-buffer` | Move the cursor to the end of the body text. | `G` |
| `end-of-buffer-extend-selection` | Extend the text selection by moving the cursor to the end of the body text. | `vG` |
| `back-char` | Move the cursor back one character, extending the selection if in extend mode. | `h` |
| `back-char-extend-selection` | Extend the selection by moving the cursor back one character. | `vh` |
| `forward-char` | Move the cursor forward one character, extending the selection if in extend mode. | `l` |
| `forward-char-extend-selection` | Extend the selection by moving the cursor forward one character. | `vl` |
| `clear-extend-mode` | Turn off extend mode: cursor movement commands do not extend the selection. | `Escape` |
| `set-extend-mode` | Turn on extend mode: cursor movement commands do extend the selection. | `v` |
| `toggle-extend-mode` | Toggle extend mode, i.e., toggle whether cursor movement commands extend the selections. | `v` |
| `end-of-line` | Move the cursor to the last character of the line. | `$` |
| `end-of-line-extend-selection` | Extend the selection by moving the cursor to the last character of the line. | `v$` |
| `exchange-point-mark` | Exchange the point (insert point) with the mark (the other end of the selected text). | `o` in VISUAL |
| `extend-to-line` | Select the line at the cursor. | `V` |
| `extend-to-word` | Compute the word at the cursor. | `viw` |
| `finish-of-line` | Move the cursor to the last character of the line. | `$` |
| `finish-of-line-extend-selection` | Extend the selection by moving the cursor to the last character of the line. | `v$` |
| `forward-end-word` | Move the cursor to the next word. | `e` |
| `forward-end-word-extend-selection` | Extend the selection by moving the cursor to the next word. | `ve` |
| `forward-word` | Move the cursor to the next word. | `w` |
| `forward-word-extend-selection` | Extend the selection by moving the cursor to the end of the next word. | `vw` |
| `forward-word-smart` | Move the cursor to the end of the current or the beginning of the next word. | `w` |
| `forward-word-smart-extend-selection` | Extend the selection by moving the cursor to the end of the current or the beginning of the next word. | `vw` |
| `move-past-close` | Move the cursor past the closing parenthesis. | `%` |
| `move-past-close-extend-selection` | Extend the selection by moving the cursor past the closing parenthesis. | `v%` |
| `back-page` | Move the cursor back one page, extending the selection if in extend mode. | `Ctrl-b` |
| `back-page-extend-selection` | Extend the selection by moving the cursor back one page. | `Ctrl-b` in VISUAL |
| `forward-page` | Move the cursor forward one page, extending the selection if in extend mode. | `Ctrl-f` |
| `forward-page-extend-selection` | Extend the selection by moving the cursor forward one page. | `Ctrl-f` in VISUAL |
| `back-paragraph` | Move the cursor to the previous paragraph. | `{` |
| `back-paragraph-extend-selection` | Extend the selection by moving the cursor to the previous paragraph. | `v{` |
| `forward-paragraph` | Move the cursor to the next paragraph. | `}` |
| `forward-paragraph-extend-selection` | Extend the selection by moving the cursor to the next paragraph. | `v}` |
| `select-all` | Select all text. | `ggVG` |
| `start-of-line` | Move the cursor to first non-blank character of the line. | `^` |
| `start-of-line-extend-selection` | Extend the selection by moving the cursor to first non-blank character of the line. | `v^` |
| `backward-kill-paragraph` | Kill the previous paragraph. | `d{` |
| `kill-paragraph` | Kill the present paragraph. | `dap` |
| `extend-to-paragraph` | Select the paragraph surrounding the cursor. | `vip` |
| `indent-rigidly` | Insert a hard tab at the start of each line of the selected text. | `>` |
| `downcase-region` | Convert all characters in the selected text to lower case. | `gu` |
| `toggle-case-region` | Toggle the case of all characters in the selected text. | `~`, `g~` |
| `upcase-region` | Convert all characters in the selected text to UPPER CASE. | `gU` |
| `scroll-down-half-page` | Scroll the presently selected pane down one line. | `Ctrl-d` |
| `scroll-down-page` | Scroll the presently selected pane down one page. | `Ctrl-f` |
| `scroll-up-half-page` | Scroll the presently selected pane down one line. | `Ctrl-u` |
| `scroll-up-page` | Scroll the presently selected pane up one page. | `Ctrl-b` |
| `transpose-lines` | Transpose the line containing the cursor with the preceding line. | `ddp` |
| `transpose-chars` | Swap the characters at the cursor. | `xp` |
| `backward-kill-word` | Kill the previous word. | `db` |
| `kill-word` | Kill the word containing the cursor. | `dw` |
| `kill-to-end-of-line` | Kill from the cursor to end of the line. | `D` |
| `kill-line` | Kill the line containing the cursor. | `dd` |
| `kill-region` | Kill the text selection. | `d` in VISUAL |
| `kill-region-save` | Add the selected text to the kill ring, but do not delete it. | `y` in VISUAL |
| `yank` | Insert the next entry of the kill ring. | `p` |
| `zap-to-character` | Kill characters from the insertion point to a given character. | `dt`, `df` |

Not implemented: `dabbrev-completion`, `dabbrev-expands`, `abbrev-kill-all`, `abbrev-list`, `toggle-abbrev-mode`, `add-comments`, `convert-all-blanks`, `convert-all-tabs`, `convert-blanks`, `convert-tabs`, `delete-comments`, `extract-names`, `insert-body-time`, `justify-toggle-auto`, `line-to-headline`, `settings`, `reformat-body`, `reformat-selection`, `hide-invisibles`, `show-invisibles`, `toggle-invisibles`, `toggle-angle-brackets`, `unformat-paragraph`, `insert-jupyter-toc`, `insert-markdown-toc`, `delete-trace-statements`, `next-or-end-of-line`, `next-or-end-of-line-extend-selection`, `previous-or-beginning-of-line`, `previous-or-beginning-of-line-extend-selection`, `select-next-trace-statement`, `insert-file-name`, `tabify`, `untabify`, `capitalize-word`, `capitalize-words-or-selection`, `set-comment-column`, `indent-to-comment-column`, `center-line`, `set-fill-column`, `center-region`, `set-fill-prefix`, `find-word`, `find-word-in-line`, `goto-char`, `delete-indentation`, `indent-relative`, `line-number`, `view-lossage`, `view-recent-commands`, `what-line`, `add-space-to-lines`, `remove-space-from-lines`, `clean-all-lines`, `clean-lines`, `delete-spaces`, `insert-hard-tab`, `newline-and-indent`, `insert-parentheses`, `insert-soft-tab`, `remove-blank-lines`, `split-line`, `extend-to-sentence`, `pop-cursor`, `push-cursor`, `back-sentence`, `back-sentence-extend-selection`, `forward-sentence`, `forward-sentence-extend-selection`, `fill-region`, `fill-region-as-paragraph`, `fill-paragraph`, `count-region`, `move-lines-down`, `move-lines-up`, `reverse-region`, `scroll-down-line`, `scroll-up-line`, `sort-columns`, `reverse-sort-lines-ignoring-case`, `reverse-sort-lines`, `sort-lines-ignoring-case`, `sort-lines`, `transpose-words`, `backward-kill-sentence`, `clear-kill-ring`, `kill-sentence`, `kill-ws`, `yank-pop`, `rectangle-clear`, `rectangle-close`, `rectangle-delete`, `rectangle-kill`, `rectangle-open`, `rectangle-string`, `rectangle-yank`, `show-spell-info`, `clean-main-spell-dict`, `clean-user-spell-dict`, `spell-tab-open`, `spell-as-you-type-toggle`, `spell-as-you-type-wrap`, `spell-as-you-type-next`, `spell-as-you-type-undo`.

## Help, keys and settings

Help text, key bindings, and settings.

57 commands, in `commanderHelpCommands.py`, `helpCommands.py`, `keyCommands.py`, `leoKeys.py`. 2 by the same name, 4 by an equivalent, 51 not at all.

Same name: `full-command`, `help`.

| command | Leo's summary | leotui |
|-|-|-|
| `show-bindings` | Print all the bindings presently in effect. | `F1`, `leotui --keys` |
| `show-commands` | Print all the known commands and their bindings, if any. | `F1` lists every command |
| `keyboard-quit` | Clears the state and the minibuffer label. | `Escape` |
| `help-for-command` | Prompts for a command name and prints the help message for that command. | `:help name` |

Not implemented: `show-buttons`, `show-buttons-and-at-commands`, `show-commands-with-docs`, `repeat-complex-command`, `set-command-state`, `set-insert-state`, `set-overwrite-state`, `toggle-input-state`, `exit-named-mode`, `mode-help`, `about-leo`, `edit-setting`, `edit-shortcut`, `open-leo-docs-leo` / `leo-docs-leo`, `open-quickstart-leo` / `leo-quickstart-leo`, `open-cheat-sheet-leo` / `leo-cheat-sheet` / `cheat-sheet`, `open-desktop-integration-leo` / `desktop-integration-leo`, `open-leo-dist-leo` / `leo-dist-leo`, `open-leo-py-leo` / `leo-py-leo`, `open-leo-py-ref-leo` / `leo-py-ref-leo`, `open-scripts-leo` / `leo-scripts-leo`, `open-leo-settings` / `open-leo-settings-leo` / `leo-settings`, `open-my-leo-settings` / `open-my-leo-settings-leo` / `my-leo-settings`, `open-online-home`, `open-online-toc`, `open-online-scripting-miscellany`, `open-online-tutorials`, `open-users-guide`, `open-online-videos`, `open-python-window`, `open-local-settings`, `help-for-abbreviations`, `help-for-autocompletion`, `help-for-bindings`, `help-for-creating-external-files`, `help-for-debugging-commands`, `help-for-drag-and-drop`, `help-for-dynamic-abbreviations`, `help-for-find-commands`, `help-for-keystroke`, `help-for-layouts`, `help-for-minibuffer`, `help-for-regular-expressions`, `help-for-scripting`, `help-for-settings`, `show-color-settings`, `show-font-settings`, `show-settings`, `show-settings-outline`, `help-for-python`, `menu-shortcut`.

## Scripting and external tools

Running code, external checkers and converters, markup, printing, diffing and debugging.

88 commands, in `checkerCommands.py`, `controlCommands.py`, `convertCommands.py`, `debugCommands.py`, `leoBeautify.py`, `leoCommands.py`, `leoCompare.py`, `leoDebugger.py`, `leoMarkup.py`, `leoPrinting.py`, `leoRst.py`. 0 by the same name, 1 by an equivalent, 87 not at all.

| command | Leo's summary | leotui |
|-|-|-|
| `redraw` | Redraw the outline. | not needed: every key redraws |

Not implemented: `beautify-c` / `pretty-print-c`, `fstringify-files`, `diff-fstringify-files` / `fstringify-files-diff`, `silent-fstringify-files` / `fstringify-files-silent`, `beautify-tree`, `execute-general-script`, `execute-external-file`, `execute-pytest`, `execute-script`, `recolor`, `dump-expanded`, `diff-and-open-leo-files`, `diff-leo-files`, `diff-marked-nodes`, `db-again`, `db-b`, `db-c`, `db-h`, `db-l`, `db-n`, `db-q`, `db-r`, `db-s`, `db-w`, `db-input`, `db-status`, `xdb`, `adoc`, `adoc-with-preview`, `pandoc`, `pandoc-with-preview`, `sphinx`, `sphinx-with-preview`, `preview-body`, `preview-html`, `preview-expanded-body`, `preview-expanded-html`, `preview-marked-bodies`, `preview-marked-html`, `preview-marked-nodes`, `preview-node`, `preview-tree-bodies`, `preview-tree-nodes`, `preview-tree-html`, `print-body`, `print-html`, `print-expanded-body`, `print-expanded-html`, `print-marked-bodies`, `print-marked-html`, `print-marked-nodes`, `print-node`, `print-tree-bodies`, `print-tree-html`, `print-tree-nodes`, `rst-convert-legacy-outline` / `convert-legacy-rst-outline`, `rst3`, `check-nodes`, `find-long-lines`, `find-missing-docstrings`, `mypy`, `ruff`, `ty`, `show-plugin-handlers`, `show-plugins-info`, `set-silent-mode`, `shell-command`, `shell-command-on-region`, `act-on-node`, `save-buffers-kill-leo`, `suspend`, `iconify-frame`, `add-mypy-annotations`, `c-to-python`, `convert-unls`, `make-stub-files`, `python-to-coffeescript`, `python-to-rust`, `python-to-typescript`, `typescript-to-py`, `debug`, `dump-node`, `gc-collect-garbage`, `gc-dump-all-objects`, `gc-show-summary`, `kill-log-listener` / `log-kill-listener`, `show-focus`.

## The application and its GUI

Leo's windows, menus, buffers, chapters, its own vim mode, sessions and caches.

65 commands, in `bufferCommands.py`, `editCommands.py`, `leoApp.py`, `leoBackground.py`, `leoChapters.py`, `leoColorizer.py`, `leoGlobals.py`, `leoPersistence.py`, `leoSessions.py`, `leoVim.py`. 3 by the same name, 11 by an equivalent, 51 not at all.

Same name: `open-url-under-cursor`, `focus-to-body`, `focus-to-tree`.

| command | Leo's summary | leotui |
|-|-|-|
| `exit-leo` / `quit-leo` | Exit Leo, prompting to save unsaved outlines first. | `:q` |
| `:e` | Handle the vim :e command. | `:e` |
| `:q` | Quit the present Leo outline, prompting for saves. | `:q` |
| `:qa` | Quit only if there are no unsaved changes. | `:q` (one outline) |
| `:q!` | Quit immediately. | `:q!` |
| `:e!` | Revert all changes to a .leo file, prompting if there have been changes. | `:e!` |
| `:%s` | Handle the Vim :%s command. | `:%s` |
| `:s` | Handle the Vim :s command. | `:s` |
| `:w` | Save the .leo file. | `:w` |
| `:xa` | Save all open files and keep working. | `:x` (one outline) |
| `:wq` | Save all open files and exit. | `:wq` |

Not implemented: `ctrl-click-at-cursor`, `demangle-recent-files`, `disable-idle-time-events`, `enable-idle-time-events`, `toggle-idle-time-events`, `open-url`, `listen-to-log` / `log-listen`, `bpm-status`, `chapter-select`, `chapter-back`, `chapter-next`, `dump-last-colorizer-trace`, `show-stats`, `cls`, `clean-persistence`, `session-clear`, `session-create`, `session-refresh`, `session-restore`, `session-snapshot-load`, `session-snapshot-save`, `:tabnew`, `:print-dot`, `:r`, `:!`, `:toggle-vim-mode`, `:toggle-vim-trace`, `:toggle-vim-trainer-mode`, `buffer-append-to`, `buffer-copy`, `buffer-insert`, `buffer-kill`, `buffers-list`, `buffers-list-alphabetically`, `buffer-prepend-to`, `buffer-switch-to`, `do-nothing`, `activate-cmds-menu`, `activate-edit-menu`, `activate-file-menu`, `activate-help-menu`, `activate-outline-menu`, `activate-plugins-menu`, `activate-window-menu`, `focus-to-log`, `focus-to-minibuffer`, `ctrl-click-icon`, `click-icon-box`, `double-click-icon-box`, `right-click-icon`, `click-click-box`.

## Directives

The 36 names in Leo's `globalDirectiveList`, set against `GLOBAL_DIRECTIVES` in `atfile_write.rs`, which holds the same names.

| directive | Leo | leotui and leolib |
|-|-|-|
| `@all` | Write every descendant, expanding no section reference or `@others`. | Full. |
| `@others` | Write the children not named by a section reference. | Full. |
| `@first` | Copy a line to the file's start, before the sentinels. | Full. |
| `@last` | Copy a line to the file's end, after the sentinels. | Full. |
| `@doc` | Start a doc part (as does a bare `@`). | Full. |
| `@c` | End a doc part. | Full. |
| `@code` | End a doc part. | Full. |
| `@comment` | Set the comment delimiters. | Full. |
| `@delims` | Set the comment delimiters, older form. | Full. |
| `@section-delims` | Set the section-reference brackets. | Full. Leo reads a reference back regex-escaped (`TODO.md`). |
| `@ignore` | Leave the tree out of every write. | Full. |
| `@path` | Directory for relative file names. | Full. |
| `@language` | Comment delimiters and colouring. | Full, for both. |
| `@lineending` | Line endings of the written file. | Full. |
| `@tabwidth` | Tab width; negative means spaces. | Full. |
| `@color` | Colour the body. | Full, by the highlighter. |
| `@nocolor` | Do not colour the body from here. | Full, by the highlighter. |
| `@nocolor-node` | Do not colour this node. | Full, by the highlighter. |
| `@killcolor` | Do not colour this node at all. | Full, by the highlighter. |
| `@noheader` | A markdown node written with no heading line. | Full, by the `@auto-md` importer and writer. |
| `@encoding` | The external file's encoding. | Partly. utf-8 and ascii only; another is refused (`porting-notes.md`). |
| `@pagewidth` | Width `reformat-paragraph` and the fill commands wrap to. | Full. `reformat-paragraph` reads it. |
| `@nosearch` | Leave the tree out of find commands. | Partly. `clone-find-all` skips it; `/` does not. |
| `@wrap` | Wrap long body lines. | Recognised only. No effect; `:set wrap` wraps. |
| `@nowrap` | Do not wrap long body lines. | Recognised only. No effect. |
| `@beautify` | Allow the beautify commands on the tree. | Recognised only. No beautify command. |
| `@nobeautify` | Keep the beautify commands off the tree. | Recognised only. No beautify command. |
| `@killbeautify` | Keep the beautify commands off the tree. | Recognised only. No beautify command. |
| `@unit` | Legacy `@root` era; read only by `convert-at-root`. | Recognised only. No `convert-at-root`. |
| `@header` | Legacy; only checked against `@noheader`. | None, as in Leo. |
| `@markup` | Listed only; the colorizer has a colour of that name. | None, as in Leo. |
| `@colorcache` | Listed only. | None, as in Leo. |
| `@nopyflakes` | Listed only. | None, as in Leo. |
| `@quiet` | Listed only. | None, as in Leo. |
| `@silent` | Listed only. | None, as in Leo. |
| `@verbose` | Listed only. | None, as in Leo. |

## Node kinds

### `@<file>` kinds

Leo's `atFileNames` and `atAutoNames`, and the `@auto-<name>` spellings its importers register, set against `AT_FILE_NAMES` and `AT_AUTO_NAMES` in `node.rs` and the importer table in `importers.rs`.

| kind | Leo | leotui and leolib |
|-|-|-|
| `@file`, `@thin`, `@file-thin` | Sentinel file; the file holds the tree. | Read and written. |
| `@clean` | No sentinels; the tree is merged back on read. | Read and written. |
| `@auto` | Imported on read, written without sentinels. | Read and written where an importer exists; another extension is read whole into the body, as in Leo. |
| `@auto-md`, `@auto-markdown`, `@auto-org`, `@auto-org-mode`, `@auto-otl`, `@auto-vim-outline` | `@auto` with a named importer. | Read and written. |
| `@auto-rst` | `@auto` with the reStructuredText importer. | Recognised; no importer, so unread. |
| `@edit` | The whole file in one body. | Read and written. |
| `@asis`, `@file-asis` | Written verbatim; never read. | Written. |
| `@nosent`, `@file-nosent` | Written without sentinels; never read. | Written. |
| `@jupytext` | A notebook, through jupytext. | Refused on read and write. |
| `@shadow` | Deprecated. | Not ported (`porting-notes.md`). |

### Other headline kinds

Headlines Leo acts on that name no file. None is ported; each depends on a feature under "Other features" or on an unported command.

| kind | Leo |
|-|-|
| `@button`, `@command`, `@rclick` | A script bound to a button, a command name, or a context menu. |
| `@settings`, and `@bool`, `@int`, `@string`, `@data`, `@shortcuts` and others below it | A settings tree. |
| `@chapter` | A chapter: a tree shown alone. |
| `@test` | A unit test, run by `execute-pytest`. |
| `@persistence` | Stored uAs and gnxs for `@auto` trees. |
| `@rst` | A tree `rst3` writes as reStructuredText. |
| `@url` | A link `open-url` follows. |
| `@spot` | The target of `clone-to-at-spot`. |
| `@leo` | An outline `open-at-leo-file` opens. |
| `@read-file-into-node` | A node `write-file-from-node` writes back. |

## Same name, different behaviour

- `search-forward` and `search-backward` open `/` and `?`, which take a regex. Leo's search plain text, with the Find tab's options.

- `find-next` and `find-prev` repeat the last `/` or `?`. Leo's use the Find tab's pattern and options.

- `help` opens the key bindings. Leo's opens a help text.

- `open-url-under-cursor` follows a section reference only. Leo's also opens urls, unls and gnxs.

- `clone-find-all` and `clone-find-all-flattened` take a regex with smartcase, as `/` does.

- `delete-marked-nodes` keeps the last top-level node, and keeps the selection if it survives.

- `extract` reads no `@data extract-patterns` node.

## Other features

- **Scripting.** `execute-script`, the `@button`, `@command`, `@rclick` and `@test` node kinds, and the `c`, `g` and `p` API. None ported. `leocub-vs-leotui.md` names Rhai as the path if it is ever wanted.

- **Plugins.** 211 commands (215 names) in 46 of the 143 files in `leo/plugins`. The largest: `qt_frame.py` 28, `viewrendered3.py` 22, `leoscreen.py` 17, `active_path.py` 14, `viewrendered.py` 13, `bookmarks.py` 12. The plugin system itself is not ported.

- **Settings.** Leo reads `@settings` trees in `leoSettings.leo`, `myLeoSettings.leo` and the outline itself: `@bool`, `@int`, `@string`, `@data`, `@shortcuts` and others. leotui and leogui read `~/.config/leo-rs/settings.toml`, and no settings node.

- **Chapters.** The `@chapter` node kind and the `chapter-*` commands. Not ported.

- **Outline formats.** `.leojs` (JSON) and `.db` (SQLite) outlines. Neither is read or written.

- **The GUI.** Qt: menus, the log pane, the Find and Spell tabs, node icons, drag and drop, several outlines in tabs or windows, a second view of one outline, recent-files menus, and rendered views (`viewrendered*` plugins). leotui shows one outline per process, with a status line.

- **Servers.** `leoserver.py` serves LeoJS and leoInteg; `leoBridge` embeds Leo in a Python program. Not ported; `ideas.md` lists an MCP server over `leolib`.

- **Sessions and caches.** `session-*`, the persistence cache, and the `@persistence` node kind. Between runs leotui remembers which nodes are unfolded and marked, in `~/.leo/leo-rs/` (`state.rs`), and nothing else.

## leotui features with no Leo counterpart

- The body is a vim buffer: operators, motions, text objects, counts, `.`, registers and VISUAL. Leo's own vim mode (`leoVim.py`, 17 commands) covers a subset.

- `:[range]s` in a body, and `:bufdo %s` over every body, each one undo step.

- `gd`, the section jump on a key.

- tree-sitter highlighting, and themes.

- `Ctrl-c` never discards typed text.

- `:set` options, read as vim reads them.

