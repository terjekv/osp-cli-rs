# Logging, Verbosity, and Debugging

`osp` separates user-facing message detail from developer-facing debug output.

## User-Facing Verbosity

Use:

- `-v`
- `-vv`
- `-vvv`
- `-q`
- `-qq`

Behavior:

- default: command results, warnings, actionable errors and server request IDs
- `-v`: selected service, account, profile and credential source; command totals
- `-vv`: actual exchanges, response status, request IDs and elapsed time
- `-vvv`: response size, directory projection and cache age/lifetime evidence
- `-q`: fewer nonessential messages
- `-qq`: errors only

The levels are cumulative and saturate at `-vvv`. Configured `ui.verbosity`
values stay supported; `evidence` follows `trace`. Counts add to the configured
base. Per-line REPL flags restore the session baseline after each command.
Help detail remains command output on stdout.

Service diagnostics and human field supplements go to stderr, including with
JSON and output pipelines. Verbosity does not create service requests or enable
a disabled REPL introduction. Unknown measurements are omitted.

Failures use the same presentation in CLI and REPL:

```text
error: The operation failed.
request-id: req-123
hint: Check the selected account.
```

The request ID and hint are optional. There is at most one hint. Local parse
errors have no service context or elapsed-time summary. JSON failures retain
`ok`, `error`, `exit_code`, `messages` and `data`; the next action is available
as `error.details.action`.

## Developer Debug Output

- `-d`: compact startup, handler selection, outcome and phase times
- `-dd`: additional host decisions and captured error backtraces
- `-ddd`: host trace events with relative elapsed time

Host diagnostics use `debug:` and `trace:` prefixes. Service traffic belongs
to `-vv`, independently of `-d`. Dependency logging is not enabled by repeated
`-d` flags. File logs keep timestamps and sanitized event names and fields.

Plugin stderr is drained concurrently. Unverified prose is withheld; `-dd`
reports that the plugin wrote it. This also applies to plugin help. Executable
stdout, stdin, exit status and inherited environment retain their contracts.
The REPL's debug timing badge remains available.

## Timing Hints

At debug levels, the CLI's `Command complete` event includes startup and dispatch
times. The REPL's right-hand prompt shows the last command timing, independently
of the command's service totals. Its color bands are:

- `0-250ms`: green
- `250-1000ms`: warning
- `>1000ms`: red

## Startup Tracing

Set `OSP_TRACE_STARTUP=1` to write a human-readable phase tree to `stderr`, or
set it to `json` for one JSON document. The setting is read once when the
process starts; it is off by default. The timing report is separate from `-d`
developer logs and does not change their format. Startup phases are tracing spans
and also provide context in developer logs.

Each phase reports its start offset, elapsed duration, nesting depth, parent,
and share of the total. Phase durations are inclusive, so nested phase shares
can add up to more than 100%. A REPL trace ends at the final callback that
assembles its first prompt, before the editor waits for input. A one-shot trace
ends as the process wrapper returns and includes command execution.

```bash
OSP_TRACE_STARTUP=1 osp
OSP_TRACE_STARTUP=json osp --defaults-only config show 2>startup.json
```

Use this trace to find repeated config passes, plugin discovery and `--osp-describe`
subprocesses, or work that runs before the command is known.

## REPL Behavior

In the REPL, `-v/-q/-d` are command-local. They do not permanently change the
shell’s behavior unless you change config defaults. Launch flags set the
session baseline; per-line flags add to that baseline for one command. The
stderr override leaves file logging unchanged.

```text
plugins list -v
plugins list -vv
plugins list -dd
```

## Config Keys

Useful config keys:

- `ui.verbosity`
- `debug.level`
- `log.file.enabled`
- `log.file.path`
- `log.file.level` (default `info`, independent of `-d`)

Use `config explain <key>` to see where an effective value comes from.

Use `-vvv -dd 2>osp.txt` to collect service evidence and host diagnostics.
`doctor last -vvv` reads the retained command evidence without replaying a
request; `doctor last -dd` shows its captured backtrace. Retained command labels
omit arguments. No last-command record is persisted by one-shot commands.

`doctor` checks local configuration, selected session bindings, plugin inventory
and trust-file readability. It does not log in or contact services. A present
credential has not been verified against its server. Detailed config/plugin/theme
subcommands and the complete JSON report remain available.

## Plugin Runtime Hints

`osp` exports logging-related hints to plugins; it never reads these names
as configuration inputs:

- `OSP_UI_VERBOSITY`
- `OSP_DEBUG_LEVEL`
- `OSP_COMMAND_VERBOSITY` (numeric `0` through `3`)

`OSP_UI_VERBOSITY` keeps the plugin vocabulary: both command levels 2 and 3
export `trace`. The numeric hint distinguishes them. Plugins should keep result
data on stdout and avoid putting credentials in any output.

## Scripting Safety

`--format json` stays safe for scripts even when `-d` is enabled because data
stays on `stdout` and diagnostics stay on `stderr`.

```bash
osp plugins list --json -d 2>debug.log
```

Configured `debug.level` applies from configuration resolution onward. Use
`-d` to include earlier startup events. Counts add to the configured base and
saturate at level 3. File logging is independent of command-local stderr.
