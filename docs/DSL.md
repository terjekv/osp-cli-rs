# DSL guide

The pipe DSL transforms command output before rendering. Every stage operates
on canonical rows. Producers may supply human headings for ordinary output,
while filters, projections and JSON keep the canonical field paths. The command
vocabulary remains owned by each product; DSL stages do not rename commands or
infer service facts.

## One-shot commands

Quote shell pipes so your shell passes them to `osp`:

```bash
osp theme list '|' F id=nord '|' C
osp --json theme list '|' P id '|' S id desc
osp 'theme list | F id=nord | C'
osp theme list '|' H F
osp theme list '|' 'F id=nord' '|' C
```

The same explicit `|` syntax works on built-in, native product and external
plugin commands that support structured output. In the REPL, write ordinary
pipes. `H` displays DSL help before running the command.
CLI help output also accepts stages, for example `osp --help '|' C`.
Shell-completion scripts and version text are not structured command output;
they reject pipe stages explicitly.

For a product task search whose JSON exposes `id` and `status.name`:

```bash
osp --json orch task find '|' F status.name=running '|' C
osp orch task find '|' P id,status.name,created_at '|' S -created_at
```

Use the actual raw field path returned by your command. `status` is not an
alias for `status.name`: use the nested path only when it exists in raw JSON.
A human-only `STALE` suffix does not change the canonical status or imply a
terminal state. Filter the raw `stale` boolean to select stale tasks.

## The row boundary

- An array supplies one row per element. Object elements retain their fields;
  scalar elements become `{value: ...}` rows.
- An object supplies one row, with nested objects and lists intact.
- A command can declare a collection field such as `items` through `row_path`.
  Its elements become rows once, before the pipeline. Filters do not see the
  wrapper's pagination fields.
- Unstaged JSON preserves an explicitly supplied raw document. Applying a
  pipeline produces a JSON array of the resulting rows, including `[]` and
  singleton arrays. Wrapper totals and cursors are discarded; they no longer
  describe the transformed result.
- Help and intro are semantic documents. Unstaged JSON emits `{"blocks": ...}`.
  A pipeline consumes flat rows in document order: `{section, name, description}`
  for entries and `{section, text}` for text lines. Preamble rows use an empty
  section. Data blocks retain their fields, with `section` reserved for the
  enclosing heading. Lists supply one row per text line.
- Document rows are marked by producer metadata, never inferred from row shape.
  `| L 2` selects the first two rows; `| S name` sorts entries across sections;
  `| F name~json` filters entries. `section` is searchable like any other column.
  Human rendering shows a heading where a run of rows with that section starts,
  in current pipeline order. It never restores a document by matching values.
- Row-preserving stages keep document-row metadata. Shape-changing stages
  (`P`, `A`, `C`, `G`, `VALUE`, `JQ`, `U`, `Z`, or cleanup that changes fields)
  clear it and render ordinary data. Every staged JSON result is a row array.

`F`, bare search, `K`, `V`, and existence searches retain complete matching
rows. They do not recursively remove members from arrays inside a row. For
example, filtering a row by `members[].uid=alice` retains that whole row.
To work on individual members, explicitly extract/unroll them first:

```text
P members[] | F uid=alice | C
U members | F members.uid=alice
```

`P members[]` emits member fields as rows; `U members` keeps parent fields
and replaces the list field with one member per row. `VALUE members[].uid`
emits plain `{value: ...}` rows in document order, preserving duplicates from
distinct addresses.

## Syntax and verbs

| Stage | Syntax and meaning | Example |
| --- | --- | --- |
| Bare search | Case-insensitive key/value search; narrows one object or filters multiple objects | `running` |
| `F` | `F field OP value`; operators `=`, `==`, `!=`, `>`, `>=`, `<`, `<=`, `~` | `F status.name=running` |
| `P` | `P field[,field...] [!field...]`; keep/drop fields | `P id,status` |
| `S` | `S field [asc\|desc] [AS num\|str\|ip] ...` | `S created_at desc id` |
| `G` | `G field [AS alias] [field ...]`; partition by group keys | `G provider` |
| `A` | `A count\|sum\|avg\|min\|max [field] [AS alias]` | `A sum memory AS total` |
| `L` | `L count [offset]`; negative count selects the tail | `L 10 20`, `L -5` |
| `Z` | Collapse group headers/aggregates into summary rows | `G provider \| A count \| Z` |
| `C` | Count the current selection | `F stale=true \| C` |
| `Y` | Mark output for copying | `P id \| Y` |
| `H` | `H [verb]`; show syntax and examples | `H S` |
| `V` | Value-only quick search | `V running` |
| `K` | Key-only quick search | `K requester` |
| `?` | No operand: remove null, empty string and empty array fields | `?` |
| `?field` | Keep rows with a truthy field; `!?field` selects absence | `?requester` |
| `U` | `U field`; unroll a list while retaining parent data | `U contacts` |
| `JQ` | Run a quoted jq expression | `JQ '.[] \| .id'` |
| `VALUE` / `VAL` | Extract selected fields as value rows; no operand extracts all | `VALUE status.name` |

The existing `VAL` spelling remains supported. No new verb synonyms are added.
Verbs are case-insensitive. An unknown single-letter verb is an error;
ordinary unregistered words are quick-search text.

Keys may use the column labels a table shows (`F state=failed` on a STATE
column resolves to its field). A key that matches no field in any row is an
error listing the columns, not an empty result or a single `null` group.
`U` leaves rows unchanged when its target path finds no array.
Shape-changing stages (`G`, `A`, `Z`) derive their columns from the result but
keep field formatting such as relative times for fields that survive.

## Matching and selectors

Quick search (`| term`) uses the number of rows entering that stage. With one
object, it keeps fields whose names match. In every other field, it keeps only
matching values and list items: scalar lists retain only matching items, and a
value match inside a list of objects retains that whole object. With multiple
objects, it filters rows and keeps each matching row whole.
Negation (`| !term`) removes matching fields or list items from one object, or
removes matching rows from multiple objects. Each group's member rows follow
this rule independently, including after earlier pipeline stages.

A term exactly naming a field projects only that field's complete value;
selectors can also project addressed objects or list members. `?field` and
`!?field` always filter by truthy presence or absence without narrowing fields.

Quick search supports these prefixes, with or without intervening spaces:

| Prefix | Meaning |
| --- | --- |
| `!text` | Remove matching fields/items or rows |
| `=text` | Case-insensitive equality |
| `==text` | Case-sensitive equality |
| `!=text` | Negated case-insensitive equality |
| `%text` | Fuzzy text matching |
| `?field` / `!?field` | Truthy presence / absence |

`K` and `V` restrict the search to keys or values. `K !=field` keeps unequal
keys in one object, or rows having a key unequal to that name. All whitespace
variants of `!=` have the same interpretation; predicate `F field != value`
also permits attached forms.
`F field text` means a contains comparison; `F field=value` means
case-insensitive equality and `F field==value` means case-sensitive equality.
Array-valued fields match if an element satisfies the predicate.

A bare literal field selector prefers the root field and its leaves, falling
back to descendant keys only when that root field is absent. Opt-in fuzzy key
search retains broader descendant matching. Dotted paths navigate nested
objects; when a named segment reaches an array it visits each member. Thus
`members.uid` and `members[].uid` select the same leaves. Use `[0]`, `[-1]`,
`[1:3]` or `[]` for indexed, tail, sliced or full traversal. Missing branches
contribute no values. Structural paths do not match a literal dotted key inside
an unrelated nested object.

Projection keepers and droppers resolve against original addresses before
compaction. A selected null remains null. Fanout columns use their leaf name;
selecting two fanouts with the same name is an ambiguity error. Static parent
fields repeat alongside projected member rows. Fanout columns join by member
address, so a missing field stays absent rather than borrowing another member's
value. Member rows follow their first appearance across the selectors.

## Sorting, dates and quoting

`S -created_at` and `S created_at desc` mean descending order; the existing
`!created_at` spelling also works. Cast and direction may appear in either
order, for example `S size desc AS num`. A missing sort field in a nonempty
selection is an error. Empty input sorts successfully. Missing values sort
last; numeric-looking values sort numerically under automatic casting.
Text sorting uses the same Unicode case folding as case-insensitive filtering.
Sorts, ordered filters and minimum/maximum compare supported integers exactly.
Comparisons involving floating-point values use floating-point ordering.

Ordered filters recognize RFC3339 timestamps and timezone-naive dates/times.
Naive values use the machine's local timezone. Ambiguous or nonexistent local
DST times are rejected with guidance to supply an offset. Prefer explicit
RFC3339 offsets in scripts. Comparisons preserve fractional seconds. A
date-only `<=` bound includes the whole local day; a `>=` bound starts at
local midnight:

```text
F created_at >= 2026-09-25T10:00:00+02:00
F created_at >= '2026-09-25 10:00:00'
```

Human output formats RFC3339 timestamps in local time with an explicit UTC
offset. Numeric timestamps are formatted only when the producer declares that
field as Unix seconds; arbitrary numbers are not guessed to be dates. Raw JSON
keeps the original values. Shape-changing stages clear producer formatting
hints so an alias cannot inherit an unrelated timestamp format.

Quote regex alternation inside a textual pipeline:

```bash
osp "theme list | F id ~ 'nord|dracula' | P id"
osp theme list '|' F id '~' 'nord|dracula' '|' P id
```

An unquoted `|` is always a pipeline boundary. Ambiguous regex-to-text-search
transitions fail with quoting guidance; use an explicit verb after an unquoted
regex, or quote the regex. Quote whole jq expressions too:

```bash
osp theme list '|' JQ '.[] | .id'
```

JQ receives an array of canonical rows, including singleton objects. Use
`.[0].field` or `.[] | .field`. Scalar results become `value` rows. For grouped
input, JQ receives each `{groups, aggregates, rows}` envelope; `.rows` accesses
members. Returning a replacement envelope updates that group; other results
replace its member rows.

## Groups and empty selections

`G` keeps group keys, aggregate values and member rows distinct. A field
holding a list of plain values groups like `field[]`: each element is its
own group, so a host with two siteadmins counts under both. A row joins each
group once even if a list repeats an element. An empty list joins the
null/missing-value group, as does explicit `field[]`. Group header names must
be distinct after aliases resolve. Lists of objects still need a leaf field.
Row operations such as `P`, `U`, `VALUE`, quick search and clean run within
each partition.
`F` tests matching header/aggregate fields first; otherwise it filters members
and removes empty groups. `S` and `L` order/limit groups. `A` adds one aggregate
per group; `Z` emits keys and aggregates as ordinary rows. Previously computed
aggregates remain snapshots until explicitly recomputed.

`C` emits one `{count: N}` row for ungrouped input and one key/count summary per
surviving group. If no groups survive, it emits `{count: 0}` without invented
group keys. `A count` follows the same empty-selection rule. Empty numeric
sum/average are zero; minimum/maximum are null. `G` alone over empty input
produces no groups.

`A count field` and `A count(field)` count non-null field values. Use `AS`
to name an aggregate, for example `A count AS total` to count all rows.
Integer-only sums stay integers while their total fits the JSON i64/u64 range;
larger totals and sums containing floating-point inputs use floating point.

## Human tables

Null and empty arrays render blank; lists render as readable comma-separated
values. Producer column order expresses priority. Narrow terminal tables omit
trailing low-priority columns before ellipsizing remaining values. Declared
columns remain stable for zero, one or many rows. Use `--json` for complete
machine data, or `P` to choose the fields needed for your task. Set
`ui.table.overflow=wrap` to preserve complete cell values across lines; the
explicit `none` setting disables width fitting.

Ordered `F` comparisons accept `now` and `now-Ns`, `now-Nm`, `now-Nh`,
`now-Nd`, for example `F created_at >= now-24h`. N is a nonnegative integer;
a day is exactly 86400 seconds. Each compiled filter samples the clock once,
and invalid or overflowing durations fail before filtering. These predicates
filter fetched rows only: they do not change server pagination or fetch history.
