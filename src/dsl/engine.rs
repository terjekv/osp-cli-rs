//! One executor over canonical rows and explicit group metadata.
//! Commands unwrap service collections before entering the DSL; raw response
//! documents are retained only for unstaged rendering.

use crate::core::{
    output_model::{
        DisplayRule, OutputItems, OutputMeta, OutputResult, compute_key_index, group_header_row,
    },
    row::Row,
};
use anyhow::Result;

use crate::dsl::verbs::{
    aggregate, collapse, filter, group, jq, limit, project, question, quick, sort, unroll, values,
};
use crate::dsl::{
    compiled::{CompiledPipeline, CompiledStage},
    model::RowSet,
    parse::pipeline::parse_stage_list,
};

/// Apply a pipeline to plain row output.
///
/// Use this when a command has already produced `Vec<Row>` and you want the
/// ordinary `osp` pipeline behavior without thinking about existing output
/// metadata.
///
/// This starts with `wants_copy = false` because there is no prior output meta
/// to preserve.
///
/// # Examples
///
/// ```
/// use osp_cli::dsl::apply_pipeline;
/// use osp_cli::row;
///
/// let output = apply_pipeline(
///     vec![
///         row! { "uid" => "alice", "team" => "ops" },
///         row! { "uid" => "bob", "team" => "infra" },
///     ],
///     &["F team=ops".to_string(), "P uid".to_string()],
/// )?;
///
/// let rows = output.as_rows().unwrap();
/// assert_eq!(rows.len(), 1);
/// assert_eq!(rows[0]["uid"], "alice");
/// assert!(!rows[0].contains_key("team"));
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn apply_pipeline(rows: Vec<Row>, stages: &[String]) -> Result<OutputResult> {
    execute_pipeline(rows, stages)
}

/// Apply a pipeline to existing output without flattening grouped data first.
///
/// Unlike `apply_pipeline`, this preserves the incoming `OutputMeta.wants_copy`
/// bit when continuing an existing output flow.
///
/// Use this when the command already produced an [`OutputResult`]. An empty
/// pipeline returns it unchanged. Transformations invalidate the original
/// document and presentation metadata; render and document-row metadata are retained
/// or rebuilt only where the stages support them.
///
/// # Examples
///
/// ```
/// use osp_cli::core::OutputResult;
/// use osp_cli::dsl::apply_output_pipeline;
/// use osp_cli::row;
///
/// let mut output = OutputResult::from_rows(vec![
///     row! { "uid" => "alice" },
///     row! { "uid" => "bob" },
/// ]);
/// output.meta.wants_copy = true;
///
/// let limited = apply_output_pipeline(output, &["L 1".to_string()])?;
///
/// assert!(limited.meta.wants_copy);
/// assert_eq!(limited.as_rows().unwrap().len(), 1);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn apply_output_pipeline(output: OutputResult, stages: &[String]) -> Result<OutputResult> {
    let mut output = output;
    let mut sinks = false;
    for stage in stages {
        let stages = resolve_stage_keys(&output, std::slice::from_ref(stage))?;
        let compiled = CompiledPipeline::from_parsed(parse_stage_list(&stages)?)?;
        for stage in &compiled.stages {
            if stage.is_sink() {
                sinks = true;
            } else {
                anyhow::ensure!(!sinks, "LESS, EDITOR and Y must follow all data verbs");
            }
        }
        output = run_compiled(output, &compiled)?;
    }
    Ok(output)
}

/// Lets keys be what a person reads in the table: a column label resolves to
/// its field, and a key no row has is an error naming the columns instead of
/// an empty or single-group result. Only plain keys of `F`, `S`, `G` and `P`
/// are checked; values, directions and aliases pass through unchanged.
fn resolve_stage_keys(output: &OutputResult, stages: &[String]) -> Result<Vec<String>> {
    let rows = crate::core::output_model::output_items_to_rows(&output.items);
    if rows.is_empty() {
        return Ok(stages.to_vec());
    }
    let labels = match (
        &output.meta.display_columns,
        &output.meta.display_column_labels,
    ) {
        (Some(columns), Some(labels)) => labels
            .iter()
            .zip(columns)
            .map(|(label, column)| (label.to_ascii_lowercase(), column.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        _ => std::collections::BTreeMap::new(),
    };
    let resolve = |key: &str| -> Result<String> {
        let bare = key.trim_start_matches(['!', '?', '=', '-', '+']);
        let prefix = &key[..key.len() - bare.len()];
        let spec = crate::dsl::parse::key_spec::KeySpec::parse(bare);
        if rows.iter().any(|row| {
            !crate::dsl::eval::resolve::resolve_values(row, &spec.token, spec.exact).is_empty()
        }) {
            return Ok(key.to_string());
        }
        if let Some(field) = labels.get(&bare.to_ascii_lowercase()) {
            return Ok(format!("{prefix}{field}"));
        }
        let columns = rows
            .iter()
            .flat_map(|row| row.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("no field '{bare}' in these rows; columns: {columns}")
    };
    stages
        .iter()
        .map(|stage| {
            use crate::dsl::parse::lexer::{
                Span, StageSegment, TokenKind, tokenize_stage, tokenize_stage_terms,
            };
            let tokens = tokenize_stage(&StageSegment {
                raw: stage.clone(),
                span: Span {
                    start: 0,
                    end: stage.len(),
                },
            })?;
            let Some(verb) = tokens.first() else {
                return Ok(stage.clone());
            };
            let verb = verb.text.to_ascii_uppercase();
            if !matches!(verb.as_str(), "F" | "S" | "G" | "P") {
                return Ok(stage.clone());
            }
            let mut replacements = Vec::new();
            let mut after_as = false;
            for token in tokens
                .iter()
                .skip(1)
                .filter(|token| token.kind == TokenKind::Word)
            {
                if after_as {
                    after_as = false;
                    continue;
                }
                if matches!(verb.as_str(), "G" | "S") && token.text.eq_ignore_ascii_case("as") {
                    after_as = true;
                    continue;
                }
                if verb == "S" && matches!(token.text.to_ascii_lowercase().as_str(), "asc" | "desc")
                {
                    continue;
                }
                if verb == "P" {
                    let raw = &stage[token.span.start..token.span.end];
                    let term_segment = StageSegment {
                        raw: raw.to_string(),
                        span: Span {
                            start: 0,
                            end: raw.len(),
                        },
                    };
                    let terms = tokenize_stage_terms(&term_segment)?;
                    let mut resolved = raw.to_string();
                    let mut changed = false;
                    for term in terms.iter().rev() {
                        let field = resolve(&term.text)?;
                        if field != term.text {
                            resolved.replace_range(
                                term.span.start..term.span.end,
                                &serde_json::to_string(&field)?,
                            );
                            changed = true;
                        }
                    }
                    if changed {
                        replacements.push((token.span, resolved));
                    }
                } else {
                    let resolved = token
                        .text
                        .split(',')
                        .map(|key| {
                            if key.is_empty() {
                                Ok(String::new())
                            } else {
                                resolve(key)
                            }
                        })
                        .collect::<Result<Vec<_>>>()?
                        .join(",");
                    if resolved != token.text {
                        replacements.push((token.span, serde_json::to_string(&resolved)?));
                    }
                }
                if verb == "F" {
                    break;
                }
            }
            // Rewrite only key spans: quoted values and their whitespace belong to
            // the DSL lexer and must survive byte-for-byte.
            let mut resolved = stage.clone();
            for (span, replacement) in replacements.into_iter().rev() {
                resolved.replace_range(span.start..span.end, &replacement);
            }
            Ok(resolved)
        })
        .collect()
}

/// Execute a pipeline starting from plain rows.
///
/// This is the lower-level row entrypoint used by tests and internal helpers.
/// Like `apply_pipeline`, it starts with `wants_copy = false`.
///
/// Prefer [`apply_pipeline`] for the common "rows in, output out" path. This
/// entrypoint starts a new output flow. Use [`apply_output_pipeline`] to continue
/// an existing flow under its stage-specific metadata rules.
///
/// # Examples
///
/// ```
/// use osp_cli::dsl::execute_pipeline;
/// use osp_cli::row;
///
/// let output = execute_pipeline(
///     vec![
///         row! { "uid" => "bob" },
///         row! { "uid" => "alice" },
///     ],
///     &["S uid".to_string()],
/// )?;
///
/// assert_eq!(output.as_rows().unwrap()[0]["uid"], "alice");
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn execute_pipeline(rows: Vec<Row>, stages: &[String]) -> Result<OutputResult> {
    apply_output_pipeline(OutputResult::from_rows(rows), stages)
}

pub(crate) fn run_compiled(
    mut output: OutputResult,
    compiled: &CompiledPipeline,
) -> Result<OutputResult> {
    if compiled.stages.is_empty() {
        return Ok(output);
    }
    // A transformed result describes the selected rows, never a stale service
    // envelope (whose total/cursors describe a different collection).
    output.document = None;
    let mut rows = RowSet::from(output.items);
    for stage in &compiled.stages {
        // Cleanup is structural only when it changes rows. In particular, a
        // no-op `?` must not discard a narrowed document's layout and render hint.
        if matches!(stage, CompiledStage::Clean)
            && rows.partitions.iter().all(|partition| {
                partition
                    .rows
                    .iter()
                    .all(|row| !row.is_empty() && !row.values().any(question::is_empty_value))
            })
        {
            continue;
        }
        // Presentation overrides describe the producer's untransformed view.
        // Once a real stage runs, only the transformed rows may be shown.
        output.meta.display_limit = None;
        output.meta.wants_copy |= matches!(stage, CompiledStage::Copy);
        output.meta.wants_page |= matches!(stage, CompiledStage::Page);
        output.meta.wants_editor |= matches!(stage, CompiledStage::Editor);
        let narrows = stage.quick_plan().is_some_and(|plan| {
            rows.partitions
                .iter()
                .any(|partition| plan.narrows(partition.rows.len()))
        });
        if narrows || !stage.behavior().preserves_render_recommendation {
            // Shape changes, including quick narrowing a lone object, derive
            // columns from the result. Field-keyed
            // formatting (relative times, timestamps) still applies to any
            // field that survives, so it stays.
            output.meta.render_recommendation = None;
            output.meta.display_notes.clear();
            output.meta.display_columns = None;
            output.meta.display_column_labels = None;
            output.meta.column_align.clear();
        }
        output.meta.document_rows &= !narrows
            && matches!(
                stage.behavior().semantic_effect,
                crate::dsl::compiled::SemanticEffect::Preserve
            );
        rows = apply_stage(rows, stage)?;
        match stage.synthesized_fields() {
            Some(created) => forget_formatting(&mut output.meta, &created),
            None => {
                output.meta.unix_timestamp_columns.clear();
                output.meta.display_rules.clear();
            }
        }
    }
    output.meta.grouped = rows.grouped;
    output.items = rows.into();
    output.meta.key_index = match &output.items {
        OutputItems::Rows(rows) => compute_key_index(rows),
        OutputItems::Groups(groups) => {
            compute_key_index(&groups.iter().map(group_header_row).collect::<Vec<_>>())
        }
    };
    Ok(output)
}

pub(crate) fn apply_stage(items: RowSet, stage: &CompiledStage) -> Result<RowSet> {
    if let Some(plan) = stage.quick_plan() {
        return items.map_rows(|rows| quick::apply_with_plan(rows, plan));
    }
    match stage {
        CompiledStage::Filter(plan) => filter::apply_set(items, plan),
        CompiledStage::Project(plan) => items.map_rows(|rows| project::apply_with_plan(rows, plan)),
        CompiledStage::Unroll(plan) => items.map_rows(|rows| unroll::apply_with_plan(rows, plan)),
        CompiledStage::Values(plan) => items.map_rows(|rows| values::apply_with_plan(rows, plan)),
        CompiledStage::Limit(spec) => limit::apply_set(items, *spec),
        CompiledStage::Sort(plan) => sort::apply_set(items, plan),
        CompiledStage::Group(plan) => group::apply_set(items, plan),
        CompiledStage::Aggregate(plan) => aggregate::apply_set(items, plan),
        CompiledStage::Collapse => collapse::apply_set(items),
        CompiledStage::CountMacro => aggregate::count_set(items),
        CompiledStage::Copy | CompiledStage::Page | CompiledStage::Editor => Ok(items),
        CompiledStage::Clean => items.map_rows(|rows| Ok(question::clean_rows(rows))),
        CompiledStage::Jq(expr) => jq::apply_set(items, expr),
        CompiledStage::Quick(_)
        | CompiledStage::Question(_)
        | CompiledStage::ValueQuick(_)
        | CompiledStage::KeyQuick(_) => unreachable!("quick family dispatched above"),
    }
}

/// Drops field-keyed formatting for fields a stage created: a count named
/// `created_at` is not a timestamp just because an input field was.
fn forget_formatting(meta: &mut OutputMeta, synthesized: &[String]) {
    if synthesized.is_empty() {
        return;
    }
    let created = |field: &String| {
        synthesized.contains(field)
            || field
                .strip_prefix('/')
                .and_then(|path| path.split('/').next())
                .map(|key| key.replace("~1", "/").replace("~0", "~"))
                .is_some_and(|key| synthesized.contains(&key))
    };
    meta.unix_timestamp_columns.retain(|field| !created(field));
    meta.display_rules.retain(|rule| match rule {
        DisplayRule::AddressRangeTotal { ranges, field, .. } => !created(ranges) && !created(field),
        DisplayRule::AddressCountRemaining {
            network,
            used,
            field,
        } => !created(network) && !created(used) && !created(field),
        DisplayRule::SuffixWhenTrue { field, when, .. }
        | DisplayRule::BlankWhenPresent { field, when } => !created(field) && !created(when),
        DisplayRule::Warning { field }
        | DisplayRule::ValueStyle { field, .. }
        | DisplayRule::NullText { field, .. }
        | DisplayRule::Date { field, .. }
        | DisplayRule::Decimal { field, .. }
        | DisplayRule::TextLimit { field, .. }
        | DisplayRule::Compose { field, .. }
        | DisplayRule::Notice { field }
        | DisplayRule::Hyperlink { field }
        | DisplayRule::Bytes { field }
        | DisplayRule::RelativeTimestamp { field }
        | DisplayRule::HideArrayItemsWhenTrue { field, .. } => !created(field),
    });
}

#[cfg(test)]
#[path = "tests/engine.rs"]
mod tests;
