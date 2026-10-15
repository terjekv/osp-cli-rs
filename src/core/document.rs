//! Semantic documents shared by help, intro, and structured command output.
//!
//! Documents describe content without prescribing its terminal or Markdown
//! layout. The UI lowers these blocks into its private rendering model.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Detail level attached to a document section.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelpLevel {
    /// Section is hidden at every help level.
    None,
    /// Section is shown in a compact synopsis.
    Tiny,
    /// Section is part of ordinary help.
    #[default]
    Normal,
    /// Section is shown only with verbose help.
    Verbose,
}

impl HelpLevel {
    /// Parses a configured help level.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "tiny" => Some(Self::Tiny),
            "normal" => Some(Self::Normal),
            "verbose" => Some(Self::Verbose),
            _ => None,
        }
    }
}

/// A semantic document whose blocks remain in authored order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// Ordered content blocks.
    pub blocks: Vec<DocBlock>,
}

impl Document {
    /// Projects blocks into canonical rows in document order.
    pub fn rows(&self) -> Vec<crate::core::row::Row> {
        fn append(blocks: &[DocBlock], section: &str, rows: &mut Vec<crate::core::row::Row>) {
            for block in blocks {
                match block {
                    DocBlock::Section { title, blocks, .. } => append(blocks, title, rows),
                    DocBlock::Entries { entries } => rows.extend(entries.iter().map(|entry| {
                        crate::row! { "section" => section, "name" => entry.name.as_str(),
                        "description" => entry.description.as_str() }
                    })),
                    DocBlock::Paragraph { text } => rows.extend(text.lines().map(|text| {
                        crate::row! { "section" => section, "text" => text }
                    })),
                    DocBlock::List { items } => rows.extend(items.iter().map(|text| {
                        crate::row! { "section" => section, "text" => format!("- {text}") }
                    })),
                    DocBlock::Data { value } => {
                        rows.extend(
                            crate::core::output_model::rows_from_value(value.clone())
                                .into_iter()
                                .map(|mut row| {
                                    row.insert("section".into(), Value::String(section.into()));
                                    row
                                }),
                        );
                    }
                }
            }
        }
        let mut rows = Vec::new();
        append(&self.blocks, "", &mut rows);
        rows
    }

    /// Keeps the unstaged document alongside its canonical row projection.
    pub fn to_output_result(&self) -> crate::core::output_model::OutputResult {
        use crate::core::output_model::{OutputDocument, OutputDocumentKind, OutputResult};
        let mut output = OutputResult::from_rows(self.rows());
        output.document = Some(OutputDocument::new(
            OutputDocumentKind::Document,
            serde_json::to_value(self).unwrap_or(Value::Null),
        ));
        output.meta.document_rows = true;
        output
    }

    /// Keeps sections available at `level`, preserving all other block order.
    pub fn filtered_for_help_level(&self, level: HelpLevel) -> Self {
        fn filter_blocks(blocks: &[DocBlock], level: HelpLevel) -> Vec<DocBlock> {
            blocks
                .iter()
                .filter_map(|block| match block {
                    DocBlock::Section {
                        title,
                        level: section_level,
                        blocks,
                    } if *section_level != HelpLevel::None && level >= *section_level => {
                        Some(DocBlock::Section {
                            title: title.clone(),
                            level: *section_level,
                            blocks: filter_blocks(blocks, level),
                        })
                    }
                    DocBlock::Section { .. } => None,
                    other => Some(other.clone()),
                })
                .collect()
        }

        Self {
            blocks: filter_blocks(&self.blocks, level),
        }
    }
}

/// A semantic document block.
///
/// The untagged serialized form uses each block's content key directly. This
/// keeps the JSON contract compact: a section is `{"section":"Options", ...}`
/// and an entries block is `{"entries":[...]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum DocBlock {
    /// A titled block group.
    Section {
        /// User-facing section title.
        #[serde(rename = "section")]
        title: String,
        /// Minimum help level required to include this section.
        #[serde(default, skip_serializing_if = "is_normal_level")]
        level: HelpLevel,
        /// Ordered content within the section.
        blocks: Vec<DocBlock>,
    },
    /// Free-form text.
    Paragraph {
        /// Paragraph contents.
        text: String,
    },
    /// Named entries such as commands, arguments, and options.
    Entries {
        /// Ordered entries.
        entries: Vec<DocumentEntry>,
    },
    /// A list of author-provided items.
    List {
        /// Ordered list items.
        items: Vec<String>,
    },
    /// Structured JSON data.
    Data {
        /// The JSON payload.
        value: Value,
    },
}

/// One named entry in a document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentEntry {
    /// Stable entry label.
    pub name: String,
    /// Description paired with the label.
    pub description: String,
}

fn is_normal_level(level: &HelpLevel) -> bool {
    *level == HelpLevel::Normal
}

#[cfg(test)]
mod tests {
    use super::{DocBlock, Document, DocumentEntry, HelpLevel};
    use serde_json::json;

    #[test]
    fn document_serde_uses_semantic_block_keys_unit() {
        let document = Document {
            blocks: vec![DocBlock::Section {
                title: "Options".to_string(),
                level: HelpLevel::Normal,
                blocks: vec![DocBlock::Entries {
                    entries: vec![DocumentEntry {
                        name: "--json".to_string(),
                        description: "Emit JSON".to_string(),
                    }],
                }],
            }],
        };
        let value = json!({
            "blocks": [{
                "section": "Options",
                "blocks": [{
                    "entries": [{"name": "--json", "description": "Emit JSON"}]
                }]
            }]
        });

        assert_eq!(serde_json::to_value(&document).unwrap(), value);
        assert_eq!(serde_json::from_value::<Document>(value).unwrap(), document);
    }
}
