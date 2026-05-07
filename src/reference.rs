//! 直接 OID 的 ref（`git update-ref` 写入的一行 hex），不包含 `ref:` symbolic 内容。

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::object::{self, ObjectSha};

/// 相对 `git_dir` 的路径 + 指向的 commit（direct ref）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub path: PathBuf,
    pub commit_id: ObjectSha,
}

/// `refs/heads/<branch_name>`
pub fn branch_ref_path(branch_name: &str) -> PathBuf {
    PathBuf::from("refs/heads").join(branch_name)
}

fn ensure_loose_commit(git_dir: &Path, oid: &ObjectSha) -> Result<()> {
    let kind = object::read_loose_object_kind(git_dir, oid).with_context(|| {
        format!(
            "object missing or unreadable for {}",
            hex::encode(oid.as_bytes())
        )
    })?;
    if kind != "commit" {
        bail!("ref must point to commit, got object type {kind:?}");
    }
    Ok(())
}

/// 读取 ref 文件：一行 40 位 hex，且 loose 对象存在且为 `commit`
pub fn read_ref(git_dir: &Path, path: impl AsRef<Path>) -> Result<Ref> {
    let path = path.as_ref();
    let full = git_dir.join(path);
    let content = fs::read_to_string(&full).with_context(|| format!("read {}", full.display()))?;
    let line = content.trim();
    if line.starts_with("ref:") {
        bail!(
            "expected direct ref (hex oid) at {}, found symbolic ref",
            full.display()
        );
    }
    if line.lines().nth(1).is_some() {
        bail!("ref file must be a single line: {}", full.display());
    }
    if line.len() != 40 {
        bail!(
            "bad SHA1 ref length {} at {}",
            line.len(),
            full.display()
        );
    }
    if !line.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("non-hex ref content at {}", full.display());
    }
    let bytes: [u8; 20] = hex::decode(line)
        .with_context(|| format!("decode ref at {}", full.display()))?
        .try_into()
        .map_err(|v: Vec<u8>| anyhow::anyhow!("oid length {}", v.len()))?;
    let commit_id = ObjectSha::SHA1(bytes);
    ensure_loose_commit(git_dir, &commit_id)?;
    Ok(Ref {
        path: path.to_path_buf(),
        commit_id,
    })
}

/// 写入 ref（`git update-ref <path> <commit>`）：校验目标为 commit 后写入一行 hex + 换行
pub fn update_ref(
    git_dir: &Path,
    path: impl AsRef<Path>,
    commit_id: &ObjectSha,
) -> Result<Ref> {
    let ObjectSha::SHA1(_) = commit_id else {
        bail!("update_ref only supports SHA1 oids");
    };
    ensure_loose_commit(git_dir, commit_id)?;
    let path = path.as_ref().to_path_buf();
    let full = git_dir.join(&path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut f =
        fs::File::create(&full).with_context(|| format!("create ref {}", full.display()))?;
    let hex = hex::encode(commit_id.as_bytes());
    write!(f, "{hex}\n").with_context(|| format!("write {}", full.display()))?;
    Ok(Ref {
        path,
        commit_id: commit_id.clone(),
    })
}
