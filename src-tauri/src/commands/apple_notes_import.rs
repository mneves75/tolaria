//! Tauri command for importing Apple Notes into the active vault.
//!
//! Thin glue over [`crate::import::run`]: it resolves the Apple Notes database
//! location and the destination/manifest paths, runs the import off the main
//! thread, persists the manifest, and maps a Full-Disk-Access permission error
//! into a recognizable message the UI can act on. macOS-only in practice (the
//! Apple Notes database only exists there); the UI hides the entry elsewhere.

use std::path::{Path, PathBuf};

use crate::import::manifest::ImportManifest;
use crate::import::run::{self, ImportReport};

use super::vault::boundary::with_requested_root;

/// Marker prefix the frontend matches to show the Full Disk Access explainer.
const FDA_REQUIRED: &str =
    "FDA_REQUIRED: Tolaria needs Full Disk Access to read Apple Notes. Open System Settings → \
     Privacy & Security → Full Disk Access, enable Tolaria, then try again.";

/// Subfolder (under the vault) imported notes are written into.
const IMPORT_SUBFOLDER: &str = "Apple Notes";

#[tauri::command]
pub async fn import_apple_notes(vault_path: String) -> Result<ImportReport, String> {
    tokio::task::spawn_blocking(move || validated_apple_notes_import(&vault_path))
        .await
        .map_err(|err| format!("import task panicked: {err}"))?
}

fn validated_apple_notes_import(vault_path: &str) -> Result<ImportReport, String> {
    with_requested_root(vault_path, |requested_root| {
        run_apple_notes_import(requested_root)
    })
}

fn run_apple_notes_import(vault_path: &str) -> Result<ImportReport, String> {
    let home = dirs::home_dir().ok_or("could not determine the home directory")?;
    let paths = ImportPaths::plan(vault_path, &home);

    if !apple_notes_present(&paths.notes_dir) {
        return Err(FDA_REQUIRED.to_string());
    }

    let prior = ImportManifest::load_or_new(&paths.manifest_path, run::SOURCE)?;
    match run::run_import(&paths.notes_dir, &paths.dest_dir, &prior) {
        Ok((report, manifest)) => {
            manifest.save(&paths.manifest_path)?;
            Ok(report)
        }
        Err(err) if is_permission_error(&err) => Err(FDA_REQUIRED.to_string()),
        Err(err) => Err(err),
    }
}

/// Resolved filesystem locations for one import run. Pure to keep it testable.
struct ImportPaths {
    notes_dir: PathBuf,
    dest_dir: PathBuf,
    manifest_path: PathBuf,
}

impl ImportPaths {
    fn plan(vault_path: &str, home: &Path) -> Self {
        let vault = Path::new(vault_path);
        Self {
            notes_dir: home.join("Library/Group Containers/group.com.apple.notes"),
            dest_dir: vault.join(IMPORT_SUBFOLDER),
            manifest_path: vault.join(".tolaria").join("apple-notes-manifest.json"),
        }
    }
}

/// Whether the Apple Notes database is readable. A `false` here means either no
/// Apple Notes data or, more often, Full Disk Access is not granted.
fn apple_notes_present(notes_dir: &Path) -> bool {
    notes_dir.join("NoteStore.sqlite").exists()
}

fn is_permission_error(message: &str) -> bool {
    message.contains("Permission denied") || message.contains("os error 13")
}

#[cfg(test)]
mod tests {
    use super::{is_permission_error, ImportPaths, FDA_REQUIRED, IMPORT_SUBFOLDER};
    use std::path::Path;

    #[test]
    fn plans_paths_under_vault_and_home() {
        let paths = ImportPaths::plan("/vault", Path::new("/Users/x"));
        assert_eq!(
            paths.notes_dir,
            Path::new("/Users/x/Library/Group Containers/group.com.apple.notes")
        );
        assert_eq!(paths.dest_dir, Path::new("/vault").join(IMPORT_SUBFOLDER));
        assert_eq!(
            paths.manifest_path,
            Path::new("/vault/.tolaria/apple-notes-manifest.json")
        );
    }

    #[test]
    fn import_rejects_unavailable_vault_roots_before_reading_notes() {
        let err =
            super::validated_apple_notes_import("/definitely/missing/tolaria-vault").unwrap_err();

        assert_eq!(err, "Active vault is not available");
    }

    #[test]
    fn detects_permission_errors() {
        assert!(is_permission_error(
            "failed to copy …: Permission denied (os error 13)"
        ));
        assert!(is_permission_error("Permission denied"));
        assert!(!is_permission_error("some unrelated error"));
    }

    #[test]
    fn fda_marker_is_actionable() {
        assert!(FDA_REQUIRED.starts_with("FDA_REQUIRED:"));
        assert!(FDA_REQUIRED.contains("Full Disk Access"));
    }
}
