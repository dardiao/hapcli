// Copyright (C) 2026 AnalyseDeCircuit

use crate::PreviewSessionState;

pub trait PreviewRenderer {
    type Output;

    fn render_preview(&self, state: &PreviewSessionState) -> Self::Output;
}
