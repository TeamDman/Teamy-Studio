use std::num::NonZeroUsize;
use std::path::PathBuf;

use teamy_studio::paths::{
    CacheHome, CellId, WorkspaceId, cell_code_path, cell_inputs_path, cell_output_path, cell_root,
    cell_transcript_path, workspace_cell_order_path, workspace_name_path, workspace_root,
    workspaces_root,
};

fn sample_cache_home() -> CacheHome {
    CacheHome(PathBuf::from(r"C:\teamy-cache"))
}

fn sample_workspace_id() -> WorkspaceId {
    WorkspaceId::new("workspace-123").expect("workspace id should be valid")
}

fn sample_cell_id() -> CellId {
    CellId::new("cell-456").expect("cell id should be valid")
}

// cli[verify path.cache.workspace-root-under-workspaces-dir]
// cli[verify path.cache.workspace-name-file]
// cli[verify path.cache.workspace-cell-order-file]
#[test]
fn notebook_workspace_paths_follow_the_workspace_layout() {
    let cache_home = sample_cache_home();
    let workspace_id = sample_workspace_id();

    assert_eq!(
        workspaces_root(&cache_home),
        PathBuf::from(r"C:\teamy-cache\workspaces")
    );
    assert_eq!(
        workspace_root(&cache_home, &workspace_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123")
    );
    assert_eq!(
        workspace_name_path(&cache_home, &workspace_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\workspace_name.txt")
    );
    assert_eq!(
        workspace_cell_order_path(&cache_home, &workspace_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\workspace_cell_order.txt")
    );
}

// cli[verify path.cache.cell-artifact-layout]
// cli[verify path.cache.cell-transcript-numbering]
#[test]
fn notebook_cell_paths_follow_the_cell_artifact_layout() {
    let cache_home = sample_cache_home();
    let workspace_id = sample_workspace_id();
    let cell_id = sample_cell_id();

    assert_eq!(
        cell_root(&cache_home, &workspace_id, &cell_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\cells\cell-456")
    );
    assert_eq!(
        cell_code_path(&cache_home, &workspace_id, &cell_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\cells\cell-456\code.ps1")
    );
    assert_eq!(
        cell_inputs_path(&cache_home, &workspace_id, &cell_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\cells\cell-456\inputs.txt")
    );
    assert_eq!(
        cell_output_path(&cache_home, &workspace_id, &cell_id),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\cells\cell-456\output.xml")
    );
    assert_eq!(
        cell_transcript_path(
            &cache_home,
            &workspace_id,
            &cell_id,
            NonZeroUsize::new(1).expect("run number should be positive"),
        ),
        PathBuf::from(r"C:\teamy-cache\workspaces\workspace-123\cells\cell-456\run1.transcript")
    );
}

#[test]
fn notebook_ids_reject_invalid_path_atoms() {
    for invalid in ["", ".", "..", "hello/world", "hello\\world", "hello\nworld"] {
        assert!(
            WorkspaceId::new(invalid).is_err(),
            "workspace id should reject {invalid:?}"
        );
        assert!(
            CellId::new(invalid).is_err(),
            "cell id should reject {invalid:?}"
        );
    }
}
