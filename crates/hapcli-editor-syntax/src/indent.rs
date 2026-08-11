// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::IndentGuide;

pub(crate) fn indent_guides(root: Node<'_>, source: &str, tab_size: usize) -> Vec<IndentGuide> {
    let mut guides = BTreeSet::new();
    collect_indent_guides(root, source, tab_size.max(1), &mut guides);
    guides
        .into_iter()
        .map(|(start_line, end_line, column)| IndentGuide {
            start_line,
            end_line,
            column,
        })
        .collect()
}

fn collect_indent_guides(
    node: Node<'_>,
    source: &str,
    tab_size: usize,
    guides: &mut BTreeSet<(usize, usize, usize)>,
) {
    if node.is_named() && is_indent_container_kind(node.kind()) {
        let start = node.start_position();
        let end = node.end_position();
        if end.row > start.row
            && let Some(column) = indent_guide_column(node, source, tab_size)
        {
            // The guide represents the container boundary rather than the
            // indentation of an arbitrary statement inside it.
            guides.insert((start.row, end.row, column));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_indent_guides(child, source, tab_size, guides);
    }
}

fn is_indent_container_kind(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "compound_statement"
            | "statement_block"
            | "declaration_list"
            | "field_declaration_list"
            | "class_body"
            | "enum_body"
            | "interface_body"
            | "object"
            | "array"
            | "switch_body"
            | "match_block"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_statement"
            | "if_expression"
            | "for_expression"
            | "while_expression"
            | "loop_expression"
            | "case_statement"
            | "try_statement"
            | "catch_clause"
            | "function_definition"
            | "function_declaration"
            | "method_declaration"
            | "class_declaration"
            | "function_item"
            | "impl_item"
            | "mod_item"
            | "struct_item"
            | "trait_item"
    ) || kind.ends_with("_block")
        || kind.ends_with("_body")
}

fn indent_guide_column(node: Node<'_>, source: &str, tab_size: usize) -> Option<usize> {
    let start = node.start_position();
    let end = node.end_position();
    if node_ends_with_closing_delimiter(node, source) {
        // Bracketed blocks own the indentation of their closing delimiter.
        // This keeps the guide aligned with braces even when the opening brace
        // follows a declaration or condition on the same line.
        return source
            .lines()
            .nth(end.row)
            .map(|line| leading_visual_columns(line, tab_size));
    }

    // Languages without closing delimiters, such as Python, still use the
    // first nested statement to establish their body indentation.
    first_nested_line_indent(source, start.row, end.row, tab_size)
}

fn node_ends_with_closing_delimiter(node: Node<'_>, source: &str) -> bool {
    node.end_byte()
        .checked_sub(1)
        .and_then(|index| source.as_bytes().get(index))
        .copied()
        .is_some_and(|byte| matches!(byte, b'}' | b']'))
}

fn first_nested_line_indent(
    source: &str,
    start_line: usize,
    end_line: usize,
    tab_size: usize,
) -> Option<usize> {
    source
        .lines()
        .enumerate()
        .skip(start_line.saturating_add(1))
        .take(end_line.saturating_sub(start_line))
        .find_map(|(_line, text)| {
            if text.trim().is_empty() {
                return None;
            }
            let column = leading_visual_columns(text, tab_size);
            (column > 0).then_some(column)
        })
}

fn leading_visual_columns(text: &str, tab_size: usize) -> usize {
    let mut column = 0;
    for ch in text.chars() {
        match ch {
            ' ' => column += 1,
            '\t' => column += tab_size - column % tab_size,
            _ => break,
        }
    }
    column
}
