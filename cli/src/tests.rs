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
    add_files(path_str(&archive), vec![path_str(&input)]).unwrap();

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
    add_files(path_str(&archive), vec![path_str(&source)]).unwrap();
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
    add_files(path_str(&archive), vec![path_str(&source)]).unwrap();

    let lines = list_archive(path_str(&archive)).unwrap();
    assert!(lines.iter().any(|line| line.contains("one.txt")));

    cleanup_tree(name);
}
