# config

Answers three questions the same way everywhere: which keys are legal, which
source wins, and which file edits are allowed. Runtime resolution, `config
explain` and `config set --save` all go through the same code, so they cannot
disagree.

Builtin declarations reuse the owning output and REPL enum vocabularies and
the plugin describe timeout constant. This module does not render output.

## Layout

| File | Owns |
|---|---|
| `core.rs` | Keys, values, layers, scopes, and schema validation |
| `builtin.rs` | Builtin key declarations: schema, docs, bootstrap rules, and defaults |
| `defaults.rs` | Computed defaults, shared runtime fallbacks, and layer assembly |
| `loader.rs` | Source loaders (file, env `OSP__…`, secrets, in-memory) and the pipeline |
| `bootstrap.rs` | Choosing the active profile and terminal once per resolution |
| `selector.rs` | Scope precedence: profile+terminal > profile > terminal > global |
| `resolver.rs` | Picking a winner per key and applying the schema |
| `interpolate.rs` | `${…}` placeholders, after winners are chosen |
| `explain.rs` | The trace behind `config explain` |
| `runtime.rs` | Path discovery, runtime defaults and loader-pipeline bootstrap |
| `store.rs` | Editing TOML files in place with the same scope rules |
| `secrets.rs` | Secret backends |

Product crates put their keys under `extensions.<product>.*` and use this
module as is. They do not keep a parallel config model.

## Common changes

**Add a builtin key**

1. Declare the key in `builtin.rs` with its schema, doc, bootstrap rule, and
   optional literal default. Reuse the owning enum's `ALL` and `as_str()` for
   canonical values; keep explicit schema lists when they include aliases. `config set <key> ⇥`
   then completes those values.
2. Keep computed user, path, and theme defaults in `defaults.rs`.
3. Read it where it is used, through the resolved config.
4. Document it in `docs/CONFIG.md` or the owning area's doc.

**Change precedence:** only in `selector.rs` or `resolver.rs`, and check that
`config explain` still tells the same story.

## Debugging

```sh
osp config explain repl.tab_mode
osp config show --sources
OSP__REPL__TAB_MODE=always osp      # one-off env override
```

Retired config keys are ignored before interpolation and validation. The host
prints one warning naming them and `osp config unset KEY` guidance. Truly unknown
keys still fail validation. The retired-key list belongs to the config schema.
