//! Markdown templates lower directly to semantic blocks; includes are passive.

use super::{DocBlock, Document};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub(super) fn parse(template: &str, help: &Document) -> Document {
    let mut document = Document::default();
    let mut section: Option<(String, Vec<DocBlock>)> = None;
    let mut text = String::new();
    let mut code: Option<String> = None;
    let mut items: Vec<String> = Vec::new();
    let mut list = Vec::new();
    for event in Parser::new_ext(template, Options::all()) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                finish_section(&mut document, &mut section);
                section = Some((text.trim().to_string(), Vec::new()));
                text.clear();
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code = Some(match kind {
                    CodeBlockKind::Fenced(language) => language.to_string(),
                    _ => String::new(),
                });
                text.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                let block = if code.as_deref() == Some("osp") {
                    serde_json::from_str(&text)
                        .ok()
                        .map(|value| DocBlock::Data { value })
                } else {
                    None
                }
                .unwrap_or_else(|| DocBlock::Paragraph {
                    text: text
                        .lines()
                        .map(|line| format!("`{line}`"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                });
                if let Some(item) = items.last_mut() {
                    append_item(item, &text);
                } else {
                    push(&mut document, &mut section, block);
                }
                code = None;
                text.clear();
            }
            Event::Start(Tag::Item) => {
                if let Some(item) = items.last_mut() {
                    append_item(item, &text);
                }
                text.clear();
                items.push(String::new());
            }
            Event::End(TagEnd::Item) => {
                if let Some(mut item) = items.pop() {
                    append_item(&mut item, &text);
                    text.clear();
                    if let Some(parent) = items.last_mut() {
                        parent.push_str("\n  - ");
                        parent.push_str(&item.replace('\n', "\n  "));
                    } else {
                        list.push(item);
                    }
                }
            }
            Event::End(TagEnd::List(_)) if items.is_empty() => {
                push(
                    &mut document,
                    &mut section,
                    DocBlock::List {
                        items: std::mem::take(&mut list),
                    },
                );
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(item) = items.last_mut() {
                    append_item(item, &text);
                } else if matches!(text.trim(), "{{ help }}" | "{{ overview }}") {
                    finish_section(&mut document, &mut section);
                    document.blocks.extend(help.blocks.iter().cloned());
                } else if !text.trim().is_empty() {
                    push(
                        &mut document,
                        &mut section,
                        DocBlock::Paragraph {
                            text: text.trim().to_string(),
                        },
                    );
                }
                text.clear();
            }
            Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                text.push_str(&value)
            }
            Event::Code(value) => {
                text.push('`');
                text.push_str(&value);
                text.push('`');
            }
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::Start(Tag::Emphasis) | Event::End(TagEnd::Emphasis) => text.push('*'),
            Event::Start(Tag::Strong) | Event::End(TagEnd::Strong) => text.push_str("**"),
            Event::Start(Tag::Strikethrough) | Event::End(TagEnd::Strikethrough) => {
                text.push_str("~~")
            }
            _ => {}
        }
    }
    finish_section(&mut document, &mut section);
    document
}

fn append_item(item: &mut String, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if !item.is_empty() {
        item.push('\n');
    }
    item.push_str(text.trim());
}

fn push(document: &mut Document, section: &mut Option<(String, Vec<DocBlock>)>, block: DocBlock) {
    if let Some((_, blocks)) = section {
        blocks.push(block);
    } else {
        document.blocks.push(block);
    }
}

fn finish_section(document: &mut Document, section: &mut Option<(String, Vec<DocBlock>)>) {
    if let Some((title, blocks)) = section.take()
        && !blocks.is_empty()
    {
        document.blocks.push(DocBlock::section(title, blocks));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_template_parses_headings_includes_lists_and_inline_markdown_unit() {
        let help = Document::from_help_text("Usage: demo\n");
        let document = parse(
            "# Title\n\nHello _there_ and **strong**.\n\n{{ help }}\n\n- first\n- second\n",
            &help,
        );
        assert!(
            matches!(&document.blocks[0], DocBlock::Section { title, blocks, .. }
            if title == "Title" && matches!(&blocks[0], DocBlock::Paragraph { text } if text == "Hello *there* and **strong**."))
        );
        assert_eq!(document.blocks[1], help.blocks[0]);
        assert!(
            matches!(&document.blocks[2], DocBlock::List { items } if items == &["first", "second"])
        );
    }

    #[test]
    fn markdown_template_parses_valid_osp_code_blocks_as_semantic_data_unit() {
        let document = parse(
            "## Keybindings\n\n```osp\n[{\"name\":\"Ctrl-D\",\"description\":\"exit\"}]\n```\n",
            &Document::default(),
        );
        assert!(
            matches!(&document.blocks[0], DocBlock::Section { title, blocks, .. }
            if title == "Keybindings" && blocks == &[DocBlock::Data { value: json!([{"name":"Ctrl-D","description":"exit"}]) }])
        );
    }

    #[test]
    fn markdown_template_keeps_non_semantic_or_invalid_code_blocks_literal_unit() {
        for (text, expected) in [
            (
                "```json\n[{\"name\":\"Ctrl-D\"}]\n```",
                "`[{\"name\":\"Ctrl-D\"}]`",
            ),
            ("```osp\n[{name:\"Ctrl-D\"}]\n```", "`[{name:\"Ctrl-D\"}]`"),
            (
                "```osp\n[{\"name\":\"Ctrl-D\"\n```",
                "`[{\"name\":\"Ctrl-D\"`",
            ),
        ] {
            let document = parse(text, &Document::default());
            assert_eq!(
                document.blocks,
                [DocBlock::Paragraph {
                    text: expected.into()
                }]
            );
        }
    }
}
