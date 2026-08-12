// Copyright (C) 2026 AnalyseDeCircuit

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionSet {
    pub capabilities: Vec<String>,
    pub allowed_host_apis: Vec<String>,
}
