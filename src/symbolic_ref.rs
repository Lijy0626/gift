//! Git symbolic ref（`ref: <refname>`）；`git_dir` 为相对 worktree 的 git 目录（`.git` / `.gift` 等）。

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `worktree.join(git_dir)`，即仓库目录的绝对路径
pub fn resolve_git_dir(worktree: &Path, git_dir: impl AsRef<Path>) -> PathBuf {
    worktree.join(git_dir.as_ref())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolicRef {
    pub ref_name: String,   // `ref:` 后的 Git ref 名（如 `refs/heads/main`或`HEAD`）
}

/// 将相对git_dir的路径 `ref_name` 转换为相对worktree的路径 `sym_file`
pub fn ref_name_to_sym_file(git_dir: impl AsRef<Path>, ref_name: impl AsRef<str>) -> PathBuf {
    git_dir
        .as_ref()
        .join(ref_name.as_ref().trim_start_matches('/'))
}

/// worktree 相对路径、位于 git 目录内的 ref 文件 → Git ref 名（如 `refs/heads/main`）
pub fn ref_file_to_ref_name(
    worktree: &Path,
    git_dir: impl AsRef<Path>,
    ref_file: impl AsRef<Path>,
) -> Result<String> {
    let abs = worktree.join(ref_file.as_ref());
    let gd = resolve_git_dir(worktree, git_dir.as_ref());
    let rel = abs.strip_prefix(&gd).with_context(|| {
        format!(
            "ref file {} not under git dir {}",
            ref_file.as_ref().display(),
            gd.display()
        )
    })?;
    let s = rel.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches('/').to_string();
    if s.is_empty() {
        bail!("empty ref name from {}", ref_file.as_ref().display());
    }
    Ok(s)
}

/// 从 `sym_file` 读取 `ref: …`，得到 `ref_name`（`git_dir` 与 `write_symbolic_ref` 对齐，供后续校验扩展）
pub fn read_symbolic_ref(
    worktree: &Path,
    sym_file: impl AsRef<Path>,
) -> Result<SymbolicRef> {
    let full = worktree.join(sym_file.as_ref());
    let content = fs::read_to_string(&full).with_context(|| format!("read {}", full.display()))?;
    let line = content.trim_end_matches(['\r', '\n']).trim_end();
    if line.lines().nth(1).is_some() {
        bail!("symbolic ref must be a single line: {}", full.display());
    }
    let rest = line
        .strip_prefix("ref:")
        .map(|s| s.trim_start())
        .context("expected `ref:` prefix (symbolic ref)")?;
    if rest.is_empty() {
        bail!("empty ref target in {}", full.display());
    }
    let ref_name = rest.replace('\\', "/");
    Ok(SymbolicRef { ref_name })
}

/// 将 `ref: <ref_name>\n` 写入 `sym_file`
pub fn write_symbolic_ref(
    worktree: &Path,
    sym_file: impl AsRef<Path>,
    sym: &SymbolicRef,
) -> Result<()> {
    let full = worktree.join(sym_file.as_ref());
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut f = fs::File::create(&full).with_context(|| format!("write {}", full.display()))?;
    write!(f, "ref: {}\n", sym.ref_name).with_context(|| format!("write ref {}", full.display()))?;
    Ok(())
}
