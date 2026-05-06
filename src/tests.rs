use std::collections::BTreeMap;
use std::fs;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::index;
use crate::index::index_tree::{IndexRootTree, TreeNode};
use crate::object::{FileMode, ObjectSha, TreeObject};

fn make_case_dir(case_name: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let root = PathBuf::from("target")
        .join("inspect")
        .join(format!("{case_name}-{ts}"));

    fs::create_dir_all(&root).unwrap();
    root
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("failed to run git");

    assert!(status.success(), "git {:?} failed", args);
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// `git ls-tree` 一行：`100644 blob <sha>\tname`
fn parse_ls_tree_line(line: &str) -> (String, String, String, String) {
    let (left, name) = line.split_once('\t').expect("ls-tree line must contain tab");
    let mut it = left.split_whitespace();
    let mode = it.next().expect("mode").to_string();
    let obj_type = it.next().expect("type").to_string();
    let sha = it.next().expect("sha").to_string();
    assert!(it.next().is_none(), "unexpected extra fields: {line:?}");
    (mode, obj_type, sha, name.to_string())
}

fn mode_word_to_file_mode(mode: &str) -> FileMode {
    match mode {
        "100644" => FileMode::NExecRegularFile,
        "100755" => FileMode::ExecRegularFile,
        "120000" => FileMode::SymbolicLink,
        "160000" => FileMode::Gitlink,
        // `git ls-tree` 对 tree 常用 6 位 `040000`；对象里 on-disk 常为 `40000`
        "40000" | "040000" => FileMode::Directory,
        other => panic!("unexpected ls-tree mode {other:?}"),
    }
}

/// 使用 `ls-tree -z`，路径为原始 UTF-8（非 ASCII 不会被 `"\345..."` 转义）。
fn git_ls_tree_map(dir: &Path, tree_oid: &str) -> BTreeMap<String, (String, String, String)> {
    let stdout = git_stdout(dir, &["ls-tree", "-z", tree_oid]);
    let mut m = BTreeMap::new();
    for chunk in stdout.split_terminator('\0') {
        if chunk.is_empty() {
            continue;
        }
        let (mode, obj_type, sha, name) = parse_ls_tree_line(chunk);
        m.insert(name, (mode, obj_type, sha));
    }
    m
}

/// DFS 收集所有 blob 叶子：相对路径 → (mode, oid)。
fn collect_blob_leaves_from_tree(rel: PathBuf, node: &TreeNode, out: &mut BTreeMap<PathBuf, (FileMode, ObjectSha)>) {
    match node {
        TreeNode::Blob(leaf) => {
            out.insert(rel, (leaf.file_mode(), leaf.object_name().clone()));
        }
        TreeNode::Tree(map) => {
            for (seg, child) in map {
                let mut next = rel.clone();
                next.push(seg);
                collect_blob_leaves_from_tree(next, child, out);
            }
        }
    }
}

fn collect_blob_leaves_from_root(root: &IndexRootTree) -> BTreeMap<PathBuf, (FileMode, ObjectSha)> {
    let mut out = BTreeMap::new();
    for (seg, child) in root.root_children() {
        let mut rel = PathBuf::new();
        rel.push(seg);
        collect_blob_leaves_from_tree(rel, child, &mut out);
    }
    out
}

#[test]
fn cmp_init() {
    let case_dir = make_case_dir("init");

    // 1. 构造测试需要的文件
    std::fs::write(case_dir.join("a.txt"), "hello\n").unwrap();
    std::fs::create_dir_all(case_dir.join("foo")).unwrap();
    std::fs::write(case_dir.join("foo").join("main.rs"), "fn main() {}\n").unwrap();

    // 2. 真实 git
    run_git(&case_dir, &["init"]);

    // 4. 我的实现
    super::init(&(&case_dir.join(".gift"))).unwrap();

    // 5. 做断言
}

#[test]
fn cmp_write_obj() {
    let case_dir = make_case_dir("write_hash_object");

    // 1. 构造测试需要的文件
    std::fs::write(case_dir.join("a.txt"), "hello\n").unwrap();
    std::fs::create_dir_all(case_dir.join("foo")).unwrap();
    std::fs::write(case_dir.join("foo").join("bar"), "你好！aa\n").unwrap();

    // 2. 真实 git
    run_git(&case_dir, &["init"]);
    run_git(&case_dir, &["hash-object", "-w", "a.txt"]);
    run_git(&case_dir, &["hash-object", "-w", "foo/bar"]);

    // 4. 我的实现
    let gift_dir = case_dir.join(".gift");
    super::init(&gift_dir).unwrap();

    let my_obj_path = case_dir.join("a.txt");
    let (obj_hash, obj_content) = super::object::hash_object(my_obj_path).unwrap();
    super::object::write_hash_object(&gift_dir, &obj_hash, &obj_content).unwrap();

    let my_obj_path = case_dir.join("foo/bar");
    let (obj_hash, obj_content) = super::object::hash_object(my_obj_path).unwrap();
    super::object::write_hash_object(&gift_dir, &obj_hash, &obj_content).unwrap();
}

#[test]
fn parse_index() {
    let case_dir = make_case_dir("parse_index");

    // 1. 构造测试需要的文件
    std::fs::write(case_dir.join("a.txt"), "hello\n").unwrap();
    std::fs::create_dir_all(case_dir.join("foo").join("啊")).unwrap();
    std::fs::write(case_dir.join("foo").join("啊").join("bar"), "你好！aa\n").unwrap();

    // 2. git指令
    run_git(&case_dir, &["init"]);
    run_git(&case_dir, &["add", "."]);

    // 3. 解析index_file
    index::display_index_file(&case_dir.join(".git/index")).unwrap();
}

/// `stage_paths` 写入 objects + index 后，用 `parse_index_file` 读回并校验路径、oid 及 loose object 文件存在。
#[test]
fn stage_paths_roundtrip_index() {
    // 准备工作环境
    let case_dir = make_case_dir("stage_roundtrip");
    let work_tree = fs::canonicalize(&case_dir).unwrap();
    let git_dir = work_tree.join(".gift");
    super::init(&git_dir).unwrap();

    fs::write(work_tree.join("top.txt"), b"hello\n").unwrap();
    fs::create_dir_all(work_tree.join("sub")).unwrap();
    fs::write(work_tree.join("sub").join("nested.txt"), b"x\n").unwrap();

    // 执行stage操作
    let inputs = vec![
        work_tree.join("top.txt"),
        work_tree.join("sub"),
    ];
    super::staging::stage_paths(&git_dir, &work_tree, &inputs, true).unwrap();

    // 拿到并检验index_file
    let idx = index::parse_index_file(git_dir.join("index")).unwrap();
    assert_eq!(idx.version(), 2, "index version");
    assert_eq!(idx.entries().len(), 2, "entry count");

    let mut paths: Vec<Vec<u8>> = idx.entries().iter().map(|e| e.path().to_vec()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![b"sub/nested.txt".to_vec(), b"top.txt".to_vec()],
        "index paths"
    );

    for e in idx.entries() {
        let rel = std::str::from_utf8(e.path()).expect("entry path utf-8");
        let disk = work_tree.join(rel);
        let (sha, _) = super::object::hash_object(&disk).unwrap();
        assert_eq!(
            hex::encode(sha.as_bytes()),
            hex::encode(e.obj_name().as_bytes()),
            "blob oid mismatch for {rel}"
        );

        let hex_oid = hex::encode(e.obj_name().as_bytes());
        let loose = git_dir
            .join("objects")
            .join(&hex_oid[0..2])
            .join(&hex_oid[2..]);
        assert!(
            loose.is_file(),
            "loose object file should exist: {}",
            loose.display()
        );
    }
}

/// 用真实 `git write-tree` 产生的 tree loose object，校验 `read_object_type` + `read_tree` 与 `git ls-tree` 一致（含 `100755` 与 `120000`）。
#[cfg(unix)]
#[test]
fn cmp_read_tree_matches_git_ls_tree() {
    use std::os::unix::fs::symlink;

    // 准备测试环境
    let case_dir = make_case_dir("read_tree");
    let git_dir = case_dir.join(".git");

    fs::write(case_dir.join("a.txt"), "hello\n").unwrap();
    fs::create_dir_all(case_dir.join("foo").join("啊")).unwrap();
    fs::write(
        case_dir.join("foo").join("啊").join("bar"),
        "你好！aa\n",
    )
    .unwrap();
    fs::write(case_dir.join("script.sh"), "#!/bin/sh\necho ok\n").unwrap();
    symlink("a.txt", case_dir.join("link")).expect("symlink link -> a.txt");

    let chmod_ok = Command::new("chmod")
        .args(["+x", "script.sh"])
        .current_dir(&case_dir)
        .status()
        .expect("chmod");
    assert!(chmod_ok.success(), "chmod +x script.sh");

    // 执行git指令
    run_git(&case_dir, &["init"]);
    run_git(&case_dir, &["add", "."]);

    let root_hex = git_stdout(&case_dir, &["write-tree"])
        .lines()
        .next()
        .expect("write-tree line")
        .trim()
        .to_string();
    assert_eq!(root_hex.len(), 40, "root tree oid hex length");

    // 得到git cat-file的结果
    let cat_t = git_stdout(&case_dir, &["cat-file", "-t", &root_hex])
        .trim()
        .to_string();
    assert_eq!(cat_t, "tree");

    // 得到git ls-tree的结果
    let expected_root = git_ls_tree_map(&case_dir, &root_hex);
    // 用我们实现的函数去读取得到的结果
    let tree_root = TreeObject::read_loose_tree(&git_dir, &root_hex);

    assert_eq!(
        hex::encode(tree_root.object_name().as_bytes()),
        root_hex,
        "TreeObject carries the opened oid"
    );
    assert_eq!(
        tree_root.entries().len(),
        expected_root.len(),
        "root entry count vs git ls-tree"
    );

    for (name, entry) in tree_root.entries() {
        let key = name.to_str().expect("utf-8 name in fixture");
        let (mode, obj_type, sha) = expected_root
            .get(key)
            .unwrap_or_else(|| panic!("unexpected entry {key:?}"));
        assert_eq!(
            mode_word_to_file_mode(mode),
            entry.file_mode,
            "mode for {key}"
        );
        assert_eq!(hex::encode(entry.object_name.as_bytes()), *sha, "sha {key}");
        match entry.file_mode {
            FileMode::Directory => assert_eq!(obj_type, "tree"),
            FileMode::SymbolicLink | FileMode::NExecRegularFile | FileMode::ExecRegularFile => {
                assert_eq!(obj_type, "blob");
            }
            _ => panic!("unexpected FileMode in fixture"),
        }
    }

    // 子 tree：foo -> 啊 -> bar
    let (_, _, foo_tree_sha) = expected_root.get("foo").expect("foo tree");
    let expected_foo = git_ls_tree_map(&case_dir, foo_tree_sha);
    let tree_foo = TreeObject::read_loose_tree(&git_dir, foo_tree_sha);
    assert_eq!(tree_foo.entries().len(), expected_foo.len());
    assert_eq!(tree_foo.entries().len(), 1, "foo/ only contains 啊");

    let (_, _, ah_tree_sha) = expected_foo
        .get("啊")
        .expect("啊 tree under foo");
    let expected_ah = git_ls_tree_map(&case_dir, ah_tree_sha);
    let tree_ah = TreeObject::read_loose_tree(&git_dir, ah_tree_sha);
    assert_eq!(tree_ah.entries().len(), 1);
    assert_eq!(tree_ah.entries().len(), expected_ah.len());

    let bar_entry = tree_ah.entries().get(OsStr::new("bar")).expect("bar blob");
    assert_eq!(bar_entry.file_mode, FileMode::NExecRegularFile);
    let (_, _, bar_sha) = expected_ah.get("bar").unwrap();
    assert_eq!(hex::encode(bar_entry.object_name.as_bytes()), *bar_sha);
}

/// 仅 `from_index_file`：内存树中每个 blob 叶子的路径、mode、OID 与 index 条目一致。
#[test]
fn from_index_file_matches_parsed_index_entries() {
    let case_dir = make_case_dir("from_index_only");
    fs::write(case_dir.join("a.txt"), "hello\n").unwrap();
    fs::create_dir_all(case_dir.join("foo").join("啊")).unwrap();
    fs::write(
        case_dir.join("foo").join("啊").join("bar"),
        "你好！aa\n",
    )
    .unwrap();

    run_git(&case_dir, &["init"]);
    run_git(&case_dir, &["add", "."]);

    let idx = index::parse_index_file(case_dir.join(".git/index")).unwrap();
    let mut expected: BTreeMap<PathBuf, (FileMode, ObjectSha)> = BTreeMap::new();
    for e in idx.entries() {
        let path = e.decode_entry_path();
        expected.insert(path, (e.file_mode(), e.obj_name().clone()));
    }

    let root = IndexRootTree::from_index_file(&idx).expect("from_index_file");
    let got = collect_blob_leaves_from_root(&root);

    assert_eq!(got.len(), idx.entries().len(), "leaf count == index entries");
    assert_eq!(got.len(), expected.len());
    for (path, exp) in &expected {
        assert_eq!(got.get(path), Some(exp), "{}", path.display());
    }
    for path in got.keys() {
        assert!(expected.contains_key(path), "unexpected leaf {}", path.display());
    }
}

/// `from_index_file` + `write_tree`：根 OID 与 `git write-tree` 一致，且各层 tree 与 `git ls-tree -z` 一致。
#[cfg(unix)]
#[test]
fn from_index_file_write_tree_matches_git_write_tree() {
    use std::os::unix::fs::symlink;

    let case_dir = make_case_dir("index_write_tree");
    let git_dir = case_dir.join(".git");

    fs::write(case_dir.join("a.txt"), "hello\n").unwrap();
    fs::create_dir_all(case_dir.join("foo").join("啊")).unwrap();
    fs::write(
        case_dir.join("foo").join("啊").join("bar"),
        "你好！aa\n",
    )
    .unwrap();
    fs::write(case_dir.join("script.sh"), "#!/bin/sh\necho ok\n").unwrap();
    symlink("a.txt", case_dir.join("link")).expect("symlink");

    let chmod_ok = Command::new("chmod")
        .args(["+x", "script.sh"])
        .current_dir(&case_dir)
        .status()
        .expect("chmod");
    assert!(chmod_ok.success());

    run_git(&case_dir, &["init"]);
    run_git(&case_dir, &["add", "."]);

    let idx = index::parse_index_file(git_dir.join("index")).unwrap();
    let root = IndexRootTree::from_index_file(&idx).expect("from_index_file");
    let gift_root_oid = root.write_tree(&git_dir, true).expect("write_tree");

    let want_hex = git_stdout(&case_dir, &["write-tree"])
        .lines()
        .next()
        .expect("write-tree")
        .trim()
        .to_string();
    assert_eq!(want_hex.len(), 40);
    assert_eq!(
        hex::encode(gift_root_oid.as_bytes()),
        want_hex,
        "root tree oid vs git write-tree"
    );

    let loose = git_dir
        .join("objects")
        .join(&want_hex[0..2])
        .join(&want_hex[2..]);
    assert!(loose.is_file(), "root loose object exists");

    let cat_t = git_stdout(&case_dir, &["cat-file", "-t", &want_hex])
        .trim()
        .to_string();
    assert_eq!(cat_t, "tree");

    let expected_root = git_ls_tree_map(&case_dir, &want_hex);
    let tree_root = TreeObject::read_loose_tree(&git_dir, &want_hex);
    assert_eq!(tree_root.entries().len(), expected_root.len());

    for (name, entry) in tree_root.entries() {
        let key = name.to_str().expect("utf-8 name");
        let (mode, obj_type, sha) = expected_root.get(key).expect("ls-tree key");
        assert_eq!(mode_word_to_file_mode(mode), entry.file_mode, "mode {key}");
        assert_eq!(hex::encode(entry.object_name.as_bytes()), *sha, "sha {key}");
        match entry.file_mode {
            FileMode::Directory => assert_eq!(obj_type, "tree"),
            FileMode::SymbolicLink | FileMode::NExecRegularFile | FileMode::ExecRegularFile => {
                assert_eq!(obj_type, "blob");
            }
            _ => panic!("unexpected mode"),
        }
    }

    let (_, _, foo_tree_sha) = expected_root.get("foo").expect("foo");
    let expected_foo = git_ls_tree_map(&case_dir, foo_tree_sha);
    let tree_foo = TreeObject::read_loose_tree(&git_dir, foo_tree_sha);
    assert_eq!(tree_foo.entries().len(), expected_foo.len());
    assert_eq!(tree_foo.entries().len(), 1);

    let (_, _, ah_tree_sha) = expected_foo.get("啊").expect("啊");
    let expected_ah = git_ls_tree_map(&case_dir, ah_tree_sha);
    let tree_ah = TreeObject::read_loose_tree(&git_dir, ah_tree_sha);
    assert_eq!(tree_ah.entries().len(), expected_ah.len());
    let bar_entry = tree_ah.entries().get(OsStr::new("bar")).expect("bar");
    assert_eq!(bar_entry.file_mode, FileMode::NExecRegularFile);
    let (_, _, bar_sha) = expected_ah.get("bar").unwrap();
    assert_eq!(hex::encode(bar_entry.object_name.as_bytes()), *bar_sha);
}

