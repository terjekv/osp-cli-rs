# DSL author notes

The execution model is `RowSet`: partitions of rows with group keys and
aggregates kept separately. Ungrouped data has one partition even when empty.
The public `OutputItems::Rows/Groups` variants are presentation boundaries;
they are converted once on entry and once on exit. They must not select
parallel implementations of verbs.

## Ownership and normalization

- `cli/rows/output.rs` unwraps producer-declared collections once. Human column
  hints never rename, remove or convert canonical values. A raw document can
  survive unstaged JSON rendering, but no staged service result retains its
  stale pagination/total envelope.
- `core::Document` supplies flat rows once, before key validation.
  Entries carry `section`, `name`, and `description`; text carries `section` and
  `text`. Output metadata declares document intent. Preserve the current row
  stream and never reconstruct sections by value equality. `DocumentRowRole`
  travels with rows through preserving filters/sorts/limits; shape-changing
  stages clear document intent and its roles. Producers set `SectionRole` and
  `EntryRole` explicitly; titles and entry spellings carry no renderer policy.
- `dsl/engine.rs` compiles and dispatches each stage once over `RowSet`.
- Verbs own algorithms over rows/partitions. Group keys and aggregates are
  deliberately metadata, not duplicate fields inserted into members.
- `ui/projection.rs` applies human-only producer field projection and timestamp
  hints. `unix_timestamp_columns` identifies Unix seconds without knowing
  product field names. `display_rules` supports literal string suffixes gated
  by a true boolean (`SuffixWhenTrue`) and blanking gated by another field's
  non-null presence (`BlankWhenPresent`), binary byte units for numeric fields
  (`Bytes`), and compact relative RFC3339 or Unix-second timestamps
  (`RelativeTimestamp`). Use producer display rules rather than UI formatting
  helpers in product code. Human formats also localize ordinary RFC3339 strings;
  Value format keeps raw timestamps and skips date, relative-time and Unix
  timestamp rewrites. Conditions always read the
  original row; only explicitly derived display rules synthesize fields.
  Derived totals see unlimited input before display limiting. JSON preserves raw
  values, and filters use canonical values before these display changes.

`DisplayRule::Command { field }` explicitly marks a command field for styling;
a column named `command` has no implicit role. Warning and command roles remain
style tokens until emission. `ValueStyle` retains a validated raw palette spec
for producer specimens; projected cells keep their own source styles across
labels, grouping, array filtering and display limits.

`DisplayRule::Swatch { field }` declares a colour specimen. With colour enabled,
human terminal output projects a valid colour/style value to `██` painted with
its foreground colour; invalid, empty or modifier-only specifications project
to empty text. Width planning measures the two-cell block. Plain, editor,
clipboard, NO_COLOR, Markdown and Value output keep the raw value. JSON/CSV/TSV
remain canonical. The serialized rule is `{"kind":"swatch","field":"accent"}`.
Like other field rules, it survives row-preserving stages and is cleared for
synthesized fields or shape-changing stages. Producers declare swatches; the
renderer never recognizes colour columns by name.

`DisplayRule::Hyperlink { field }` marks a string field whose value is an
HTTP(S) URL. The renderer emits OSC 8 only for rich TTY output. Plain output,
JSON, Markdown, CSV/TSV, pipelines and `last` replay keep the plain URL; product
text remains sanitized. The marker never changes canonical values.

`NullText`, `Date`, `Decimal`, and `TextLimit` format scalar fields only at
the human rendering boundary. `Compose` joins ordered literal and relative
field spans from a typed object or array into one displayed value. Apply scalar rules
before composition. These rules accept the same field selectors as curated
columns, never replace the JSON document, and never interpolate values in the
command adapter. A shape-changing stage clears them; synthesized fields also
lose any formatting attached to the overwritten input field. Table output
with declared columns retains its headings for an empty collection without
manufacturing a result row.

A stage that changes shape clears curated columns, alignment, numeric timestamp
hints, conditional display rules and renderer recommendations. Guide layout
survives only row-preserving stages; cleanup (`?`), projection, aggregation and
JQ expose their resulting row shape.

## Selector contract

All inputs use the same selector resolver. Bare keys search descendants;
structural paths use addressed traversal. Named segments descend through arrays,
so `a.b` and `a[].b` resolve equivalent leaves. Indexed/sliced selectors retain
original addresses until projection is complete. Missing branches contribute
nothing; selected null and duplicate values are retained.

Filtering retains complete canonical rows. It does not switch to recursive
member pruning when an object has a raw JSON sidecar. Callers that want to
filter members first expose them as rows with a declared collection boundary,
`P collection[]`, or `U collection`.

`P` resolves all keepers/droppers against original addresses, then rebuilds and
compacts sparse arrays. Dynamic projection labels must be unambiguous. `VALUE`
extracts leaves as flat value rows and never reconstructs service envelopes.

## Group semantics

- `G` partitions rows; regrouping operates on existing partitions.
- Row stages map the same algorithm over partitions and preserve metadata.
- `F` tests group headers when its selector resolves there, otherwise filters
  member rows and drops empty groups.
- `S` and `L` operate on group headers/partitions.
- `A` stores aggregate results separately; existing aggregates are snapshots,
  not silently recomputed when later member filters run.
- `C` emits key/count summaries and `Z` emits keys/aggregates.
- Empty `C` and empty `A count` produce one zero summary even after all groups
  have disappeared. They do not invent a group key. Empty `G` remains empty.
- JQ receives the row array, or each explicit group envelope. There is one jaq
  evaluator; its result is normalized at that explicit boundary.

## Parsing and verification

The lexer shares one quote/escape scanner for stage splitting and tokenization.
Quote regex/JQ pipes. One-shot argv separates literal pipe tokens before command
parsing so built-ins and product/plugin commands share operand handling.

Use the existing library, contract, integration and end-to-end suites. Existing
JSON fixtures must enter through the actual command adapter and renderer;
fixture helpers must not reintroduce the removed document executor. Preserve
all existing test functions/coverage when updating intended contracts. Do not
add regression tests. Compile and run checks on the internal builder.

See [DSL.md](DSL.md) for the user contract and [CONFIG.md](CONFIG.md) for explicit
profile selection. Product command vocabulary and server-owned facts belong to
the product/API owner, not to DSL aliases or renderer inference.
