// Copyright (C) 2026 AnalyseDeCircuit

//! Safe extraction and replacement of a downloaded sidecar archive.

use std::{
    fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
};

pub(super) fn install_runtime_archive(
    archive: &[u8],
    asset_name: &str,
    install_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, String> {
    let parent = install_dir.parent().unwrap_or(install_dir);
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create runtime directory: {error}"))?;
    let staging_dir = create_staging_directory(parent)?;
    let staging_binary = staging_dir.join(binary_name);

    let extraction_result = if asset_name.ends_with(".zip") {
        extract_zip_binary(archive, binary_name, &staging_binary)
    } else if asset_name.ends_with(".tar.gz") {
        extract_tar_gz_binary(archive, binary_name, &staging_binary)
    } else {
        Err(format!("Unsupported Wasm runtime archive: {asset_name}"))
    };
    if let Err(error) = extraction_result {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error);
    }
    if let Err(error) = mark_wasm_runtime_executable(&staging_binary) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error);
    }

    // Keep the previous sidecar intact until the staged directory is in place.
    if let Err(error) = replace_install_directory(&staging_dir, install_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error);
    }
    Ok(install_dir.join(binary_name))
}

fn extract_zip_binary(archive: &[u8], binary_name: &str, output: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive))
        .map_err(|error| format!("Failed to read Wasm runtime zip: {error}"))?;
    let mut extracted = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Failed to read Wasm runtime zip entry: {error}"))?;
        let entry_name = entry.name().to_string();
        validate_archive_entry_path(&entry_name)?;
        if archive_entry_final_name(&entry_name) != binary_name || entry.is_dir() {
            continue;
        }
        extract_selected_binary(&mut entry, output, binary_name, &mut extracted)?;
    }
    extracted
        .then_some(())
        .ok_or_else(|| format!("Wasm runtime zip does not contain {binary_name}"))
}

fn extract_tar_gz_binary(archive: &[u8], binary_name: &str, output: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(archive));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| format!("Failed to read Wasm runtime tarball: {error}"))?;
    let mut extracted = false;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("Failed to read tar entry: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| format!("Failed to read tar entry path: {error}"))?;
        let entry_name = path
            .to_str()
            .ok_or_else(|| "Wasm runtime tarball contains a non-UTF-8 path".to_string())?;
        validate_archive_entry_path(entry_name)?;
        if archive_entry_final_name(entry_name) != binary_name
            || !entry.header().entry_type().is_file()
        {
            continue;
        }
        extract_selected_binary(&mut entry, output, binary_name, &mut extracted)?;
    }
    extracted
        .then_some(())
        .ok_or_else(|| format!("Wasm runtime tarball does not contain {binary_name}"))
}

fn extract_selected_binary(
    source: &mut impl io::Read,
    output: &Path,
    binary_name: &str,
    extracted: &mut bool,
) -> Result<(), String> {
    if *extracted {
        return Err(format!(
            "Wasm runtime archive contains multiple {binary_name} entries"
        ));
    }
    let mut output_file = fs::File::create(output)
        .map_err(|error| format!("Failed to create Wasm runtime binary: {error}"))?;
    io::copy(source, &mut output_file)
        .map_err(|error| format!("Failed to extract Wasm runtime binary: {error}"))?;
    output_file
        .sync_all()
        .map_err(|error| format!("Failed to sync Wasm runtime binary: {error}"))?;
    *extracted = true;
    Ok(())
}

fn validate_archive_entry_path(entry_name: &str) -> Result<(), String> {
    // Archive names are checked before any entry is copied into the staging directory.
    if entry_name.is_empty() || entry_name.starts_with(['/', '\\']) || entry_name.contains('\0') {
        return Err(format!("Unsafe Wasm runtime archive entry: {entry_name}"));
    }
    if entry_name
        .split(['/', '\\'])
        .any(|component| component == ".." || component.contains(':'))
    {
        return Err(format!("Unsafe Wasm runtime archive entry: {entry_name}"));
    }
    Ok(())
}

fn archive_entry_final_name(entry_name: &str) -> &str {
    entry_name.rsplit(['/', '\\']).next().unwrap_or_default()
}

fn create_staging_directory(parent: &Path) -> Result<PathBuf, String> {
    create_unique_directory(parent, ".wasm-runtime-staging")
}

fn create_unique_directory(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    for attempt in 0..1000_u16 {
        let candidate = parent.join(format!("{prefix}-{}-{attempt}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Failed to create runtime staging directory: {error}"
                ));
            }
        }
    }
    Err("Failed to allocate a unique Wasm runtime staging directory".to_string())
}

fn replace_install_directory(staging_dir: &Path, install_dir: &Path) -> Result<(), String> {
    let parent = install_dir.parent().unwrap_or(install_dir);
    let backup_dir = create_backup_path(parent)?;
    let has_existing_install = install_dir.exists();
    if has_existing_install {
        fs::rename(install_dir, &backup_dir).map_err(|error| {
            format!("Failed to stage old Wasm runtime for replacement: {error}")
        })?;
    }

    if let Err(error) = fs::rename(staging_dir, install_dir) {
        let restore_result = if has_existing_install {
            fs::rename(&backup_dir, install_dir)
        } else {
            Ok(())
        };
        let restore_detail = restore_result
            .err()
            .map(|restore_error| format!("; failed to restore old runtime: {restore_error}"))
            .unwrap_or_default();
        return Err(format!(
            "Failed to install Wasm runtime: {error}{restore_detail}"
        ));
    }

    if has_existing_install {
        // A cleanup failure must not turn an already successful replacement into a failed install.
        let _ = fs::remove_dir_all(&backup_dir);
    }
    Ok(())
}

fn create_backup_path(parent: &Path) -> Result<PathBuf, String> {
    for attempt in 0..1000_u16 {
        let candidate = parent.join(format!(
            ".wasm-runtime-backup-{}-{attempt}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to allocate a unique Wasm runtime backup directory".to_string())
}

#[cfg(unix)]
fn mark_wasm_runtime_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|error| format!("Failed to stat Wasm runtime binary: {error}"))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("Failed to mark Wasm runtime executable: {error}"))
}

#[cfg(not(unix))]
fn mark_wasm_runtime_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write as _,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn temporary_directory(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "hapcli-plugin-runtime-install-{label}-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn zip_archive(entry_name: &str, contents: &[u8]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(entry_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn tar_gz_archive(entry_name: &str, contents: &[u8]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, entry_name, contents)
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn zip_install_extracts_only_the_target_binary_and_replaces_previous_runtime() {
        let root = temporary_directory("zip");
        let install_dir = root.join("wasm");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(install_dir.join("old-runtime"), b"old").unwrap();

        let archive = zip_archive("release/runtime-bin", b"new runtime");
        let path =
            install_runtime_archive(&archive, "runtime.zip", &install_dir, "runtime-bin").unwrap();

        assert_eq!(fs::read(path).unwrap(), b"new runtime");
        assert!(!install_dir.join("old-runtime").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tar_gz_install_extracts_the_target_binary() {
        let root = temporary_directory("tar");
        let install_dir = root.join("wasm");
        let archive = tar_gz_archive("release/runtime-bin", b"new runtime");

        let path = install_runtime_archive(&archive, "runtime.tar.gz", &install_dir, "runtime-bin")
            .unwrap();

        assert_eq!(fs::read(path).unwrap(), b"new runtime");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_paths_reject_unix_and_windows_traversal() {
        assert!(validate_archive_entry_path("../runtime-bin").is_err());
        assert!(validate_archive_entry_path("release\\..\\runtime-bin").is_err());
        assert!(validate_archive_entry_path("/runtime-bin").is_err());
    }

    #[test]
    fn zip_install_rejects_path_traversal_before_writing() {
        let root = temporary_directory("traversal");
        let install_dir = root.join("wasm");
        let archive = zip_archive("../runtime-bin", b"malicious");

        let error = install_runtime_archive(&archive, "runtime.zip", &install_dir, "runtime-bin")
            .unwrap_err();

        assert!(error.contains("Unsafe Wasm runtime archive entry"));
        assert!(!install_dir.exists());
        let _ = fs::remove_dir_all(root);
    }
}
