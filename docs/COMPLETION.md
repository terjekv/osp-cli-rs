# Completion and History

In the REPL, completion suggests commands and arguments as you type. History
lets you recall and repeat previous commands. Both use local data.

## How completion works

Completion works from already-known local information:

- the visible command catalog
- built-in command/flag metadata
- plugin-provided command metadata that has already been discovered
- config-key metadata
- the current shell scope
- the current cursor position and tokens already on the line

It does not call remote systems while you are pressing tab.

## While You Type

- **Tab** completes like a shell first: a single match, or an exact word no
  other choice extends, is inserted with a trailing space; otherwise the word
  grows to the choices' shared prefix. When there is nothing to add, Tab opens
  the menu with nothing selected, and further Tabs (Shift-Tab backwards) put
  each choice into the line. Space keeps the choice and moves on; Enter keeps
  it without running the line.
- **The menu** lists up to 20 choices as one column with their descriptions,
  and more as a grid. A flag's other spellings share one entry
  (`--interactive, -i`), required flags come first and are marked `*`, and
  value aliases match what you type without being listed.
- **Muted text after the cursor** suggests the rest of the line: the newest
  history entry that extends it, or the only completion for the current word.
  Right or End accepts it; Alt-Right accepts one word.
- **The line under the prompt** shows the command's declared positional usage,
  with the current slot in bold: `<NAME>` is required, `[NAME]` is optional,
  and `…` permits repeated values. Filled slots use bare names. A `·` separates
  usage from help or the current slot's choices (`on | off`, for example).
  Flag values retain their own help. Commands without positional declarations
  show their usual description and missing required flags. Errors name the
  invalid word. The status stays on one terminal row.
- **Red** marks the first word that cannot be valid: an unknown subcommand or
  flag. A word still being typed is not marked while it can grow into a valid
  one.

`repl.tab_mode` chooses when typing opens the menu without Tab:

- `tab` (default): only Tab, Ctrl-Space, or starting a flag with `-`
- `always`: every typed character, including a space
- `after_<n>_letters`, e.g. `after_1_letter` or `after_3_letters`: once the
  current word has that many characters

Any positive count is accepted. Unrecognized values fall back to `tab`.

## Command prefixes

`command.prefixes` controls native command-word abbreviations:

- `interactive` (default): accept unique prefixes in the REPL.
- `always`: also accept them in one-shot shell invocations.
- `off`: require full command words and suggest the exact command instead.

Change it with `config set command.prefixes always` (or `off`/`interactive`).
The top-level command name remains exact. Exact subcommand names win over
longer matches; ambiguous prefixes list canonical command paths. Typo hints
never execute the suggested command. Argument values are not abbreviated.
Native commands can reserve literal controls such as login and logout.

Tab works with partial input in every mode and inserts full canonical names.
Rich help and completion menus bold the shortest executable prefix when
abbreviations are enabled; plain and JSON output keep ordinary names. Scoped
integration menus omit global utilities, which still work by their exact names.

## Shell Completion Scripts

Generate a static script for your shell with:

```bash
osp completions bash
osp completions zsh
osp completions fish
```

The generated script covers built-in one-shot commands and flags. Native
integrations and plugins are discovered at runtime, so their dynamic command
trees remain available through the REPL completion engine rather than the
static shell script.

## What Completion Can Suggest

The REPL completes:

- commands and subcommands
- flags
- flag values
- `config set` keys
- scoped commands inside a supported REPL shell, when your install exposes one
- DSL stages after `|`
- path-like values when a command or plugin marks them as path inputs

Examples:

```text
plugins <TAB>
config set <TAB>
theme use <TAB>
plugins list | <TAB>
```

That is the important contract: completion should help you finish real command
shapes, not just top-level words.

## Context-Aware Suggestions

Completion uses the current command context, including flags already present on
the line.

That matters for things like:

- provider-specific values
- path-like flag values
- DSL suggestions after a pipeline marker
- shell-scoped commands after entering a supported shellable root

Built-in namespaces like `plugins`, `config`, and `theme` do not become
shells. Shell controls like `exit` and `quit` remain REPL-owned.

## Invocation Flags

REPL completion also knows about the invocation-level flags shared with the
CLI, such as:

- `--format`
- `--json`
- `--verbose`
- `--quiet`
- `--debug`

Parsing also accepts short forms like `-v/-q/-d`, but completion suggestions
use the long names.

## History

The REPL keeps history for two different jobs:

- navigation through previous commands
- history expansion when you want to replay or adapt an earlier command

An accepted command stays in navigation and persistent history when execution
fails. Unknown commands and invalid syntax are removed after dispatch. The
entered bang request expands in the editor on Tab; Enter echoes and executes
the expanded command and keeps it as the last command when it is accepted,
including after an execution failure.

History updates are written immediately under a file lock and atomically
replaced with private permissions. They use normal filesystem buffering rather
than forcing a disk flush before the next prompt; recent history can be lost
if the machine crashes or loses power.

Ctrl-R filters commands as you type, ranks matches by score, and underlines the
matched text. Equal scores retain newest-first order. Inside a shell it searches
commands in that shell without their stored prefix; Ctrl-R again searches all
history. Enter runs the selection and Tab puts it back in the editor.

Bang IDs match `history list` and Ctrl-R in the current shell scope. The shell
prefix is omitted from the recalled input and applied once during execution.

Alt+. inserts the previous command's last shell word at the cursor, preserving
quotes and escapes. Consecutive presses replace that insertion with the last
word from successively older entries; any other key ends the cycle. Inside a
shell it uses the same scoped, prefix-free history as Up-arrow and Ctrl-R.

Supported expansions:

- `!!`
  - last command
- `!-N`
  - Nth previous command
- `!123`
  - absolute history entry
- `!123 args...`
  - absolute history entry with arguments appended before execution
- `!prefix`
  - most recent command starting with that prefix
- `!?text`
  - most recent command containing that text

Examples:

```text
!! 
!plugins
!-2
!123 --json
```

Help/history commands and literal bang expressions are excluded. Accepted bang
replays save the expanded command unless excluded; a leading space keeps the
replay off disk.

`history list` accepts inclusive ISO-like time bounds when you want a smaller
window:

```text
history list --since 2026-08-11 --until 2026-08-11T23:59:59Z
```

Accepted bounds are a date, a date plus `HH:MM[:SS]`, or an RFC3339-like
timestamp with `Z` or a numeric offset. Natural-language values such as
`today` and `now` are not accepted.

## Scoped History

`repl.history.path` controls identity/profile isolation. Default filenames
lowercase the identity and keep ordinary and drift accounts separate. Use a path
without `${profile.active}` to share history across profiles.
`repl.history.profile_scoped` is retired.

History is shell-aware. That makes repeated work inside a supported shellable
root more predictable because the shell prefix stays part of the command
context after shell entry.

The public REPL model is shell-first:

- bare `ldap` enters the `ldap` shell from root
- repeating the current shell root such as `(ldap) ldap` shows help for that
  shell
- typing another shell root switches shells; shells never nest

If your install never exposes shellable roots, you can ignore this section.

## If Completion Looks Wrong

Check the command catalog and configuration first:

1. confirm the command exists in the current catalog:
   `plugins list`
2. confirm the plugin/provider state is healthy:
   `doctor plugins`
3. check shadowed or disabled entries in `plugins list`
4. if the issue is on `config set`, confirm you are using a real config key

Completion problems are often catalog problems, not editor problems.

If the line looks visually wrong rather than semantically wrong, that is more
likely a REPL/editor presentation issue than a completion-logic issue. See
[REPL.md](REPL.md).
