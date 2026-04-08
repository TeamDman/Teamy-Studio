use std::num::NonZeroUsize;
use std::path::PathBuf;

use eyre::{Result, ensure};

use crate::paths::CacheHome;

const WORKSPACES_DIR_NAME: &str = "workspaces";
const CELLS_DIR_NAME: &str = "cells";
const WORKSPACE_NAME_FILENAME: &str = "workspace_name.txt";
const WORKSPACE_CELL_ORDER_FILENAME: &str = "workspace_cell_order.txt";
const CELL_CODE_FILENAME: &str = "code.ps1";
const CELL_INPUTS_FILENAME: &str = "inputs.txt";
const CELL_OUTPUT_FILENAME: &str = "output.xml";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Create a validated workspace identifier.
    ///
    /// # Errors
    ///
    /// This function will return an error if the provided id is empty or contains invalid path characters.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_path_atom(&value, "workspace id")?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellId(String);

impl CellId {
    /// Create a validated cell identifier.
    ///
    /// # Errors
    ///
    /// This function will return an error if the provided id is empty or contains invalid path characters.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_path_atom(&value, "cell id")?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// cli[impl path.cache.workspace-root-under-workspaces-dir]
#[must_use]
pub fn workspaces_root(cache_home: &CacheHome) -> PathBuf {
    cache_home.join(WORKSPACES_DIR_NAME)
}

/// cli[impl path.cache.workspace-root-under-workspaces-dir]
#[must_use]
pub fn workspace_root(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> PathBuf {
    workspaces_root(cache_home).join(workspace_id.as_str())
}

/// cli[impl path.cache.workspace-name-file]
#[must_use]
pub fn workspace_name_path(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> PathBuf {
    workspace_root(cache_home, workspace_id).join(WORKSPACE_NAME_FILENAME)
}

/// cli[impl path.cache.workspace-cell-order-file]
#[must_use]
pub fn workspace_cell_order_path(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> PathBuf {
    workspace_root(cache_home, workspace_id).join(WORKSPACE_CELL_ORDER_FILENAME)
}

#[must_use]
pub fn workspace_cells_root(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> PathBuf {
    workspace_root(cache_home, workspace_id).join(CELLS_DIR_NAME)
}

/// cli[impl path.cache.cell-artifact-layout]
#[must_use]
pub fn cell_root(cache_home: &CacheHome, workspace_id: &WorkspaceId, cell_id: &CellId) -> PathBuf {
    workspace_cells_root(cache_home, workspace_id).join(cell_id.as_str())
}

/// cli[impl path.cache.cell-artifact-layout]
#[must_use]
pub fn cell_code_path(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    cell_id: &CellId,
) -> PathBuf {
    cell_root(cache_home, workspace_id, cell_id).join(CELL_CODE_FILENAME)
}

/// cli[impl path.cache.cell-artifact-layout]
#[must_use]
pub fn cell_inputs_path(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    cell_id: &CellId,
) -> PathBuf {
    cell_root(cache_home, workspace_id, cell_id).join(CELL_INPUTS_FILENAME)
}

/// cli[impl path.cache.cell-artifact-layout]
#[must_use]
pub fn cell_output_path(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    cell_id: &CellId,
) -> PathBuf {
    cell_root(cache_home, workspace_id, cell_id).join(CELL_OUTPUT_FILENAME)
}

/// cli[impl path.cache.cell-transcript-numbering]
#[must_use]
pub fn cell_transcript_path(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    cell_id: &CellId,
    run_number: NonZeroUsize,
) -> PathBuf {
    cell_root(cache_home, workspace_id, cell_id).join(format!("run{run_number}.transcript"))
}

fn validate_path_atom(value: &str, label: &str) -> Result<()> {
    ensure!(!value.is_empty(), "{label} cannot be empty");
    ensure!(value != ".", "{label} cannot be '.'");
    ensure!(value != "..", "{label} cannot be '..'");
    ensure!(
        !value.contains(['/', '\\', '\r', '\n', '\t']),
        "{label} cannot contain path separators or control whitespace"
    );
    Ok(())
}
