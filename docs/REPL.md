# REPL

The REPL runs the same commands as one-shot `osp` and keeps shell scope,
history, the last result, and session configuration between commands.

Use the REPL when you are exploring, iterating, or repeatedly looking at the
same backend data with different pipes and output formats. Use one-shot CLI
commands when you are scripting, automating, or just need one answer.

Examples that use `inventory ...` below are illustrative provider-backed
commands. Replace them with a real plugin command from
`plugins list`.

## How a line is processed

```text
typed line
  ↓
shell scope + one-shot flags + alias/DSL parsing
  ↓
same command execution path as one-shot `osp`
  ↓
optional DSL pipeline
  ↓
render output
  ↓
keep session state for the next prompt
```

## Start the REPL

```bash
osp
```

## Intro and help documents

The intro and root `help` compose ordered semantic document blocks. Configure
`repl.intro` and `repl.intro_template.minimal`, `.compact`, or `.full` to author
an intro with Markdown headings, paragraphs, inline markup, and lists. Nested
and loose list items retain their parent text. `{{ help }}` and `{{ overview }}`
insert the root help blocks at that position.

Fenced `osp` blocks contain passive JSON data; other fences and invalid JSON
remain literal text. Repeated fences stay separate and ordered. Templates do
not execute commands. `intro --json` emits a document object; `intro | L 2`
limits its flat rows. Root help uses the same row pipeline and section headings.

## Same Grammar as the CLI

If a command works as a one-shot invocation, the same command text should work
inside the REPL.

```bash
osp plugins list --json -v
```

```text
plugins list --json -v
```

That includes:

- DSL pipes
- `--format` and format shorthands like `--json`
- `-v/-q/-d`

Failures use the same renderer as one-shot commands: `error:`, an optional
`request-id:`, and at most one `hint:`. Failure wording stays stable as verbosity
increases. Plugin messages preserve sanitized text, levels and ordering on
failed commands; known injected secrets are redacted.

Launch `-v`/`-q`/`-d` flags set session defaults. Per-line flags add to those
defaults for one command only. `-v` explains service and session selection,
`-vv` shows exchanges, and `-vvv` adds response/cache evidence. These diagnostics
and verbose field supplements are complete stderr lines above the prompt.
`-d` shows host dispatch; `-dd` adds internal host decisions and captured
backtraces. File logging is independent of per-line stderr settings.

Read the error and its hint. `doctor last -vvv` reads retained sanitized evidence
without repeating the failed operation. `doctor last -dd` writes the captured
backtrace to stderr. `doctor` checks local setup without contacting services.
A configured `repl.intro=none` remains disabled at every verbosity level.

For successful results, the REPL also keeps the last replayable output:

- `last`
  - replay the last successful result with its original pipe stages
- `last --raw`
  - show the pre-pipeline payload from that same successful command
- `last | state`, `last --json`, `last --raw | P name`
  - use normal output formats and DSL stages without another service request
  - new stages follow the saved pipeline; `--raw` skips that saved pipeline
  - replay keeps the saved result intact, so later `last` calls start from it
- `doctor last`
  - stays failure-only

## What The REPL Adds

The REPL is useful because it keeps a few things alive across commands:

- shell scope
- history and history expansion
- completion
- the last successful result for local replay and inspection
- session-scoped config overrides
- session context published by native commands and plugins

Use `context` to inspect the latest value for each subject kind. Rows include
the kind, value, command path, and update time (stored in UTC, shown in local
time). `context clear` removes all entries; `context clear host` removes only
the `host` entry. The command accepts normal output flags and DSL pipes, for
example `context --json` or `context | F kind == 'host'`.

Context exists only in the REPL process, so `osp context` is not useful as a
one-shot command and is restricted to the REPL. Entries survive config-driven
REPL rebuilds for the life of that session.

## Shell Scope

Suggested commands match the current prompt. Inside `orch`, both `task …` and
`orch task …` work unless a host command or alias shadows the relative first
word: use `orch context …` to distinguish it from the host's `context` command.
Suggestions for another root keep their root form; leave the current shell
before running them. JSON follow-ups in the REPL use the same form, while
one-shot output uses `osp …`.

`repl.shellable_commands` selects eligible
top-level roots; its defaults are `nh`, `mreg`, `ldap`, `vm`, and `orch`.
Built-in namespaces such as `plugins`, `config`, `theme`, and `help` do not
become shells.

Most plain upstream installs will not expose those domain roots, so you can
ignore shell scope until your downstream distribution provides one. When it
does, shell entry is shell-first:

```text
ldap
user oistes
exit
```

Typing the bare root enters that shell and makes later lines inherit the root
until you leave it.

Inside an active shell, repeating the current root shows help for that shell:

```text
ldap
ldap
```

The second `ldap` is interpreted as `ldap --help`.

Shells do not nest. Typing another shell's root switches to it: inside `orch`,
`nh` leaves the `orch` shell and enters the `nh` shell, and `exit` returns to
root.

Shell controls such as `exit`, `quit`, and bare `help` stay REPL-owned. They
manage the shell rather than dispatching a normal command.

Host commands such as `doctor last -v`, `config`, and `history` keep their root
meaning inside integration shells. Scoped completion includes the same global
commands that dispatch accepts; you do not need to leave `[orch]` to inspect a
failure.

## History And Completion

The REPL provides:

- shell-like tokenization
- command and flag completion
- scoped completion inside shells
- history navigation
- `!!` repeats the last accepted command, including one that failed during
  execution; `sudo !!` repeats it with elevation. Tab expands these into the
  editor without running; Enter expands and runs them. Unknown commands and
  invalid syntax are excluded from saved history.
- saved-history expansion such as `!123`, `!-2`, and `!prefix`
- paged results: `next` and `prev`, or Shift+→ / Shift+← (also PageDown /
  PageUp) on an empty line; with text on the line, Shift+arrows move the cursor

Completion does not call remote services while you are typing. It works from
the known command catalog, config vocabulary, and already-available runtime
state.

See [COMPLETION.md](COMPLETION.md).

## Command Aliases

Use the `alias` wrapper for ordinary alias work instead of spelling config
keys by hand:

```text
alias add lsng 'ldap netgroup ${1} --value | P members'
alias list --sources
alias remove lsng
```

Templates are checked by the same placeholder, command, and DSL parsers used
when an alias runs. Positional placeholders such as `${1}` and `${@}` remain
unresolved until invocation.

Aliases follow config scope and storage rules. Without a configured
`config.default-target` or explicit scope/store flags, a one-shot CLI write
persists, while a REPL write lasts for the current session. `--permanent`
requests persistence; `--profile`, `--global`, and `--terminal` select the same
scopes as `config set` and `config unset`.

The root REPL is silent on exit by default. Set `repl.exit_message` when a
product or profile wants a sign-off:

```text
config set repl.exit_message 'So long, and thanks for all the fish!' --permanent
```

The message is printed for root `exit`, `quit`, or end-of-input. Leaving a
command shell keeps its existing `Leaving … shell` message instead.

## Config Writes Inside The REPL

Inside the REPL, `config set` defaults to session storage unless
`config.default-target` or explicit scope/store flags select another
destination. Use `--session` to force a temporary change and `--permanent` to
request persistence. [CONFIG.md](CONFIG.md#repl-config-writes) explains store
and scope selection.

```text
config set ui.format json
config set ui.format json --permanent
```

Use the first form when you want to experiment. Use `--permanent` when you have
decided the setting should become part of your stored config.

Theme, presentation, and prompt-related changes rebuild the REPL on the next
cycle so the new state becomes visible immediately.

## Prompt, Intro, And Input Mode

Useful REPL config keys:

- `repl.prompt`
- `repl.simple_prompt`
- `repl.shell_indicator`
- `repl.intro`
  - `none | minimal | compact | full`
- `repl.input_mode`
- `repl.tab_mode`
  - `tab | always | after_<n>_letters`; see [COMPLETION.md](COMPLETION.md)

Presentation presets also affect the REPL:

- `expressive`
- `compact`
- `austere`

Roughly:

- `repl.simple_prompt` controls prompt density
- `repl.intro` controls how much startup/help material appears
- `repl.input_mode` controls how ambitious the line editor should be

If the REPL feels unreliable in a weak terminal, `basic` is the first setting
to try for `repl.input_mode`.

## Practical Recipes

Run a batch in the current authenticated session:

```text
source changes.osp
source --ignore-errors follow-up.osp
```

Command files contain one command per line. Blank lines and lines beginning
with `#` are ignored. By default execution stops at the first failure and
reports its file and line number; `--ignore-errors` processes the rest.

Do a short built-in investigation:

```text
plugins list
plugins list --format md
help config
```

Change presentation temporarily for the current session:

```text
config set ui.presentation compact
config set repl.simple_prompt true
```

Inspect the most recent result, then keep slicing it locally:

```text
last
last --raw
last --format json
```

## When Not To Use The REPL

Prefer one-shot commands when:

- you are scripting or piping into other tools
- you need exact reproducible output in CI or shell scripts
- the command is a one-off and session state adds no value

In those cases, use ordinary CLI commands with explicit render flags such as:

```bash
osp --format json --render-mode plain plugins list
```

## Background notices

Products can use `osp_cli::repl::post_notice` for a short informational line
computed after startup. The editor prints it above the prompt; work posted
before the editor starts is queued. The basic reader also displays notices on
stderr, although it cannot redraw a partially typed line. Notice delivery does
not wait for network work or block the prompt.

### Product status

Native commands may supply `repl_status()` as cached plain text in a separate
right-prompt slot. Rendering never performs network I/O. The host sanitizes
control sequences and line breaks; Reedline measures terminal cells and omits
the right prompt when it collides with input. Context and incognito remain;
product attention takes priority over timing. Basic and JSON sessions omit it.
A cached update is visible at the next editor paint. The pinned Reedline reader blocks on terminal input: cache changes and queued
transition notices appear at the next input event, rather than waking an idle
reader. No fake input events or worker ANSI writes are used.

### Product actions and terminal ownership

The editor dispatches `ReplEvent::Line` for a submitted line and
`ReplEvent::Action` for a named action. Native products register `ReplAction`s
and implement `execute_repl_action`; the host uses ordinary elevation without
running an interactive login hook. Actions bypass user history, alias expansion,
shell navigation, last-command expansion and pagination reset. The same editor
retains its draft and byte cursor across normal return, cancellation and errors.

`repl.bindings.<action-id>` accepts F2 through F12 or `none`; conflicting keys
are rejected. Products supply a default and a human label. Basic and JSON
sessions have no product shortcuts. Existing Ctrl+R, Ctrl+L and pagination keys
keep their meanings.

`start_repl(config, Some(ReplSession))` supplies a cancellable rich-editor lifetime.
Basic selection and fallback report `None` for legacy startup behavior only;
recurring attention and sound remain disabled.
Workers use its bounded, sanitized `post_notice(text)` sink and
`wait(duration)` cancellation point. Reedline queues delivery while foreground
commands, line prompts, raw menus, pagers or the history picker own the terminal.
Rebuild, fallback and exit cancel the handle and discard queued product output.
Workers must also validate their own credential generation before publication.

### Terminal bells

`repl.notifications.sound = "off" | "bell"` is the master policy (library default
`off`; products can choose another default). `notifications preview` explicitly
exercises the same local sink without HTTP. It respects the master, rate limit
and machine-output suppression. Native commands request semantic feedback with
`NativeCommandContext::request_bell()`, flushed after foreground output. Workers
may request a best-effort bell through their current `ReplSession`. Background
bells are immediate and subject to the same master policy and rate limit.

BEL is best effort: the local terminal may play a sound, flash or ignore it,
including over SSH and tmux. OSP never runs an audio subprocess or configures
terminal passthrough. At most one BEL is emitted per ten seconds process-wide.
One-shot, basic/non-TTY and JSON output never emit event or preview bells.
Disable sound for this session with
`config set repl.notifications.sound off --session`. Muting never replays audio.

Intro templates and daily tips may use `{{actions}}` to show registered action
labels with their resolved keys; disabled and unavailable actions are omitted.
The root help surface uses the same resolved metadata.

A leading `|` replays the saved answer: `| F status=failed` means
`last | F status=failed` and makes no new request. Separate leading-pipe lines
each start from the saved answer, not the previous replay. Quoted literal pipes
remain ordinary command arguments.

Pasting `osp COMMAND …` with a known root command removes only `osp` and
returns the rest as editable input. Press Enter to execute it. Launcher flags
such as `osp -u USER …` or `osp --prod …` stay visible; these lines are not
normalized because their first word after `osp` is not a command root.

`each 'host {}'` runs once per `VALUE` value of the saved result, after its
pipeline. Values are shell-quoted into `{}`; put the placeholder outside quotes
inside the template. More than 50 values or a command path not ending in a recognized read
verb (such as host, info, find, list, get, or status) requires `--yes`. This
includes mutations and unclassified integration commands. Existing
command confirmations still apply. `--ignore-errors` continues after failures;
a final `N ok, M failed` tally is always printed after execution. Like `source`,
exit or reload stops the batch, and nested source/each commands are refused.

`help --all` lists every visible command path and description as ordinary rows.
Use `help --all | uptime`, `help --all | F command ~ task`, `help --all --json`,
or `help --all | Y` to filter, export, or copy the catalog. Hidden commands
and positional/value completion entries are excluded.
