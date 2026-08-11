// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! GitHub release DTOs and integrity metadata selection.

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
pub(super) struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct GithubReleaseAsset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

impl GithubRelease {
    pub(super) fn asset_named(&self, name: &str) -> Option<&GithubReleaseAsset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    pub(super) fn select_runtime_asset(&self, target: &str) -> Result<&GithubReleaseAsset, String> {
        self.assets
            .iter()
            .find(|asset| {
                asset.name.starts_with("hapcli-wasm-runtime-")
                    && asset.name.contains(target)
                    && (asset.name.ends_with(".zip") || asset.name.ends_with(".tar.gz"))
            })
            .ok_or_else(|| format!("No Wasm runtime asset found for target {target}"))
    }

    pub(super) fn version(&self) -> String {
        self.tag_name.trim_start_matches('v').to_string()
    }
}

pub(super) fn checksum_for_asset(checksums: &str, asset_name: &str) -> Result<String, String> {
    checksums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == asset_name).then(|| hash.to_string())
        })
        .ok_or_else(|| format!("SHA256SUMS does not contain {asset_name}"))
}

pub(super) fn verify_sha256(bytes: &[u8], expected: &str, asset_name: &str) -> Result<(), String> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "Checksum mismatch for {asset_name}: expected {expected}, got {actual}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_selection_accepts_star_prefixed_filenames() {
        let checksums =
            "d2a4c0b5e2e9a2448a5cf4331e32c3d870fceb77f0757c4a66c1c9ea0a4f5c26  *runtime.zip";

        assert_eq!(
            checksum_for_asset(checksums, "runtime.zip").unwrap(),
            "d2a4c0b5e2e9a2448a5cf4331e32c3d870fceb77f0757c4a66c1c9ea0a4f5c26"
        );
    }

    #[test]
    fn checksum_verification_rejects_mismatched_bytes() {
        let error = verify_sha256(b"runtime", "00", "runtime.zip").unwrap_err();

        assert!(error.contains("Checksum mismatch"));
    }
}
