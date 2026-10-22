# ui

Turns structured output into text for a person or a pipe. Commands and the DSL
decide *what* the data is; this module decides *how it looks*: table, key/value,
documents, markdown or JSON, with theme, width, colour and unicode policy applied.

Imports `core` and `config` (render settings). It does not re-run
commands, re-resolve config, or invent data. If a value is wrong here, it was
wrong before it arrived.

## Flow

```text
OutputResult / Document
   ▼ plan_format   choose format, provenance and resolved settings
   ├ JSON/CSV/TSV  dispatch to typed machine writers
   ▼ projection    project human values, labels, styles, limits; sanitize once
   ▼ plan_layout   fit projected tables; choose grids, tables or cards
   ▼ lower.rs      copy the plan and projected content into doc.rs
   ▼ emit/         render terminal or Markdown text
```

## Layout

| Path | Owns |
|---|---|
| `plan/` | Format provenance, single-column grids, compact/classic MREG layout and table fitting/visibility |
| `projection.rs` | Human field/value projection, style annotations, limits, sanitization and table candidates |
| `doc.rs`, `lower.rs` | One human document IR and lowering according to the plan |
| `emit/` | Terminal/Markdown IR emitters and separate typed JSON/delimited writers |
| `settings/` | `RenderSettings` and resolving them from config |
| `theme/`, `theme_catalog.rs` | Builtin themes; loading custom theme files |
| `style/` | Style tokens mapped to theme colours |
| `messages/` | Buffered success/warning/error messages and their layout |
| `chrome/`, `section_chrome.rs` | Shared title/rule geometry; styled message/completion dividers and frames |
| `clipboard/` | Copy-to-clipboard transport |
| `prompt.rs` | Plain line input for command prompts and the basic REPL |

## Common changes

- **Wrong human value, label or total:** start in `projection.rs`. Check `--json`
  for the canonical data; derived display values precede display limits.
- **Wrong format, cards or hidden columns:** start in `plan/`. The fit function
  measures projected cell text, preserves atomic values and drops trailing columns; `lower.rs` and emitters consume its decision.
- **Wrong spacing, border or colour:** start in the emitter and `ThemeStyler`.
  The IR carries tokens; palette specimens alone retain validated raw specs.
- **New style role:** add the token in `style/`, map it in each builtin theme
  in `theme/`, document it in `docs/THEMES.md`.
- **New output format:** add format planning and `core/output.rs` vocabulary;
  human formats use the IR, while machine formats dispatch typed data directly.
- **New document behavior:** declare a producer role in `core::document` and
  carry it through projection/lowering. Never infer intent from names or titles.
- **New chrome:** reuse `chrome/` geometry. Callers own when chrome appears.

## Debugging

```sh
osp theme list
osp plugins list --format table --plain     # no colour, ASCII chrome
osp plugins list --json                     # the data before ui
```

Documents lower to the same UI blocks as ordinary data, including `EntriesBlock`.
Direct and help layouts share the block traversal, with help spacing and entry
hints applied before emission.
Marked document rows render consecutive section runs in their current order.
The renderer never infers document intent from ordinary row fields.

`MAX_NESTED_TABLE_COLUMNS` names the eight-column nested comparison-table rule.
`MAX_FALLBACK_CARD_ROWS` limits automatic table-to-card fallback to 20 source
rows. Eligible candidates compare visible values per rendered line; cards win
only at strictly higher density. `ui.table.too_wide=mreg|hide` selects automatic cards or the retained hide
path; explicit/configured tables, grouped output, documents, copy-safe sinks
and nested tables never fall back. See `docs/UI.md` for the full contract.

`RenderTarget` owns destination policy in `settings/`: only REPL screens retain
margin; pager/editor/clipboard/pipe targets have no width or display limits.
The host retains staged structured output for action re-renders, rather than
reusing a fitted screen string. Emitters apply the resolved margin uniformly,
including machine output on the REPL screen.

`plan/cards.rs` owns compact flat-list cards: ruled first values, globally sized
short-field grids and wrapped long fields. Single and nested records retain
classic cards. Lowering copies planned grids into the data-only IR.
