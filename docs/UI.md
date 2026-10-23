# UI and Rendering

This guide explains how `osp` chooses output formats and how to configure
its presentation. For the implementation, start with `src/ui/mod.rs`.

## What the UI layer does

Rendering has one owner for each decision:

1. commands produce structured output; the DSL transforms canonical data
2. `plan_format` chooses the effective format and records its provenance
3. JSON, CSV and TSV dispatch directly to typed machine writers
4. human projection applies producer display rules, labels, limits and sanitization
5. `plan_layout` fits projected tables and chooses tables or MREG cards
6. lowering copies the chosen structure and typed styles into `ui/doc.rs`
7. terminal or Markdown emitters render that IR

General renderer rules use only shape and value type: row/column counts,
scalar versus nested values, measured width, and atomic tokens versus prose.
They never depend on field names or value meaning. Command-specific presentation
belongs to producer metadata (`column_labels`, `display_columns`, `DisplayRule`s),
never to reshaping canonical data. If the renderer needs to know what a field
means, the producer must declare the rule.

The UI does not reload configuration, rerun commands or invent data semantics.
Machine exports bypass human projection and the document IR. JSON stays free
of colour and retains canonical values, including every row and nested item.
REPL screen chrome may indent it; other targets have no margin.

## Format Selection

Supported formats:

- `auto`
- `guide`
- `json`
- `csv`
- `tsv`
- `table`
- `md`
- `mreg`
- `value`

Selection order is:

1. an explicit invocation format (`--format`, `--table`, `--mreg`, etc.)
2. a non-auto configured `ui.format`
3. one ordinary record overrides a Table recommendation with a classic MREG card
4. a command/plugin recommendation attached to the output
5. automatic inference from the output shape

`--format auto` permits recommendation and inference. An `auto` recommendation
also defers selection; a document recommendation without document intent does
not turn ordinary rows into a document. Configured defaults beat recommendations. A single top-level object or a
one-element record array becomes a classic card even with a table recommendation;
`--table`, `--format table` or configured `ui.format=table` keeps a table.
For example, `[{"Name":"example","Count":2}]` with a table hint is a card.
Other recommendations, including Markdown, Value, Guide and MREG, retain their
format for single records. The plan retains whether the
choice was explicit, configured, recommended or inferred.

Help and intro documents keep their document layout when
`ui.guide.default_format=guide`, unless `--format` is given for that invocation.
With `inherit` or `none`, they follow the configured format.

When `format=auto`, the current rules are:

- grouped output renders as `table`
- rows where every row only has a `value` field render as `value`
- zero or one ordinary row renders as `mreg` (value-only rows use `value`)
- larger row sets render as `table`

A top-level list of multiple rows that all contain exactly one scalar field with the same
key renders as a titled `Key (N):` grid for inferred, recommended or configured
human formats, except when MREG requests cards. The planner reuses the card grid-list geometry at the target's
width. An unmeasured screen uses a single 100-column fallback resolved in
settings; pager, editor, clipboard and pipe targets use natural, unlimited width.
Explicit `--table` and Markdown keep a table; explicit `--mreg` prints consecutive
`Key: value` lines without blank separators. Grid and entry values show embedded
line breaks as literal escapes. JSON, CSV, TSV and Value are unchanged.

An explicit `--mreg` or `--format mreg` always requests cards, including flat
multi-row and grouped output. Grouped cards carry each member's group keys and
aggregates; an empty group still has its context card. A producer recommendation
of MREG also requests cards. Inferred and configured MREG use cards for one row,
or for several rows only when every row contains a nested comparison table.
Their flat lists and grouped output remain tables unless the eligible density
comparison below selects cards.
When several flat records render as cards (explicit or recommended MREG, or automatic table
fallback), the planner selects compact cards. Flat means every value is a
scalar or an array of scalars, including reduced single-field object lists.
When every record's title and remaining nonempty fields fit together at the
available width, the planner uses one line per record: `title  label value`
with three spaces between subsequent pairs. Title, label and value columns
are sized across all records; the whole list uses this layout or the titled
layout together. Values use their projected, escaped display widths.
Otherwise each card uses its first field's value in a title-styled `value:`
heading at the effective margin, followed by nonempty fields in producer order,
without blank separators by default. Every body line is indented two spaces
under the heading; body width budgets include that indent. Records with only a
title value after omitting empty fields print `Label: value` entries instead.
A field is short when its label width plus two spaces plus its longest value
across all records fits half the available width. Consecutive short runs pack
row-major with one shared column count and label width for the whole result.
Each grid column sizes its values from their rendered widths across records,
including projected swatches. Labels and values have a two-space gap; grid
pairs have a three-space gap. A long field breaks the run, shares the first
grid column's label width and wraps at spaces (hard-wrapping tokens). Long scalar
arrays use the existing grid list geometry; short arrays are comma-joined.
Body geometry uses the resolved
screen width minus the margin and two-space indent. For example, at width 80
and margin 0:

```text
engageinternal.uiocloud.no:
  Comment      Vertkøy til bruk i engagelab/IPED/UV - Terje Kristian Lian
               Thoresen <t.k.l.thoresen@uv.uio.no>
  Nameservers  ns1.nrec.no, ns2.nrec.no
include.uiocloud.no:
  Comment      Terje Kristian Lian Thoresen <t.k.l.thoresen@uv.uio.no>
  Nameservers  ns1.nrec.no, ns2.nrec.no
```

Single records and lists containing objects or arrays
of objects retain the classic aligned cards, nested tables, lists and blank
separators. With two or more classic cards, each first field's value in producer
order becomes a title-styled `value:` heading at the effective margin. The field
is omitted from the body, which is indented two spaces and planned within that
width. A single classic record keeps its first field as a normal row, without a
heading. Producers can set optional `title_field` metadata to a literal record
field name. When that field is present, classic and compact cards always use
its value as a `value:` heading, including for one record, and omit it from the
body. A missing title field falls back to the usual card behavior. Curated
columns map the title field to its displayed label; an unselected field cannot
become a heading. Empty implicit titles show their field label when
retained. JSON, CSV/TSV and DSL values are unchanged.
**Indent = 2 columns per level, everywhere; never align across levels.**

1. Card title (`<value>:` heading, classic or compact) at the effective margin m.
2. The record's fields one level in (m+2 under a title; m when there is no title). Siblings at the same level share a value column; that column is sized to fit labels up to `ui.mreg.label_max` (default 18) display columns (label incl. its colon/separator). Labels longer than the cap do not widen the column: they render `label: value` (classic) / `label  value` (compact) with the normal single separator, unaligned. Compact cards' multi-column grid packing keeps working with this column rule.
3. Anything nested under a field — nested tables, grid lists, multi-line lists, nested objects — is a hanging block one more level in (field indent + 2), on the lines after the label line. Never aligned to the level-0 value column.
4. A list value that fits on ONE line within the card width stays inline as a normal value (`Groups:  usit-gsd-clients`, `Nameservers  ns1.nrec.no, ns2.nrec.no`). Only lists that need more than one line become a hanging block (label line `Roles (7):` then items at +2, grid layout as today).
5. Long scalar text values still wrap with continuation lines aligned to that field's value column (that is the same level, so allowed).

A single record always uses classic cards, including with a declared title and
an MREG recommendation. Fields use colons, one field per line, and list counts
appear only for multiple items. Compact packing is reserved for multiple records.

For each card, the widest scalar field line (including its indentation,
label and separator, excluding lists) determines the card width. Clamp it between
`ui.mreg.min_width` (default 40) and `ui.mreg.max_width` (default 80), then to the
available terminal width. The maximum wins when configured below the minimum.
These settings accept positive integers.
Lists stay inline only when their complete line fits that width; otherwise they
hang two spaces deeper using the shared grid geometry. Lists never widen a card.
Long scalar strings wrap at the maximum, with continuations under their own
value column. If a label exceeds the alignment cap and its value needs wrapping,
the value hangs two spaces deeper instead. Terminals wider than the maximum do
not alter card layout. These card limits also apply to unlimited targets.

`ui.mreg.gap` controls blank lines between consecutive cards, including automatic
fallback and one-line compact entries. The default `auto` keeps compact cards
dense and inserts one blank line between classic/nested cards. An integer from
0 to 10 overrides both styles. Single records, tables, card interiors and nested
blocks are unchanged; presentation presets leave the setting at `auto`.

A nested comparison table is a nonempty array of objects with at most eight
nonblank columns (`MAX_NESTED_TABLE_COLUMNS`); wider object arrays remain lists.
By default, cards omit empty fields after human projection and reduction, and
tables drop columns empty in every row. At UI verbosity 1 or higher (`-v`),
empty card fields and all-empty table columns remain visible. Verbosity leaves
the fitting, density comparison and footer policies unchanged.
Before human layout, object arrays drop fields that are empty (`null`, `""`,
`[]`, `{}`) in every item, unless `-v` keeps them. If exactly one field remains, the array becomes a
list of its values. For example, `[{"hostname":"ns1.nrec.no","TTL":null},
{"hostname":"ns2.nrec.no","TTL":null}]` displays as `ns1.nrec.no, ns2.nrec.no`
in table cells and as a list in cards, without a nested hostname header.
Records containing only scalars and scalar lists after reduction count as flat.
Remaining object arrays keep nested tables in cards. Top-level terminal table
cells summarize these arrays as `1 item` / `N items`; object cells show their
nonempty field count as `1 field` / `N fields`. These counts are atomic when
fitting and count as shortened values in the screen footer. Markdown keeps
complete cell values.
JSON, CSV, TSV and DSL data retain the original objects and empty fields.

`theme list` declares `DisplayRule::Swatch` for its nine palette colour fields.
With colour enabled, valid colour values become two-cell painted blocks before
width planning; invalid/empty values become empty. Plain, automatic NO_COLOR, editor,
clipboard, Markdown and Value output retain raw values. Machine exports always
retain canonical data. The generic renderer has no theme-column name rules.

Producers can compose a field's human value with `DisplayRule::Compose` and
mark its literal top-level name with `DisplayRule::Notice` to show an unlabelled
line above its card. JSON retains the original structured field.

Help and intro producers build `core::Document`: ordered sections,
paragraphs, named entries, lists, and data blocks. `auto` renders these as human
documents laid out for the active `ui.presentation`. Ordinary records never
become documents from field names such as `notes` or `commands`.

Unstaged `--json` emits the document object, for example
`{"blocks":[{"section":"Options","blocks":[{"entries":[{"name":"--json","description":"Emit JSON"}]}]}]}`.
With any pipe stage, JSON emits the resulting flat rows instead. Document rows
carry their section in a column; human rendering emits headings for consecutive
runs without changing row order. Shape-changing stages return ordinary data.
`guide` and `--guide` render human documents with the shared document renderer.

Typed document producers declare `SectionRole` and `EntryRole`; row-preserving
DSL stages carry `DocumentRowRole` alongside rows. `Document::from_help_text`
is the single adapter that parses English help text into roles: it recognizes
Commands, Usage, Common Invocation Options and Product options sections, and
option names beginning with `-` or `--`. The renderer itself never infers roles
from titles, option-like names or `name`/`description` data fields. Columns named
`command` have no implicit role; use `DisplayRule::Command` to style them.
Presentation roles are omitted from canonical JSON: decoding that export
restores content, while typed producers must declare presentation roles again.

## Render Modes

Render mode controls the backend:

- `plain`: ASCII-safe, no ANSI color, no Unicode box drawing
- `rich`: rich terminal rendering, color and Unicode when allowed
- `auto`: choose based on runtime conditions

In `auto` mode:

- non-TTY output falls back to `plain`
- a `dumb` terminal falls back to `plain`
- otherwise `auto` uses `rich`
- forcing `color=always` or `unicode=always` keeps the rich backend active

Plain mode is strict. Once plain mode wins, color and Unicode are both off
even if the terminal could support them.

## Color, Unicode, and Width

Useful one-shot flags:

- `--source-record`
- `--format`
- `--render-mode`
- `--color {auto,always,never}`
- `--no-color`
- `--unicode {auto,always,never}`
- `--ascii`

Useful persistent config keys:

- `ui.format`
- `ui.mode`
- `ui.color.mode`
- `ui.unicode.mode`
- `ui.width`
- `ui.width-max`

`ui.width-max` defaults to `-1`: no additional cap is imposed on the measured
terminal width. Set a positive maximum to fit output to that limit or the
narrower terminal. `0` also disables the additional cap. An explicit `ui.width`
is an upper width preference for terminal output. Piped output keeps full values
regardless of these terminal preferences.

## Render Targets

`RenderTarget` resolves destination policy once in `ui/settings`:

| Target | Width | Colour | Margin | Display limits | Automatic cards |
|---|---|---|---|---|---|
| REPL screen | Terminal | Per settings | `ui.margin` | Yes | Yes |
| One-shot screen | Terminal | Per settings | 0 | Yes | Yes |
| Pager | Unlimited | Per settings | 0 | No | No |
| Editor | Unlimited | Never | 0 | No | No |
| Clipboard | Unlimited | Never; ASCII | 0 | No | No |
| Pipe, file, non-TTY | Auto off; explicit wins | 0 | No | No |

Targets supply defaults for automatic colour and Unicode only. Explicit
`--color always|never` and `--unicode always|never`, or their configured modes,
win on screen, pager and pipe targets. Thus `--color always | less -R` retains
ANSI; `auto` on a pipe remains colour-free and ASCII. NO_COLOR disables automatic
colour; an explicit `always` still wins. Strict plain render mode stays plain.
Editor and clipboard always suppress colour; clipboard also stays ASCII.

Margin is REPL screen chrome for every format, including JSON. It never enters
one-shot output, pipes, files, pagers, editors or clipboard content.
The pager, editor and clipboard re-render structured results for their target;
they do not reuse fitted screen text. Plain-text command output is retained as
text when no structured result is available.

One-shot `ui.pager=auto` still pages when the screen render is taller than the
terminal, then shows the unlimited Pager render. The REPL never auto-pages.
JSON, CSV and TSV never page. When `PAGER` is unset, the default is `less -RS`
(colour and horizontal scrolling); a user-set `PAGER` is respected unchanged.

## Tables and MREG Layout

Table-oriented tuning lives here:

- `ui.table.border`
  - `none | square | round`
- `ui.table.nested_border`
  - `inherit | none | square | round`; defaults to `none`
  - applies only to tables nested inside an MREG record, such as
    `addresses (2):`; `inherit` follows `ui.table.border`
- `ui.table.overflow`
  - `clip | ellipsis | wrap | none`
  - defaults to `ellipsis`: each cell stays on one line, with embedded newlines
    shown as literal `\n` and long values ellipsized to the fitted column width
  - explicit `wrap` preserves complete values and embedded line breaks across
    as many lines as needed
  - `none` explicitly permits tables wider than the terminal
- `ui.table.too_wide`
  - `mreg | hide`; defaults to `mreg` in every presentation preset
  - `hide` drops trailing columns and emits the fit summary below
  - `mreg` compares eligible planned tables and cards by visible-value density;
    hidden columns alone do not force cards

Automatic card fallback requires **all** of the following: inferred Table
format, normal TTY output, a known width, a single ungrouped
top-level data table, enabled fitting (`ui.table.overflow != none`), and
one through 20 source rows (`MAX_FALLBACK_CARD_ROWS`, before display limiting).
A width hint without a TTY is insufficient. Explicit `--table`, configured `ui.format=table`,
producer-recommended tables, copy-safe output, documents/help and nested tables
never fall back. Their fit path retains column order, first-column protection and the combined fit footer.

The shared top-level and nested table fitter treats a column as atomic when
all its nonempty rendered cells contain no whitespace and measure at most 24
columns (`ATOMIC_MAX_WIDTH`). Atomic columns and the first column retain their
natural widths. Prose columns shrink to the smaller of their natural width and
the larger of 12 (`PROSE_MIN_WIDTH`) or their header width. Headers stay whole.
If those minimum widths do not fit, columns drop from the right in producer
order, with the `+N hidden: …` footer. Spare width grows prose and list columns
from left to right up to natural width. The first column may exceed the available
width.
Prose cells may be clipped, ellipsized or wrapped; atomic values remain whole.
`none` uses natural widths without fitting.

Within those guards, the planner compares values per rendered line. Table
lines include the header, physical row lines, three border lines when bordered,
and the physical fit-footer lines if needed. Table values are row count times
visible columns. Card lines are measured from the planned compact (or classic
nested) layout; card values are row count times all fields capable of nonempty
display. Cards win only when their density is strictly greater; ties keep the
table. `TABLE_HEADER_LINES` and `TABLE_BORDER_LINES` name the fixed table geometry.
For example, eight rows with eleven populated fields and seven visible columns
have 56 visible table values; the density denominator includes the header,
eight rows, three border lines and any wrapped fit footer. If complete cards need
24 lines, their density is `88 / 24 = 3.67`. A single
ordinary row already infers MREG before this comparison.

Table cells comma-join lists. Fitted list cells keep leading whole items and a
remaining count: `ns1.nrec.no +1`. Only when the first item cannot fit is that
item ellipsized in mixed prose/list columns. A column is a list column when at
least one projected cell is a scalar list (including a reduced single-field
object list) and every other nonempty cell is a list or an atomic value under
the rule above. Mixed prose/list columns remain prose. A list column's minimum
is the maximum across rows of the complete single value or first item plus its
` +N` remaining count, never below the header width. If that minimum cannot fit,
the column is hidden instead of cutting inside an item. Wrap mode measures the
longest item and prints one complete item per line; `none` retains natural widths.
Markdown keeps complete cell values and escapes
embedded newlines as `<br>`; JSON preserves the original typed values.
Terminal fitting measures the exact escaped representation for `none`, `clip`
and `ellipsis`, and the widest physical line for `wrap`.

Screen tables combine hidden columns and shortened visible cells in one footer:

```text
+2 hidden: Reserved host addresses, Unused addresses
61 values shortened in Comment, Nameservers
  full width: | LESS     all fields: --mreg     pick: | P Name,'Reserved host addresses'
```

Each summary appears only when applicable. Counts refer to cells shortened by
ellipsis, list remaining counts or structured item/field summaries; column
names retain producer order. Lines wrap to terminal width with two-space
continuations and are never cropped.
These are at most three logical entries; wrapping may add physical lines.
`| LESS` always re-renders at unlimited width. `all fields: --mreg` appears only
for top-level records where cards are an alternative, including nested records
and singleton tables. The pick suggestion uses the first column and the first
hidden column, or the most-shortened column when none is hidden; whitespace and commas in literal names are quoted for DSL.
Nested card tables show only the applicable summaries, without actions.
`-q` or `ui.help.level=none` suppresses the footer. Pager, editor, clipboard and
pipe targets preserve full widths and never show this footer.

Human formats automatically localize RFC3339 strings at seconds precision,
including nested data and document data blocks. `DisplayRule::Date` and
`DisplayRule::RelativeTimestamp` take precedence; `unix_timestamp_columns`
marks numeric Unix seconds. Pipe-oriented `value` output skips these timestamp
rewrites, including explicitly annotated numeric timestamps. Other declared
human display rules still apply. Derived totals see unlimited data before a
terminal display limit is applied; JSON/CSV/TSV always receive canonical data.

`table` is the dense grid view for many rows.

`mreg` is the more semantic key/value view that `osp` prefers for one-row
results. It is easier to scan for a single object, while `table` is better
for comparing many rows.

## Presentation Presets

`ui.presentation` seeds a coherent UI profile:

- `expressive`
- `compact`
- `austere`

`gammel-og-bitter` remains a compatibility alias for `austere`.

These presets do not hard-lock the UI. They seed canonical keys that still
sit on builtin defaults. Once you set an explicit value like `ui.mode` or
`ui.chrome.frame`, that explicit value wins over the preset.

In practice:

- `expressive` favors richer chrome and fuller help/intro behavior
- `compact` keeps the UI denser without going fully austere
- `austere` strips the UI toward quieter, plainer output

## Help, Messages, and Section Chrome

The main keys here are:

- `ui.help.level`
  - `none | tiny | normal | verbose` (default `normal`, adjusted by `-v`/`-q`)
- `ui.messages.layout`
  - See [CONFIG.md](CONFIG.md#uirepl-keys-current) for values and aliases.
- `ui.chrome.frame`
  - `none | top | bottom | top-bottom | square | round`
- `ui.chrome.rule_policy`
  - `per-section | shared`

What these do:

- `ui.help.level` controls how much help/detail is shown
- `ui.messages.layout` controls whether messages are grouped into sections or
  shown as plain indented groups or inline with minimal chrome
- `ui.chrome.frame` controls section framing for grouped messages. Help and
  document headings follow the selected help layout.
- `ui.chrome.rule_policy` controls whether grouped message rules are rendered
  independently or shared across sibling message sections

Documents, messages and completion share title/rule geometry: unknown width
uses 24 columns, the minimum is 12, surrounding whitespace and trailing colons
are normalized, and a full-width title adds no trailing space. Callers decide
when headings and closing rules appear; messages keep their own event/severity
model. Style colours resolve during emission through semantic tokens.

These settings change presentation only. They do not change the underlying
command data.

## Prompt and REPL Presentation

The full startup overview keeps a compact welcome and pipe cheat sheet; use
`| H` for the detailed DSL reference. Inline code is highlighted, while command
descriptions use the theme's muted text colour. `ui.margin` applies only to
REPL screen output, with no additional implicit REPL margin.

Grouped and compact messages retain event order. Adjacent untitled messages of
the same severity share a heading; a single message uses “Error” or “Warning”.
Embedders can use `MessageBuffer::push_titled` for contextual headings. Message
bodies and JSON error payloads are unchanged; titles are presentation metadata.

Products can supply local artwork with `App::with_intro_decoration`. The
callback receives resolved screen settings and returns trusted terminal rows;
it must do no service I/O or output writes. Empty rows leave only the text.
The renderer places artwork beside the opening paragraph and fits prose to
that column, while later sections retain the full width. Minimal intros,
exports and transformed data omit decoration. Pixels, palettes, terminal-size
thresholds and product identity stay with the product.

The REPL prompt and intro surface are controlled primarily by:

- `repl.prompt`
- `repl.simple_prompt`
- `repl.shell_indicator`
- `repl.intro`
  - `none | minimal | compact | full`
The important distinction is:

- `repl.simple_prompt` changes prompt density
- `repl.intro` changes how much startup/help material the REPL shows
- the selected theme owns prompt, completion, message, and value colors

## Clipboard and Copy Mode

Clipboard copy is opt-in:

- `--copy` on supported commands
- DSL `| Y`

`| LESS` explicitly pages output and `| EDITOR` opens a temporary document.
These sinks stack with `| Y` in any order, after all data verbs. With EDITOR,
copying uses the edited text. REPL Alt+L, Alt+E and Alt+Y re-render the last
staged result for their target without running commands or stages again; see
[REPL.md](REPL.md#shortcuts-and-terminal-bells).

Without EDITOR, copy rendering uses the plain-safe path even when visible output is
rich. That is deliberate. Clipboard content should be stable, pasteable text,
not ANSI-decorated terminal output.

## Practical Examples

Compact REPL defaults:

```toml
[terminal.repl]
ui.presentation = "compact"
repl.simple_prompt = true
repl.intro = "compact"
```

Quiet operator profile:

```toml
[profile.ops]
ui.presentation = "austere"
ui.mode = "plain"
ui.color.mode = "never"
ui.chrome.frame = "none"
ui.messages.layout = "minimal"
```

Force Markdown for one command without changing config:

```bash
osp --format md plugins list
```

Force plain JSON for scripting:

```bash
osp --format json --render-mode plain plugins list
```

## Fixed layout defaults

Help tables have no border, help entries use two-space indentation, and help
sections retain their default spacing. Nested output uses two-space indentation;
lists expand after five items. These layout details are fixed rather than
configurable. The right REPL prompt combines session context, incognito state,
and timing when applicable.
