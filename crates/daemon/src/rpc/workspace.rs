//! Workspace RPC mapping; all filesystem/Git work runs off async workers.
use crate::{service::workspace, Error, Result};
use amarcode_protocol::rpc::*;
use serde::Serialize;
use serde_json::Value;

fn result<T: Serialize>(value: std::result::Result<T, String>) -> Result<Value> {
    Ok(serde_json::to_value(value.map_err(Error::msg)?)?)
}

pub(super) async fn dispatch(method: String, params: Value) -> Result<Value> {
    tokio::task::spawn_blocking(move || match method.as_str() {
        methods::LIST_WORKSPACE_FILES => {
            let p: ListWorkspaceFilesParams = serde_json::from_value(params)?;
            result(workspace::list_workspace_files(p.workspace_path, p.query))
        }
        methods::GET_WORKSPACE_INFO => {
            let p: WorkspaceParams = serde_json::from_value(params)?;
            result(workspace::get_workspace_info(p.workspace_path))
        }
        methods::LIST_WORKSPACE_CHANGES => {
            let p: WorkspaceParams = serde_json::from_value(params)?;
            result(workspace::list_workspace_changes(p.workspace_path))
        }
        methods::STAGE_WORKSPACE_FILE => {
            let p: WorkspaceFileParams = serde_json::from_value(params)?;
            result(workspace::stage_workspace_file(
                p.workspace_path,
                p.relative_path,
            ))
        }
        methods::UNSTAGE_WORKSPACE_FILE => {
            let p: WorkspaceFileParams = serde_json::from_value(params)?;
            result(workspace::unstage_workspace_file(
                p.workspace_path,
                p.relative_path,
            ))
        }
        methods::STAGE_ALL_WORKSPACE_FILES => {
            let p: WorkspaceParams = serde_json::from_value(params)?;
            result(workspace::stage_all_workspace_files(p.workspace_path))
        }
        methods::COMMIT_WORKSPACE_CHANGES => {
            let p: CommitWorkspaceChangesParams = serde_json::from_value(params)?;
            result(workspace::commit_workspace_changes(
                p.workspace_path,
                p.message,
            ))
        }
        methods::GET_WORKSPACE_FILE_DIFF => {
            let p: GetWorkspaceFileDiffParams = serde_json::from_value(params)?;
            result(workspace::get_workspace_file_diff(
                p.workspace_path,
                p.relative_path,
                p.staged,
            ))
        }
        methods::READ_WORKSPACE_FILE => {
            let p: WorkspaceFileParams = serde_json::from_value(params)?;
            result(workspace::read_workspace_file(
                p.workspace_path,
                p.relative_path,
            ))
        }
        methods::SAVE_WORKSPACE_FILE => {
            let p: SaveWorkspaceFileParams = serde_json::from_value(params)?;
            result(workspace::save_workspace_file(
                p.workspace_path,
                p.relative_path,
                p.expected_content,
                p.content,
            ))
        }
        _ => Err(Error::msg(format!("unknown workspace method: {method}"))),
    })
    .await
    .map_err(|error| Error::msg(format!("workspace worker failed: {error}")))?
}
