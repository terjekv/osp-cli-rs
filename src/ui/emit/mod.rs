//! Final text emitters for the UI document IR.
//!
//! Human emitters consume the shared [`crate::ui::doc::Doc`] shape. JSON and
//! delimited machine writers consume typed canonical data directly and never
//! use the human IR.
//!
//! This layer should stay about rendering only. Format selection and semantic
//! lowering happen earlier in the UI pipeline.

pub(crate) mod delimited;
mod entries;
pub(crate) mod grid;
pub(crate) mod json;
mod key_value;
mod markdown;
pub(crate) mod shared;
mod table;
mod terminal;

use crate::core::output::OutputFormat;

use super::doc::Doc;
use super::settings::ResolvedRenderSettings;

pub fn emit_doc(doc: &Doc, format: OutputFormat, settings: &ResolvedRenderSettings) -> String {
    let mut settings = settings.clone();
    settings.width = settings
        .width
        .map(|width| width.saturating_sub(settings.margin));
    settings.margin = 0;
    match format {
        OutputFormat::Markdown => markdown::emit_doc(doc),
        _ => terminal::emit_doc(doc, &settings),
    }
}

/// Decoration occupies only the opening paragraph; subsequent sections keep their width.
pub(super) fn emit_with_leading_decoration(
    doc: &Doc,
    format: OutputFormat,
    settings: &ResolvedRenderSettings,
    decoration: &[String],
) -> String {
    let width = settings.width.unwrap_or(80).saturating_sub(settings.margin);
    let Some((super::doc::Block::Paragraph(opening), rest)) = doc.blocks.split_first() else {
        return shared::fit_prose(&emit_doc(doc, format, settings), width);
    };
    if decoration.is_empty() {
        return shared::fit_prose(&emit_doc(doc, format, settings), width);
    }
    let art_width = decoration
        .iter()
        .map(|row| super::display_width(row))
        .max()
        .unwrap_or(0);
    let mut narrow = settings.clone();
    narrow.width = Some(width.saturating_sub(art_width + 2));
    narrow.margin = 0;
    let text = shared::fit_prose(
        &emit_doc(
            &Doc {
                blocks: vec![super::doc::Block::Paragraph(opening.clone())],
            },
            format,
            &narrow,
        ),
        narrow.width.unwrap_or(80),
    );
    let text = text.lines().collect::<Vec<_>>();
    let mut rows = Vec::new();
    for index in 0..decoration.len().max(text.len()) {
        let art = decoration.get(index).map(String::as_str).unwrap_or("");
        let pad = " ".repeat(art_width.saturating_sub(super::display_width(art)));
        let line = text.get(index).copied().unwrap_or("");
        rows.push(format!("{art}{pad}  {line}"));
    }
    let mut rendered = rows.join("\n");
    rendered.push('\n');
    if !rest.is_empty() {
        if !matches!(rest.first(), Some(super::doc::Block::Blank)) {
            rendered.push('\n');
        }
        rendered.push_str(&shared::fit_prose(
            &emit_doc(
                &Doc {
                    blocks: rest.to_vec(),
                },
                format,
                settings,
            ),
            width,
        ));
    }
    rendered
}
