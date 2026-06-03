use super::expand_tilde;
use std::path::{Path, PathBuf};

const CLONE_BASE_DIR_NAME: &str = "Vaults";

#[cfg(desktop)]
fn clone_base_dir() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(CLONE_BASE_DIR_NAME))
        .ok_or_else(|| "Could not determine the home directory".to_string())
}

#[cfg(desktop)]
fn validate_clone_child_name(path: &Path) -> Result<&std::ffi::OsStr, String> {
    let Some(name) = path.file_name() else {
        return Err("Choose a vault folder inside ~/Vaults".to_string());
    };
    Ok(name)
}

#[cfg(desktop)]
fn validate_clone_destination(local_path: &str) -> Result<String, String> {
    let dest = PathBuf::from(local_path);
    if dest.exists() {
        return Err("Choose a new folder inside ~/Vaults for the cloned vault".to_string());
    }

    let base = clone_base_dir()?;
    let Some(parent) = dest.parent() else {
        return Err("Choose a vault folder inside ~/Vaults".to_string());
    };
    if parent != base {
        return Err("Clone destination must be a new folder inside ~/Vaults".to_string());
    }

    let name = validate_clone_child_name(&dest)?;
    if name.is_empty() {
        return Err("Choose a vault folder inside ~/Vaults".to_string());
    }

    if base.exists() {
        let metadata = std::fs::symlink_metadata(&base)
            .map_err(|e| format!("Failed to inspect ~/Vaults: {e}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("~/Vaults must be a normal directory".to_string());
        }
    }

    Ok(dest.to_string_lossy().into_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn clone_git_repo(url: String, local_path: String) -> Result<String, String> {
    let url = url.trim().to_string();
    let expanded_path = expand_tilde(&local_path).into_owned();
    let local_path = validate_clone_destination(&expanded_path)?;

    tokio::task::spawn_blocking(move || super::git::clone_repo(url, local_path))
        .await
        .map_err(|e| format!("Task panicked: {e}"))?
}

#[cfg(mobile)]
#[tauri::command]
pub async fn clone_git_repo(_url: String, _local_path: String) -> Result<String, String> {
    Err("Git clone is not available on mobile".into())
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn clone_destination_accepts_new_child_inside_vaults() {
        let dest = clone_base_dir().unwrap().join("tolaria-test-clone-target");
        if dest.exists() {
            std::fs::remove_dir_all(&dest).unwrap();
        }

        assert_eq!(
            validate_clone_destination(dest.to_str().unwrap()).unwrap(),
            dest.to_string_lossy()
        );
    }

    #[test]
    fn clone_destination_rejects_existing_empty_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let error = validate_clone_destination(dir.path().to_str().unwrap()).unwrap_err();

        assert!(error.contains("new folder"));
    }

    #[test]
    fn clone_destination_rejects_paths_outside_vaults() {
        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("repo");

        let error = validate_clone_destination(dest.to_str().unwrap()).unwrap_err();

        assert!(error.contains("~/Vaults"));
    }

    #[test]
    fn clone_destination_rejects_nested_children() {
        let dest = clone_base_dir().unwrap().join("parent").join("repo");

        let error = validate_clone_destination(dest.to_str().unwrap()).unwrap_err();

        assert!(error.contains("~/Vaults"));
    }
}
