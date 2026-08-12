// Copyright (C) 2026 AnalyseDeCircuit

mod model;
mod runtime;
mod validation;

pub use model::{
    BackgroundTaskCondition, BackgroundTaskEvent, BackgroundTaskExecution,
    BackgroundTaskExecutionResult, BackgroundTaskId, BackgroundTaskMode, BackgroundTaskOwner,
    BackgroundTaskSnapshot, BackgroundTaskSpec, BackgroundTaskState,
};
pub use runtime::{BackgroundTaskExecutor, BackgroundTaskRuntime, fingerprint_json};
pub use validation::{
    BackgroundTaskLimits, BackgroundTaskValidationError, validate_background_task_spec,
};
