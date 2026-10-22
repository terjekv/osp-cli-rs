# Plugins

An executable named `osp-NAME` adds the command `osp NAME`. Python scripts,
compiled binaries, and shell scripts all work. No registration is required.

```python
#!/usr/bin/env python3
import sys
print("hello", *sys.argv[1:])
```

Save this as `osp-hello`, make it executable, and put it on PATH or in
`~/.config/osp/plugins/`. Then `osp hello bob` prints `hello bob`.

## Discovery and management

Directories are searched in this order:

1. Each `--plugin-dir DIR`, in argument order.
2. Each directory in `OSP_PLUGIN_PATH`.
3. `~/.config/osp/plugins/` (the configured user config root).
4. `OSP_BUNDLED_PLUGIN_DIR`, then `plugins/` next to the binary and
   `../lib/osp/plugins/`.
5. PATH.

Names with file extensions are skipped after stripping Windows `.exe`, `.cmd`,
or `.bat` suffixes.

Builtins and native commands win name clashes. Among plugins, the first
executable found wins. `osp plugins list` shows name, about, source, path,
and status, including disabled entries and shadowed copies.

```bash
osp plugins list
osp plugins list --json
osp doctor plugins
osp plugins refresh
osp plugins disable hello
osp plugins enable hello
```

Enable and disable edit `extensions.plugins.disabled = ["hello"]` in the
active profile. Disabled executables are neither run nor described. Refresh
drops cached metadata; doctor shows inventory rows whose status is not `ok`.
A describe error does not prevent execution.

## Arguments, help, and the terminal

`osp hello bob` executes `osp-hello bob`. The command name is supplied as
`OSP_COMMAND=hello`, not as a leading argument. To share an executable across
several commands, install one symlink per name and read `OSP_COMMAND`.

Stdin and stderr are inherited. Stdout is inherited unless structured rendering
or a pipeline requires capture. The environment is inherited. Ordinary runs have
no timeout and retain the plugin's exit code. Ctrl-C reaches the foreground
plugin while the host waits. `osp hello --help` and `osp hello -h` always
run the executable's own help directly, so argparse and Click work normally.
Host invocation flags are consumed before `--`; place `--` before arguments
that must reach the plugin unchanged.

## Optional describe metadata

To add help text, REPL completion, authorization metadata, or automatic JSON
rendering, include the literal text `--osp-describe` in the executable. Only
executables containing that marker are invoked for metadata. On that call,
print one JSON object and exit successfully:

```json
{
  "about": "Greet people",
  "output": "json",
  "args": [{"name": "who", "multi": true}],
  "flags": {"--loud": {"about": "Shout", "flag_only": true}}
}
```

Every field is optional. `args`, `flags`, `subcommands`, and `auth` use the
`DescribeCommandV1` shapes in `osp_cli::core::plugin`; the top-level name comes
from the filename. Child commands include their names. Authorization is
inherited by child commands and checked by the same host policy as natives.

Describe runs have null stdin, captured stdout, and a time limit set by
`extensions.plugins.timeout_ms` (default 10000 ms). Metadata, marker scan
results, and errors are cached by executable path, size, and modification
time. Plain executables are never run speculatively. Listings, root help,
and REPL completion use the cached metadata.

`scripts/plugin_describe_from_argparse.py MODULE:ATTR` and
`scripts/plugin_describe_from_click.py MODULE:ATTR` emit starter metadata for
JSON-producing plugins. An argparse attribute can be a parser factory.

## JSON output and pipes

Stdout is captured when metadata sets `"output": "json"`, `--source-record` or an
osp format flag such as `--json` or `--format table` is supplied, or the command
has DSL stages.
Stdin and stderr remain inherited. Captured stdout is limited to 64 MiB;
capture ends when the direct child exits, even if descendants retain stdout.

Bare JSON arrays and objects become rows for the normal renderer and DSL:

```bash
osp hello bob alice
osp hello alice --json
osp hello bob alice --loud '|' 'F name=bob'
```

Captured text is printed as text. A DSL stage requires JSON; otherwise osp
reports `` `hello` printed text, not JSON; pipes need JSON output ``.
A nonzero captured run reports `error: Plugin hello crashed (exit status N).`
and keeps that exit status.

A JSON object with `protocol_version` is decoded as `ResponseV1`. Its data and
render hints retain their existing semantics. Message text
is shown at its declared level, with terminal controls stripped, newlines
flattened and a 512-character limit. Known secrets supplied through the plugin
environment are redacted in host-rendered structured fields only; inherited
plugin streams are not redacted. `meta`
may be omitted. An `ok: false` response is rendered as an error.

`ResponseMetaV1` in `src/core/plugin.rs` is the wire schema. Final output comes
from `data` through the shared renderer; `columns`, `column_labels`,
`display_rules`, `row_path` and `preserve_json_document` describe its projection.
A colour specimen can declare `{"kind":"swatch","field":"accent"}` in
`display_rules`: coloured human output shows a two-cell painted block, while
plain output and machine exports retain the raw colour value. Invalid colours
render empty in swatch mode. See [DSL authors](DSL_AUTHORS.md) for field rules.
When `data` is a display document shaped from a decoded server record, set the
optional `ResponseV1.source` to that original record. `--source-record` selects it
before rendering or DSL stages and ignores the display document's rendering metadata.
Omit `source` when `data` already is the source record; `--source-record` then leaves
the output unchanged. The REPL retains both documents for `last --source-record`.
One-line notices belong in levelled `messages`. The early-development protocol
has removed final-text overrides and string-only progress append metadata;
plugins must return structured data instead. There is no replacement final-text
override. Native streaming commands use the separate structured event channel
documented in [Embedding](EMBEDDING.md#native-commands); a subprocess response
remains one `ResponseV1` document.

## Environment and config

The full inherited environment is retained. The host also supplies:

- `OSP_COMMAND` and `OSP_USER` (the effective identity).
- `OSP_PROFILE`, `OSP_FORMAT`, `OSP_COLOR`, and `OSP_UNICODE`.
- `OSP_UI_VERBOSITY`, `OSP_DEBUG_LEVEL`, `OSP_TERMINAL`, and `OSP_TERMINAL_KIND`.

`extensions.plugins.env.NAME` becomes `OSP_PLUGIN_CFG_NAME` for every plugin.
`extensions.plugins.hello.env.NAME` overrides it for `hello`. Nonalphanumeric
separators become underscores and names become uppercase. Values within a
scope follow config-key order; later values win after normalization. Lists
are serialized as JSON. Secrets are unwrapped only at the process boundary.
Use `osp config get extensions.plugins` to inspect configuration.

## Complete example

[examples/plugins/osp-hello](../examples/plugins/osp-hello) uses argparse,
optional metadata, and bare JSON output:

```bash
install -D -m755 examples/plugins/osp-hello ~/.config/osp/plugins/osp-hello
osp hello bob alice --loud
osp hello --help
```

The same command is available in the REPL. `hello --l<TAB>` completes the
flag from its metadata. Running plain text plugins keeps their terminal
behavior; JSON plugins participate in the renderer and DSL.

## Command detail hints

`OSP_COMMAND_VERBOSITY` is an output-only numeric hint from 0 to 3. Existing
`OSP_UI_VERBOSITY` values remain unchanged; levels 2 and 3 both export `trace`.
Use the numeric hint when the executable has distinct request and evidence
views. Host flags before `--` belong to osp; plugin-owned flags go after it.

Structured messages preserve their text, levels and ordering. Failure summaries
include the plugin's reason. Terminal controls are stripped and messages are
bounded to one line of 512 characters; known environment secrets are redacted
in host-rendered structured fields only.

## Entity context

Descriptions may declare `"consumes": [{"kind": "account"}]` to receive the
current shared account as `OSP_CTX_ACCOUNT` inside the REPL. No context variables
are supplied to one-shot commands. A consumed positional may additionally name
`argument` and `fill: "read"`; writes use `fill: "explicit"` and require a typed
value or `.`. `fill: "narrow"` is reserved for scope flags that can only narrow
an explicitly named object's operation. Explicit arguments always win.

Declare `"produces": {"account": ["/username"]}` for a canonical field in a
successful singleton response. Alternative pointers must agree on one canonical
value; pointers through arrays require a single element. Lists select nothing.
Declared kinds in a
response's `context` map can acknowledge creation even when later follow-up
fails. The host owns selection, scope, prompt rendering, announcements and
clearing. A producer sets `meta.context_not_found: true` when the selected entity
is absent; the host clears only context used to fill a read. Native commands
attach the same `ContextContract` with clap `add`,
also available in `#[command(add = ...)]` derive attributes.
