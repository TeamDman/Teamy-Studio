use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::{Context, Result, bail, ensure};

use crate::paths::{
    CacheHome, CellId, WorkspaceId, cell_code_path, cell_inputs_path, cell_root,
    workspace_cell_order_path, workspace_cells_root, workspace_name_path, workspace_root,
    workspaces_root,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub name: String,
    pub cell_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceLaunch {
    pub workspace: WorkspaceSummary,
    pub first_cell_id: CellId,
    pub first_cell_dir: PathBuf,
    pub cell_number: usize,
}

/// cli[impl workspace.list.prints-id-name-cell-count]
///
/// # Errors
///
/// This function will return an error if the workspaces directory cannot be read.
pub fn list_workspaces(cache_home: &CacheHome) -> Result<Vec<WorkspaceSummary>> {
    let root = workspaces_root(cache_home);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut workspaces = Vec::new();
    for entry in fs::read_dir(&root)
        .wrap_err_with(|| format!("failed to read workspaces directory {}", root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(workspace_id) = WorkspaceId::new(name) else {
            continue;
        };
        workspaces.push(load_workspace_summary(cache_home, &workspace_id)?);
    }

    workspaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    Ok(workspaces)
}

/// cli[impl workspace.show.bails-when-missing]
/// cli[impl workspace.show.prints-id-name-cell-count]
///
/// # Errors
///
/// This function will return an error if the workspace cannot be resolved.
pub fn show_workspace(cache_home: &CacheHome, target: &str) -> Result<WorkspaceSummary> {
    resolve_workspace(cache_home, target)?
        .ok_or_else(|| eyre::eyre!("workspace `{target}` not found"))
}

/// cli[impl workspace.create.name-optional]
///
/// # Errors
///
/// This function will return an error if the workspace cannot be created on disk.
pub fn create_workspace(cache_home: &CacheHome, name: Option<&str>) -> Result<WorkspaceLaunch> {
    fs::create_dir_all(workspaces_root(cache_home)).wrap_err("failed to create workspaces root")?;

    let workspace_id = next_workspace_id(cache_home)?;
    let workspace_name = normalize_workspace_name(name, workspace_id.as_str())?;
    ensure_workspace_name_available(cache_home, &workspace_name)?;

    let first_cell_id = CellId::new("cell-1")?;
    let root = workspace_root(cache_home, &workspace_id);
    fs::create_dir_all(&root)
        .wrap_err_with(|| format!("failed to create workspace directory {}", root.display()))?;

    write_text_file(
        &workspace_name_path(cache_home, &workspace_id),
        &format!("{workspace_name}\n"),
    )?;
    write_text_file(
        &workspace_cell_order_path(cache_home, &workspace_id),
        &format!("{}\n", first_cell_id.as_str()),
    )?;
    scaffold_cell(cache_home, &workspace_id, &first_cell_id)?;

    Ok(WorkspaceLaunch {
        workspace: WorkspaceSummary {
            id: workspace_id.clone(),
            name: workspace_name,
            cell_count: 1,
        },
        first_cell_dir: cell_root(cache_home, &workspace_id, &first_cell_id),
        first_cell_id,
        cell_number: 1,
    })
}

/// cli[impl workspace.run.no-target-creates-workspace]
/// cli[impl workspace.run.target-by-id-or-name]
///
/// # Errors
///
/// This function will return an error if the target workspace cannot be opened or created.
pub fn open_workspace(cache_home: &CacheHome, target: Option<&str>) -> Result<WorkspaceLaunch> {
    match target {
        Some(target) => {
            let workspace = show_workspace(cache_home, target)?;
            let first_cell_id = ensure_first_cell(cache_home, &workspace.id)?;
            let cell_number = cell_number(cache_home, &workspace.id, &first_cell_id)?;
            let workspace = load_workspace_summary(cache_home, &workspace.id)?;
            Ok(WorkspaceLaunch {
                first_cell_dir: cell_root(cache_home, &workspace.id, &first_cell_id),
                first_cell_id,
                cell_number,
                workspace,
            })
        }
        None => create_workspace(cache_home, None),
    }
}

/// cli[impl workspace.plus-button.appends-cell]
///
/// # Errors
///
/// This function will return an error if the next cell cannot be created or persisted.
pub fn append_workspace_cell(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
) -> Result<WorkspaceLaunch> {
    let mut order = read_cell_order(cache_home, workspace_id)?;
    if order.is_empty() {
        let first = CellId::new("cell-1")?;
        scaffold_cell(cache_home, workspace_id, &first)?;
        order.push(first);
    }

    let cell_number = order.len() + 1;
    let next_cell_id = CellId::new(format!("cell-{cell_number}"))?;
    scaffold_cell(cache_home, workspace_id, &next_cell_id)?;
    order.push(next_cell_id.clone());
    write_cell_order(cache_home, workspace_id, &order)?;

    let workspace = load_workspace_summary(cache_home, workspace_id)?;
    Ok(WorkspaceLaunch {
        workspace,
        first_cell_id: next_cell_id.clone(),
        first_cell_dir: cell_root(cache_home, workspace_id, &next_cell_id),
        cell_number,
    })
}

fn resolve_workspace(cache_home: &CacheHome, target: &str) -> Result<Option<WorkspaceSummary>> {
    if let Ok(workspace_id) = WorkspaceId::new(target.to_owned())
        && workspace_root(cache_home, &workspace_id).is_dir()
    {
        return Ok(Some(load_workspace_summary(cache_home, &workspace_id)?));
    }

    let mut matches = list_workspaces(cache_home)?
        .into_iter()
        .filter(|workspace| workspace.name == target)
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => bail!("multiple workspaces found named `{target}`"),
    }
}

fn load_workspace_summary(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
) -> Result<WorkspaceSummary> {
    let name_path = workspace_name_path(cache_home, workspace_id);
    let name = match fs::read_to_string(&name_path) {
        Ok(contents) => contents.trim_end_matches(['\r', '\n']).to_owned(),
        Err(error) if error.kind() == ErrorKind::NotFound => workspace_id.as_str().to_owned(),
        Err(error) => {
            return Err(error).wrap_err_with(|| {
                format!("failed to read workspace name from {}", name_path.display())
            });
        }
    };

    Ok(WorkspaceSummary {
        cell_count: count_cells(cache_home, workspace_id)?,
        id: workspace_id.clone(),
        name,
    })
}

fn count_cells(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> Result<usize> {
    let cells_root = workspace_cells_root(cache_home, workspace_id);
    if !cells_root.exists() {
        return Ok(0);
    }

    let mut count = 0_usize;
    for entry in fs::read_dir(&cells_root).wrap_err_with(|| {
        format!(
            "failed to read workspace cells directory {}",
            cells_root.display()
        )
    })? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            count += 1;
        }
    }
    Ok(count)
}

fn ensure_first_cell(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> Result<CellId> {
    let order_path = workspace_cell_order_path(cache_home, workspace_id);
    let first_cell_id = match read_first_cell_id(&order_path)? {
        Some(cell_id) => cell_id,
        None => CellId::new("cell-1")?,
    };

    if !order_path.exists() {
        write_text_file(&order_path, &format!("{}\n", first_cell_id.as_str()))?;
    }
    scaffold_cell(cache_home, workspace_id, &first_cell_id)?;
    Ok(first_cell_id)
}

fn cell_number(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    cell_id: &CellId,
) -> Result<usize> {
    let order = read_cell_order(cache_home, workspace_id)?;
    order
        .iter()
        .position(|candidate| candidate == cell_id)
        .map(|index| index + 1)
        .ok_or_else(|| eyre::eyre!("cell `{}` not found in workspace order", cell_id.as_str()))
}

fn read_first_cell_id(path: &Path) -> Result<Option<CellId>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).wrap_err_with(|| {
                format!(
                    "failed to read workspace cell order from {}",
                    path.display()
                )
            });
        }
    };

    if let Some(line) = contents.lines().find(|line| !line.trim().is_empty()) {
        return Ok(Some(CellId::new(line.to_owned())?));
    }
    Ok(None)
}

fn read_cell_order(cache_home: &CacheHome, workspace_id: &WorkspaceId) -> Result<Vec<CellId>> {
    let path = workspace_cell_order_path(cache_home, workspace_id);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).wrap_err_with(|| {
                format!(
                    "failed to read workspace cell order from {}",
                    path.display()
                )
            });
        }
    };

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| CellId::new(line.to_owned()))
        .collect()
}

fn write_cell_order(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    order: &[CellId],
) -> Result<()> {
    let contents = order
        .iter()
        .map(CellId::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    write_text_file(
        &workspace_cell_order_path(cache_home, workspace_id),
        &format!("{contents}\n"),
    )
}

fn scaffold_cell(
    cache_home: &CacheHome,
    workspace_id: &WorkspaceId,
    cell_id: &CellId,
) -> Result<()> {
    let dir = cell_root(cache_home, workspace_id, cell_id);
    fs::create_dir_all(&dir)
        .wrap_err_with(|| format!("failed to create cell directory {}", dir.display()))?;
    ensure_file_exists(&cell_code_path(cache_home, workspace_id, cell_id))?;
    ensure_file_exists(&cell_inputs_path(cache_home, workspace_id, cell_id))?;
    Ok(())
}

fn ensure_file_exists(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_text_file(path, "")
}

fn write_text_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create parent directory {}", parent.display()))?;
    }
    fs::write(path, contents).wrap_err_with(|| format!("failed to write {}", path.display()))
}

fn next_workspace_id(cache_home: &CacheHome) -> Result<WorkspaceId> {
    for attempt in 0_u32..1000 {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let candidate = WorkspaceId::new(format!("workspace-{seed}-{attempt}"))?;
        if !workspace_root(cache_home, &candidate).exists() {
            return Ok(candidate);
        }
    }
    bail!("failed to generate a unique workspace id")
}

fn normalize_workspace_name(name: Option<&str>, fallback: &str) -> Result<String> {
    match name {
        Some(name) => {
            let name = name.trim();
            ensure!(!name.is_empty(), "workspace name cannot be empty");
            ensure!(
                !name.contains(['\r', '\n']),
                "workspace name cannot contain newlines"
            );
            Ok(name.to_owned())
        }
        None => Ok(fallback.to_owned()),
    }
}

fn ensure_workspace_name_available(cache_home: &CacheHome, workspace_name: &str) -> Result<()> {
    if list_workspaces(cache_home)?
        .into_iter()
        .any(|workspace| workspace.name == workspace_name)
    {
        bail!("workspace `{workspace_name}` already exists")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        append_workspace_cell, create_workspace, list_workspaces, open_workspace, show_workspace,
    };
    use crate::paths::CacheHome;

    struct TestCacheHome {
        path: PathBuf,
    }

    impl TestCacheHome {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos();
            Self {
                path: std::env::temp_dir().join(format!("teamy-studio-workspace-test-{unique}")),
            }
        }

        fn cache_home(&self) -> CacheHome {
            CacheHome(self.path.clone())
        }
    }

    impl Drop for TestCacheHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn create_workspace_persists_name_and_initial_cell() {
        let test_home = TestCacheHome::new();
        let cache_home = test_home.cache_home();

        let created =
            create_workspace(&cache_home, Some("alpha")).expect("workspace should create");

        assert_eq!(created.workspace.name, "alpha");
        assert_eq!(created.workspace.cell_count, 1);
        assert!(created.first_cell_dir.exists());
        assert!(created.first_cell_dir.join("code.ps1").exists());
        assert!(created.first_cell_dir.join("inputs.txt").exists());
    }

    #[test]
    fn list_and_show_workspace_report_id_name_and_cell_count() {
        let test_home = TestCacheHome::new();
        let cache_home = test_home.cache_home();
        let created =
            create_workspace(&cache_home, Some("alpha")).expect("workspace should create");

        let listed = list_workspaces(&cache_home).expect("workspace list should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "alpha");
        assert_eq!(listed[0].cell_count, 1);

        let shown = show_workspace(&cache_home, created.workspace.id.as_str())
            .expect("workspace show should find the workspace");
        assert_eq!(shown.id, created.workspace.id);
        assert_eq!(shown.name, "alpha");
        assert_eq!(shown.cell_count, 1);
    }

    #[test]
    fn open_workspace_without_target_creates_a_workspace() {
        let test_home = TestCacheHome::new();
        let cache_home = test_home.cache_home();

        let launch =
            open_workspace(&cache_home, None).expect("workspace open should create a workspace");

        assert_eq!(launch.workspace.cell_count, 1);
        assert!(launch.first_cell_dir.exists());
    }

    #[test]
    fn append_workspace_cell_adds_a_new_ordered_cell() {
        let test_home = TestCacheHome::new();
        let cache_home = test_home.cache_home();
        let created =
            create_workspace(&cache_home, Some("alpha")).expect("workspace should create");

        let appended = append_workspace_cell(&cache_home, &created.workspace.id)
            .expect("workspace append should create a new cell");

        assert_eq!(appended.cell_number, 2);
        assert_eq!(appended.workspace.cell_count, 2);
        assert!(appended.first_cell_dir.exists());
        assert!(appended.first_cell_dir.join("code.ps1").exists());
    }
}
