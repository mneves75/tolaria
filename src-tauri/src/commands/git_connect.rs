use crate::git::GitAddRemoteResult;
use serde::Deserialize;

use super::vault::boundary::with_requested_root;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAddRemoteRequest {
    vault_path: String,
    remote_url: String,
}

#[cfg(desktop)]
#[tauri::command]
pub async fn git_add_remote(request: GitAddRemoteRequest) -> Result<GitAddRemoteResult, String> {
    let vault_path = with_requested_root(&request.vault_path, |requested_root| {
        Ok(requested_root.to_string())
    })?;
    let remote_url = request.remote_url;
    tokio::task::spawn_blocking(move || crate::git::git_add_remote(&vault_path, &remote_url))
        .await
        .map_err(|e| format!("Task panicked: {e}"))?
}

#[cfg(mobile)]
#[tauri::command]
pub async fn git_add_remote(_request: GitAddRemoteRequest) -> Result<GitAddRemoteResult, String> {
    Err("Adding git remotes is not available on mobile".into())
}
