use super::*;
use std::fs;
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("img-cli-{name}-{}", std::process::id()))
}

fn temp_path(name: &str, parts: &[&str]) -> PathBuf {
    let path = parts.iter().fold(temp_root(name), |base, part| base.join(part));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    path
}

fn cleanup_tree(name: &str) {
    let _ = fs::remove_dir_all(temp_root(name));
}

fn write_input(name: &str, parts: &[&str], contents: &[u8]) -> PathBuf {
    let path = temp_path(name, parts);
    fs::write(&path, contents).unwrap();
    path
}

fn path_str(path: &PathBuf) -> &str {
    path.to_str().expect("utf-8 path")
}

#[test]
fn new_creates_archive_in_nested_directory() {
    let name = "cli-new-nested";
    cleanup_tree(name);

    let archive = temp_path(name, &["var", "archives", "bundle.img"]);
    create_archive(path_str(&archive)).unwrap();
    assert!(archive.is_file());

    cleanup_tree(name);
}

#[test]
fn add_reads_from_nested_input_paths() {
    let name = "cli-add-nested";
    cleanup_tree(name);

    let archive = temp_path(name, &["data", "store.img"]);
    let input = write_input(name, &["inputs", "nested", "hello.txt"], b"from nested dir");

    create_archive(path_str(&archive)).unwrap();
    add_files(path_str(&archive), vec![path_str(&input)], &[]).unwrap();

    let contents = read_files(path_str(&archive), &["hello.txt"]).unwrap();
    assert_eq!(contents, vec![b"from nested dir".to_vec()]);

    cleanup_tree(name);
}

#[test]
fn extract_writes_to_nested_output_directory() {
    let name = "cli-extract-nested";
    cleanup_tree(name);

    let archive = temp_path(name, &["archives", "pack.img"]);
    let output_dir = temp_path(name, &["output", "nested", "dir"]);
    let extracted = output_dir.join("payload.bin");

    create_archive(path_str(&archive)).unwrap();
    let source = write_input(name, &["sources", "payload.bin"], b"payload bytes");
    add_files(path_str(&archive), vec![path_str(&source)], &[]).unwrap();
    extract_files(
        path_str(&archive),
        vec!["payload.bin"],
        Some(path_str(&output_dir)),
    )
    .unwrap();

    assert_eq!(fs::read(&extracted).unwrap(), b"payload bytes");

    cleanup_tree(name);
}

#[test]
fn list_opens_archive_in_nested_directory() {
    let name = "cli-list-nested";
    cleanup_tree(name);

    let archive = temp_path(name, &["deep", "nested", "list.img"]);
    let source = write_input(name, &["files", "one.txt"], b"1");

    create_archive(path_str(&archive)).unwrap();
    add_files(path_str(&archive), vec![path_str(&source)], &[]).unwrap();

    let lines = list_archive(path_str(&archive)).unwrap();
    assert!(lines.iter().any(|line| line.contains("one.txt")));

    cleanup_tree(name);
}

#[test]
fn add_expands_glob_patterns() {
    let name = "cli-add-glob";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    write_input(name, &["folder", "a.scm"], b"alpha");
    write_input(name, &["folder", "b.scm"], b"beta");
    write_input(name, &["folder", "notes.txt"], b"ignored");

    create_archive(path_str(&archive)).unwrap();
    let pattern = temp_root(name).join("folder").join("*.scm");
    add_files(path_str(&archive), vec![path_str(&pattern)], &[]).unwrap();

    let contents = read_files(path_str(&archive), &["a.scm", "b.scm"]).unwrap();
    assert_eq!(contents, vec![b"alpha".to_vec(), b"beta".to_vec()]);

    cleanup_tree(name);
}

#[test]
fn remove_expands_glob_patterns() {
    let name = "cli-remove-glob";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    create_archive(path_str(&archive)).unwrap();

    let a = write_input(name, &["sources", "a.scm"], b"a");
    let b = write_input(name, &["sources", "b.scm"], b"b");
    let keep = write_input(name, &["sources", "keep.txt"], b"keep");
    add_files(
        path_str(&archive),
        vec![path_str(&a), path_str(&b), path_str(&keep)],
        &[],
    )
    .unwrap();

    remove_files(path_str(&archive), vec!["*.scm"], &[]).unwrap();

    let lines = list_archive(path_str(&archive)).unwrap();
    assert!(lines.iter().any(|line| line.contains("keep.txt")));
    assert!(!lines.iter().any(|line| line.contains("a.scm")));
    assert!(!lines.iter().any(|line| line.contains("b.scm")));

    cleanup_tree(name);
}

#[test]
fn cat_matches_entry_names_case_insensitively() {
    let name = "cli-cat-case";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    let source = write_input(name, &["sources", "README.md"], b"# readme");

    create_archive(path_str(&archive)).unwrap();
    add_files(path_str(&archive), vec![path_str(&source)], &[]).unwrap();

    let contents = read_files(path_str(&archive), &["readme.md"]).unwrap();
    assert_eq!(contents, vec![b"# readme".to_vec()]);

    cleanup_tree(name);
}

#[test]
fn cat_expands_glob_patterns() {
    let name = "cli-cat-glob";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    create_archive(path_str(&archive)).unwrap();

    let a = write_input(name, &["sources", "a.scm"], b"a");
    let b = write_input(name, &["sources", "b.scm"], b"b");
    let keep = write_input(name, &["sources", "keep.txt"], b"keep");
    add_files(
        path_str(&archive),
        vec![path_str(&a), path_str(&b), path_str(&keep)],
        &[],
    )
    .unwrap();

    let contents = read_files(path_str(&archive), &["*.scm"]).unwrap();
    assert_eq!(contents.len(), 2);
    assert!(contents.contains(&b"a".to_vec()));
    assert!(contents.contains(&b"b".to_vec()));

    cleanup_tree(name);
}

#[test]
fn add_excludes_matching_paths() {
    let name = "cli-add-exclude";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    write_input(name, &["folder", "a.scm"], b"alpha");
    write_input(name, &["folder", "backup.scm"], b"backup");
    write_input(name, &["folder", "notes.txt"], b"notes");

    create_archive(path_str(&archive)).unwrap();
    let pattern = temp_root(name).join("folder").join("*");
    add_files(
        path_str(&archive),
        vec![path_str(&pattern)],
        &["*.scm"],
    )
    .unwrap();

    let lines = list_archive(path_str(&archive)).unwrap();
    assert!(lines.iter().any(|line| line.contains("notes.txt")));
    assert!(!lines.iter().any(|line| line.contains("a.scm")));
    assert!(!lines.iter().any(|line| line.contains("backup.scm")));

    cleanup_tree(name);
}

#[test]
fn remove_excludes_matching_names() {
    let name = "cli-remove-exclude";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    create_archive(path_str(&archive)).unwrap();

    let a = write_input(name, &["sources", "a.scm"], b"a");
    let b = write_input(name, &["sources", "b.scm"], b"b");
    let init = write_input(name, &["sources", "init.scm"], b"init");
    add_files(
        path_str(&archive),
        vec![path_str(&a), path_str(&b), path_str(&init)],
        &[],
    )
    .unwrap();

    remove_files(path_str(&archive), vec!["*.scm"], &["init.scm"]).unwrap();

    let lines = list_archive(path_str(&archive)).unwrap();
    assert!(lines.iter().any(|line| line.contains("init.scm")));
    assert!(!lines.iter().any(|line| line.contains("a.scm")));
    assert!(!lines.iter().any(|line| line.contains("b.scm")));

    cleanup_tree(name);
}

#[test]
fn list_json_output() {
    let name = "cli-list-json";
    cleanup_tree(name);

    let archive = temp_path(name, &["store.img"]);
    let source = write_input(name, &["files", "one.txt"], b"hello");

    create_archive(path_str(&archive)).unwrap();
    add_files(path_str(&archive), vec![path_str(&source)], &[]).unwrap();

    let json = list_archive_json(path_str(&archive)).unwrap();
    let entries: Vec<ListEntry> = serde_json::from_str(&json).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "one.txt");
    assert_eq!(entries[0].size, 2048);

    cleanup_tree(name);
}
