//! Completion-tree path resolution and command-derived context hints.
//!
//! The parser tells completion "what tokens exist near the cursor". This
//! module answers the next question: which completion-tree node and flag scope
//! those tokens imply for the later suggestion pass.

use crate::completion::model::{CompletionContext, CompletionNode, CompletionTree};

pub(crate) struct ResolvedNodes<'a> {
    pub(crate) context_node: &'a CompletionNode,
    pub(crate) flag_scope_node: &'a CompletionNode,
}

pub(crate) struct TreeResolver<'a> {
    tree: &'a CompletionTree,
}

impl<'a> TreeResolver<'a> {
    pub(crate) fn new(tree: &'a CompletionTree) -> Self {
        Self { tree }
    }

    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn matched_command_len_tokens(&self, tokens: &[String]) -> usize {
        let mut node = &self.tree.root;
        let mut matched = 0usize;

        for token in tokens {
            if token == "|" || token.starts_with('-') {
                break;
            }
            let Some((_, child)) = node.resolve_child(token) else {
                break;
            };
            matched += 1;
            if child.value_key || child.value_leaf {
                break;
            }
            node = child;
        }

        matched
    }

    pub(crate) fn resolved_nodes(&self, context: &CompletionContext) -> ResolvedNodes<'a> {
        ResolvedNodes {
            context_node: self.resolve_exact_or_root(&context.matched_path),
            flag_scope_node: self.resolve_exact_or_root(&context.flag_scope_path),
        }
    }

    pub(crate) fn resolve_exact_or_root(&self, path: &[String]) -> &'a CompletionNode {
        self.resolve_exact(path).unwrap_or(&self.tree.root)
    }

    pub(crate) fn resolve_flag_scope_path(&self, matched_path: &[String]) -> Vec<String> {
        let floor = if matched_path.is_empty() { 0 } else { 1 };
        for i in (floor..=matched_path.len()).rev() {
            let prefix = &matched_path[..i];
            let Some(node) = self.resolve_exact(prefix) else {
                continue;
            };
            if !node.flags.is_empty() {
                return prefix.to_vec();
            }
        }
        if matched_path.is_empty() {
            Vec::new()
        } else {
            matched_path.to_vec()
        }
    }

    pub(crate) fn resolve_context(&self, path: &[String]) -> (&'a CompletionNode, Vec<String>) {
        let mut node = &self.tree.root;
        let mut matched = Vec::new();

        for segment in path {
            let Some((canonical, next)) = node.resolve_child(segment) else {
                break;
            };
            node = next;
            matched.push(canonical.clone());
            if node.value_leaf {
                break;
            }
        }

        (node, matched)
    }

    pub(crate) fn resolve_exact(&self, path: &[String]) -> Option<&'a CompletionNode> {
        let mut node = &self.tree.root;
        for (index, segment) in path.iter().enumerate() {
            node = node.resolve_child(segment)?.1;
            if node.value_leaf && index + 1 < path.len() {
                return None;
            }
        }
        Some(node)
    }
}
