// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP download helpers with a hard in-memory size limit.

use futures_util::StreamExt as _;

pub(super) async fn download_asset_bytes(
    client: &reqwest::Client,
    url: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Failed to download Wasm runtime asset: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Wasm runtime asset returned HTTP {}",
            response.status().as_u16()
        ));
    }
    if let Some(content_length) = response.content_length() {
        validate_download_size(content_length, maximum_bytes)?;
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Failed to read Wasm runtime asset: {error}"))?;
        let next_length = bytes.len().saturating_add(chunk.len()) as u64;
        validate_download_size(next_length, maximum_bytes)?;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_download_size(actual_bytes: u64, maximum_bytes: u64) -> Result<(), String> {
    if actual_bytes > maximum_bytes {
        return Err(format!(
            "Wasm runtime asset too large: {actual_bytes} bytes"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_size_limit_rejects_streams_that_exceed_the_limit() {
        assert!(validate_download_size(256, 128).is_err());
        assert!(validate_download_size(128, 128).is_ok());
    }
}
