// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Quick Command storage, editing, filtering, and risk classification.
//!
//! The GPUI view owns interaction and presentation state; this crate owns the
//! portable domain behavior shared by `.oxide` import/export, the CLI, and UI.

mod editing;
pub mod model;
mod risk;
pub mod store;

pub use editing::{
    QuickCommandCategoryDraft, QuickCommandDraft, delete_quick_command,
    delete_quick_command_category, ensure_active_quick_command_category,
    match_quick_command_host_pattern, quick_command_category_draft_can_save,
    quick_command_draft_can_save, upsert_quick_command, upsert_quick_command_category,
    visible_quick_commands,
};
pub use model::{
    QUICK_COMMANDS_SCHEMA_VERSION, QuickCommand, QuickCommandCategory, QuickCommandIcon,
    QuickCommandImportResult, QuickCommandImportStrategy, QuickCommandsSnapshot,
};
pub use risk::{QuickCommandRisk, classify_command_risk};
pub use store::{
    MAX_CATEGORIES, QuickCommandsCheckpoint, apply_snapshot_json, capture_checkpoint,
    default_quick_command_categories, default_quick_commands, export_snapshot_json,
    is_builtin_category_id, load_snapshot, new_quick_category_id, new_quick_command_id, now_ms,
    quick_commands_path, restore_checkpoint, save_snapshot,
};
