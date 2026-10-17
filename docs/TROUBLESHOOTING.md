# Troubleshooting

Start here when commands, configuration, or terminal output behave unexpectedly.

Examples that use `inventory ...` below are illustrative provider-backed
commands. Replace them with a real plugin command from
`osp plugins list`.

## Start Here

These commands usually tell you the most with the least effort:

- `osp -v ...`
- `osp -vv ...`
- `osp -vvv ...`
- `osp -d ...`
- `osp config explain <key>`
- `osp doctor plugins`
- `osp plugins list`

Use these to identify the problem before changing settings.

Failures give one actionable error and an optional request ID and hint. Use
`-v` to inspect service/account/session selection, `-vv` to inspect exchanges,
and `-vvv` for additional response and cache evidence. Use `-d` for host
loading/dispatch, or `-dd` for additional host decisions and backtraces.

`doctor` checks this machine only. It cannot establish remote service health
or resolve an unreachable service. Check connectivity or your VPN for network
failures. JSON command data stays on stdout; diagnostics go to stderr.

For startup delays before the first REPL prompt or a one-shot command, run
`OSP_TRACE_STARTUP=1 osp` or `OSP_TRACE_STARTUP=json osp ...` and inspect the
phase tree on `stderr`. See [startup tracing](LOGGING.md#startup-tracing).

In the REPL, the normal loop is:

1. let the command fail normally
2. read the short error
3. rerun with `-v` or inspect the recorded failure with
   `doctor last -v`

For successful REPL output, use:

- `last`
  - replay the last successful result with its original pipeline
- `last --raw`
  - inspect the pre-pipeline payload

## Symptom: A Plugin Command Is Missing

Check `osp plugins list` for its path and status. The file must be executable
and named `osp-NAME`. Builtins and native commands win name clashes, then the
first executable in search order wins. Enable a disabled plugin with
`osp plugins enable NAME`; inspect issues with `osp doctor plugins`.

## Symptom: Plugin Metadata Times Out

`extensions.plugins.timeout_ms` limits optional `--osp-describe` calls.
Inspect `osp doctor plugins`, fix the describe hook, then run
`osp plugins refresh`. A describe failure does not prevent ordinary runs,
which have no host time limit.

## Symptom: The Wrong Profile Seems Active

If the command looks like it is hitting the wrong environment, make profile
selection explicit and compare behavior:

```bash
osp --profile tsd plugins list
osp config explain profile.default
```

`--profile` selects a configured profile; positional words always name commands.
An unknown explicit profile or environment-selected default fails before command
execution. Inspect the configured profile names instead of expecting fallback.

## Symptom: REPL Startup Or Editing Is Bad In A Weak Terminal

Set:

```bash
osp config set repl.input_mode basic --save
```

`basic` is the safest option for limited terminals, odd PTYs, and terminals
with weak interactive support.

If completion looks semantically wrong rather than visually wrong, also check
[COMPLETION.md](COMPLETION.md).

## Symptom: JSON Output Is Polluted

`osp` keeps machine output on `stdout` and diagnostics on `stderr`.

If a script sees invalid JSON:

1. redirect `stderr`
2. check whether a plugin is printing data to the wrong stream
3. retry with `-v` or `-vv`

Illustrative provider-backed example:

```bash
osp inventory host web-01 --json 2>debug.log
```

If the JSON becomes valid once `stderr` is redirected, the issue is stream
mixing rather than JSON formatting.

## Symptom: A Config Change Did Not Stick

Without a configured `config.default-target` or explicit scope/store flags,
REPL writes use session storage. Use `--session` to force a temporary change or
`--permanent` to request persistence. See
[config write destinations](CONFIG.md#repl-config-writes).

```text
config set ui.format json
config set ui.format json --permanent
```

To see which setting takes precedence, run:

```bash
osp config explain ui.verbosity
```

## Symptom: Themes Look Wrong

Start with:

```bash
osp theme list
osp theme show dracula
osp doctor theme
osp config explain theme.name
```

That usually answers whether the wrong theme won, a custom theme failed to
load, or a custom file overrode a builtin.

## More Help

- REPL usage: [REPL.md](REPL.md)
- Completion and history: [COMPLETION.md](COMPLETION.md)
- Invocation flags: [FORMATTING.md](FORMATTING.md)
- Config: [CONFIG.md](CONFIG.md)
- Plugins: [PLUGINS.md](PLUGINS.md)
