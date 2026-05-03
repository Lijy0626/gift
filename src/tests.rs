use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::index;

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

