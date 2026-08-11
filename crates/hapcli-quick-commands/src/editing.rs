// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Pure quick-command editing and filtering behavior shared by UI adapters.

use crate::{
    MAX_CATEGORIES, QuickCommand, QuickCommandCategory, QuickCommandIcon,
    default_quick_command_categories, new_quick_category_id, new_quick_command_id,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickCommandDraft {
    pub id: Option<String>,
    pub name: String,
    pub command: String,
    pub category: String,
    pub description: String,
    pub host_pattern: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickCommandCategoryDraft {
    pub id: Option<String>,
    pub name: String,
    pub icon: QuickCommandIcon,
}

pub fn quick_command_draft_can_save(draft: &QuickCommandDraft) -> bool {
    !draft.name.trim().is_empty() && !draft.command.trim().is_empty()
}

pub fn quick_command_category_draft_can_save(draft: &QuickCommandCategoryDraft) -> bool {
    !draft.name.trim().is_empty()
}

pub fn visible_quick_commands(
    commands: &[QuickCommand],
    active_category: &str,
    query: &str,
    target_fields: &[String],
) -> Vec<QuickCommand> {
    // Normalize once so filtering remains independent from UI input state.
    let normalized_query = query.trim().to_lowercase();
    commands
        .iter()
        .filter(|command| command.category == active_category)
        .filter(|command| {
            match_quick_command_host_pattern(command.host_pattern.as_deref(), target_fields)
        })
        .filter(|command| {
            normalized_query.is_empty()
                || command.name.to_lowercase().contains(&normalized_query)
                || command.command.to_lowercase().contains(&normalized_query)
                || command
                    .description
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&normalized_query)
        })
        .cloned()
        .collect()
}

pub fn upsert_quick_command(
    commands: &mut Vec<QuickCommand>,
    categories: &[QuickCommandCategory],
    draft: QuickCommandDraft,
    now: u64,
) -> bool {
    // Creation time is stable across edits; update time records each accepted draft.
    let existing_created_at = draft.id.as_ref().and_then(|id| {
        commands
            .iter()
            .find(|command| &command.id == id)
            .map(|command| command.created_at)
    });
    let command = QuickCommand {
        id: draft.id.unwrap_or_else(new_quick_command_id),
        name: draft.name.trim().to_string(),
        command: draft.command.trim().to_string(),
        category: if categories.iter().any(|item| item.id == draft.category) {
            draft.category
        } else {
            "custom".to_string()
        },
        description: trim_optional(&draft.description),
        host_pattern: trim_optional(&draft.host_pattern),
        created_at: existing_created_at.unwrap_or(now),
        updated_at: now,
    };
    if command.name.is_empty() || command.command.is_empty() {
        return false;
    }

    if let Some(existing) = commands.iter_mut().find(|item| item.id == command.id) {
        *existing = command;
    } else {
        commands.push(command);
    }
    true
}

pub fn delete_quick_command(commands: &mut Vec<QuickCommand>, id: &str) -> bool {
    let previous_len = commands.len();
    commands.retain(|command| command.id != id);
    commands.len() != previous_len
}

pub fn upsert_quick_command_category(
    categories: &mut Vec<QuickCommandCategory>,
    draft: QuickCommandCategoryDraft,
    current_active_category: &str,
) -> String {
    let category = QuickCommandCategory {
        id: draft.id.unwrap_or_else(new_quick_category_id),
        name: draft.name.trim().to_string(),
        icon: draft.icon,
    };
    if category.name.is_empty() {
        return current_active_category.to_string();
    }

    if let Some(existing) = categories.iter_mut().find(|item| item.id == category.id) {
        *existing = category.clone();
    } else if categories.len() < MAX_CATEGORIES {
        categories.push(category.clone());
    }
    category.id
}

pub fn delete_quick_command_category(
    categories: &mut Vec<QuickCommandCategory>,
    commands: &[QuickCommand],
    id: &str,
) -> bool {
    if default_quick_command_categories()
        .iter()
        .any(|category| category.id == id)
        || commands.iter().any(|command| command.category == id)
    {
        return false;
    }
    let previous_len = categories.len();
    categories.retain(|category| category.id != id);
    categories.len() != previous_len
}

pub fn ensure_active_quick_command_category(
    categories: &[QuickCommandCategory],
    active_category: &mut String,
) {
    if categories
        .iter()
        .any(|category| category.id == *active_category)
    {
        return;
    }
    *active_category = categories
        .first()
        .map(|category| category.id.clone())
        .unwrap_or_else(|| "custom".to_string());
}

pub fn match_quick_command_host_pattern(pattern: Option<&str>, target_fields: &[String]) -> bool {
    let Some(pattern) = pattern.map(str::trim).filter(|pattern| !pattern.is_empty()) else {
        return true;
    };
    let normalized_pattern = pattern.to_lowercase();
    target_fields
        .iter()
        .any(|field| wildcard_match(&normalized_pattern, &field.to_lowercase()))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut cursor = 0;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(found) = value[cursor..].find(part) else {
            return false;
        };
        if index == 0 && found != 0 {
            return false;
        }
        cursor += found + part.len();
    }
    pattern.ends_with('*') || parts.last().is_none_or(|last| value.ends_with(last))
}

fn trim_optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_upsert_normalizes_fields_and_preserves_creation_time() {
        let categories = default_quick_command_categories();
        let mut commands = vec![QuickCommand {
            id: "existing".to_string(),
            name: "Old".to_string(),
            command: "old".to_string(),
            category: "custom".to_string(),
            description: None,
            host_pattern: None,
            created_at: 7,
            updated_at: 7,
        }];
        assert!(upsert_quick_command(
            &mut commands,
            &categories,
            QuickCommandDraft {
                id: Some("existing".to_string()),
                name: " Updated ".to_string(),
                command: " echo ready ".to_string(),
                category: "missing".to_string(),
                description: "  ".to_string(),
                host_pattern: " *.example.com ".to_string(),
            },
            11,
        ));
        assert_eq!(commands[0].name, "Updated");
        assert_eq!(commands[0].category, "custom");
        assert_eq!(commands[0].created_at, 7);
        assert_eq!(commands[0].updated_at, 11);
        assert_eq!(commands[0].host_pattern.as_deref(), Some("*.example.com"));
    }

    #[test]
    fn host_pattern_matching_preserves_anchored_wildcard_semantics() {
        let targets = vec!["prod.example.com".to_string()];
        assert!(match_quick_command_host_pattern(
            Some("*.example.com"),
            &targets
        ));
        assert!(!match_quick_command_host_pattern(
            Some("example.*"),
            &targets
        ));
    }
}
