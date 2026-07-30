//! IMG V2 command-line interface.

use anyhow::Context;
use clap::{Command, arg};
use glob::{MatchOptions, Pattern, glob};
use img::{AddFileResult, IMGArchive, MAX_NAME_LEN};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn name_match_options() -> MatchOptions {
    MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        ..MatchOptions::new()
    }
}

pub fn cli() -> Command {
    Command::new("img")
        .about("IMG V2 CLI")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .allow_external_subcommands(true)
        .subcommand(
            Command::new("new")
                .about("creates a new archive")
                .arg_required_else_help(true)
                .arg(arg!(<IMG> "IMG file to create").value_parser(clap::value_parser!(String))),
        )
        .subcommand(
            Command::new("add")
                .about("adds a file to the archive")
                .arg_required_else_help(true)
                .arg(arg!(<IMG> "IMG file to process").value_parser(clap::value_parser!(String)))
                .arg(arg!(<PATH> ... "Files to add").value_parser(clap::value_parser!(String)))
                .arg(
                    arg!(-x --exclude <PATTERN> "Skip paths matching this pattern")
                        .action(clap::ArgAction::Append)
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .subcommand(
            Command::new("list")
                .about("lists the files in the archive")
                .arg_required_else_help(false)
                .arg(arg!(<IMG> "IMG file to process").value_parser(clap::value_parser!(String)))
                .arg(arg!(--json "Output entries as JSON")),
        )
        .subcommand(
            Command::new("cat")
                .about("writes archive entry contents to stdout")
                .arg_required_else_help(true)
                .arg(arg!(<IMG> "IMG file to process").value_parser(clap::value_parser!(String)))
                .arg(
                    arg!(<NAME> ... "Names of files to print")
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .subcommand(
            Command::new("extract")
                .about("extracts files from the archive")
                .arg_required_else_help(true)
                .arg(arg!(<IMG> "IMG file to process").value_parser(clap::value_parser!(String)))
                .arg(
                    arg!(<NAME> ... "Names of files to extract")
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    arg!(-o --output <DIR> "Output directory")
                        .required(false)
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .subcommand(
            Command::new("remove")
                .about("removes a file from the archive")
                .arg_required_else_help(true)
                .arg(arg!(<IMG> "IMG file to process").value_parser(clap::value_parser!(String)))
                .arg(
                    arg!(<NAME> ... "Names of files to remove")
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    arg!(-x --exclude <PATTERN> "Skip archive names matching this pattern")
                        .action(clap::ArgAction::Append)
                        .value_parser(clap::value_parser!(String)),
                ),
        )
}

pub fn create_archive(img: &str) -> anyhow::Result<()> {
    let mut archive = IMGArchive::default();
    let img_path = PathBuf::from(img);
    if let Some(parent) = img_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    archive.write(&img_path)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    pub offset: u32,
    pub name: String,
    pub size: u32,
}

pub fn list_archive_entries(img: &str) -> anyhow::Result<Vec<ListEntry>> {
    let img_path = PathBuf::from(img);
    let archive = IMGArchive::from_path(&img_path)?;
    Ok(archive
        .list_entries()
        .into_iter()
        .map(|(offset, entry)| ListEntry {
            offset,
            name: entry.name.clone(),
            size: entry.stored_size(),
        })
        .collect())
}

pub fn format_list_entry(entry: &ListEntry) -> String {
    format!(
        "{:08}: {} ({} bytes)",
        entry.offset, entry.name, entry.size
    )
}

pub fn list_archive(img: &str) -> anyhow::Result<Vec<String>> {
    Ok(list_archive_entries(img)?
        .iter()
        .map(format_list_entry)
        .collect())
}

pub fn list_archive_json(img: &str) -> anyhow::Result<String> {
    let entries = list_archive_entries(img)?;
    Ok(serde_json::to_string_pretty(&entries)?)
}

pub fn read_files(img: &str, names: &[&str]) -> anyhow::Result<Vec<Vec<u8>>> {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path)?;
    let resolved = expand_name_patterns(&archive, names, NameMatchMode::Error)?;
    resolved
        .iter()
        .map(|name| archive.read_file(name))
        .collect()
}

pub fn cat_files(img: &str, names: Vec<&str>) -> anyhow::Result<()> {
    let mut stdout = io::stdout().lock();
    for (index, content) in read_files(img, &names)?.into_iter().enumerate() {
        if index > 0 {
            stdout.write_all(b"\n")?;
        }
        stdout.write_all(&content)?;
    }
    Ok(())
}

pub fn extract_files(img: &str, names: Vec<&str>, output: Option<&str>) -> anyhow::Result<()> {
    let img_path = PathBuf::from(img);
    let output_dir = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if output.is_some() {
        std::fs::create_dir_all(&output_dir).with_context(|| {
            format!("failed to create output directory {}", output_dir.display())
        })?;
    }

    let mut archive = IMGArchive::from_path(&img_path)?;
    let resolved = expand_name_patterns(&archive, &names, NameMatchMode::Error)?;
    for name in resolved {
        let content = archive.read_file(&name)?;
        let out_path = output_dir.join(&name);
        std::fs::write(&out_path, content).with_context(|| {
            format!("failed to write extracted file {}", out_path.display())
        })?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum NameMatchMode {
    Warn,
    Error,
}

fn compile_glob_patterns(patterns: &[&str]) -> anyhow::Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern)
                .with_context(|| format!("invalid glob pattern: {pattern}"))
        })
        .collect()
}

fn matches_any(candidate: &str, patterns: &[Pattern]) -> bool {
    let options = name_match_options();
    patterns
        .iter()
        .any(|pattern| pattern.matches_with(candidate, options))
}

fn path_exclusion_candidates(path: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(name) = path.file_name().and_then(|part| part.to_str()) {
        candidates.push(name.to_string());
    }
    candidates.push(path.to_string_lossy().into_owned());
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(relative) = path.strip_prefix(&cwd) {
            candidates.push(relative.to_string_lossy().into_owned());
        }
    }
    candidates
}

fn apply_path_excludes(
    paths: Vec<PathBuf>,
    excludes: &[&str],
) -> anyhow::Result<Vec<PathBuf>> {
    if excludes.is_empty() {
        return Ok(paths);
    }
    let exclude_patterns = compile_glob_patterns(excludes)?;
    Ok(paths
        .into_iter()
        .filter(|path| {
            !path_exclusion_candidates(path)
                .iter()
                .any(|candidate| matches_any(candidate, &exclude_patterns))
        })
        .collect())
}

fn apply_name_excludes(names: Vec<String>, excludes: &[&str]) -> anyhow::Result<Vec<String>> {
    if excludes.is_empty() {
        return Ok(names);
    }
    let exclude_patterns = compile_glob_patterns(excludes)?;
    Ok(names
        .into_iter()
        .filter(|name| !matches_any(name, &exclude_patterns))
        .collect())
}

fn expand_path_patterns(patterns: &[&str]) -> anyhow::Result<Vec<PathBuf>> {
    let mut expanded = Vec::new();
    for pattern in patterns {
        let matches: Vec<PathBuf> = glob(pattern)
            .with_context(|| format!("invalid glob pattern: {pattern}"))?
            .filter_map(|entry| entry.ok())
            .filter(|path| path.is_file())
            .collect();
        if matches.is_empty() {
            eprintln!("warning: no files matched pattern: {pattern}");
            continue;
        }
        expanded.extend(matches);
    }
    expanded.sort();
    expanded.dedup();
    Ok(expanded)
}

fn expand_name_patterns(
    archive: &IMGArchive,
    patterns: &[&str],
    mode: NameMatchMode,
) -> anyhow::Result<Vec<String>> {
    let entry_names: Vec<String> = archive
        .list_entries()
        .into_iter()
        .map(|(_, entry)| entry.name.clone())
        .collect();

    let options = name_match_options();

    let mut matched = Vec::new();
    for pattern in patterns {
        let glob = Pattern::new(pattern)
            .with_context(|| format!("invalid glob pattern: {pattern}"))?;
        let hits: Vec<String> = entry_names
            .iter()
            .filter(|name| glob.matches_with(name, options))
            .cloned()
            .collect();
        if hits.is_empty() {
            match mode {
                NameMatchMode::Warn => {
                    eprintln!("warning: no entries matched pattern: {pattern}");
                }
                NameMatchMode::Error => {
                    anyhow::bail!("no entries matched pattern: {pattern}");
                }
            }
        } else {
            matched.extend(hits);
        }
    }
    matched.sort();
    matched.dedup();
    Ok(matched)
}

pub fn add_files(img: &str, paths: Vec<&str>, excludes: &[&str]) -> anyhow::Result<()> {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path)?;
    for path in apply_path_excludes(expand_path_patterns(&paths)?, excludes)? {
        let basename = path
            .file_name()
            .with_context(|| format!("failed to get basename for {}", path.display()))?;
        let name = basename
            .to_str()
            .with_context(|| format!("non-UTF-8 basename in {}", path.display()))?;
        if name.len() > MAX_NAME_LEN {
            eprintln!(
                "warning: filename exceeds maximum length of {MAX_NAME_LEN} bytes, skipping: {name}"
            );
            continue;
        }
        let data = std::fs::read(&path)
            .with_context(|| format!("failed to read file {}", path.display()))?;
        match archive.add_file(&data, name)? {
            AddFileResult::Added => {}
            AddFileResult::DuplicateIgnored => {
                eprintln!("warning: {name} already exists in archive, skipping");
            }
        }
    }
    archive.write(&img_path)?;
    Ok(())
}

pub fn remove_files(img: &str, names: Vec<&str>, excludes: &[&str]) -> anyhow::Result<()> {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path)?;
    for name in apply_name_excludes(
        expand_name_patterns(&archive, &names, NameMatchMode::Warn)?,
        excludes,
    )? {
        let removed = archive.remove_file(&name);
        if removed == 0 {
            eprintln!("warning: {name} not found in archive, skipping");
        }
    }
    archive.write(&img_path)?;
    Ok(())
}

pub fn run() -> anyhow::Result<()> {
    let matches = cli().get_matches();
    run_matches(&matches)
}

pub fn run_matches(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    fn matches_to_vec_str<'a>(matches: &'a clap::ArgMatches, name: &'a str) -> Vec<&'a str> {
        matches
            .get_many::<String>(name)
            .into_iter()
            .flatten()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
    }

    match matches.subcommand() {
        Some(("new", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            create_archive(img)?;
        }
        Some(("list", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            if sub_matches.get_flag("json") {
                println!("{}", list_archive_json(img)?);
            } else {
                for line in list_archive(img)? {
                    println!("{line}");
                }
            }
        }
        Some(("cat", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            cat_files(img, matches_to_vec_str(sub_matches, "NAME"))?;
        }
        Some(("extract", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            extract_files(
                img,
                matches_to_vec_str(sub_matches, "NAME"),
                sub_matches.get_one::<String>("output").map(String::as_str),
            )?;
        }
        Some(("add", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            add_files(
                img,
                matches_to_vec_str(sub_matches, "PATH"),
                &matches_to_vec_str(sub_matches, "exclude"),
            )?;
        }
        Some(("remove", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            remove_files(
                img,
                matches_to_vec_str(sub_matches, "NAME"),
                &matches_to_vec_str(sub_matches, "exclude"),
            )?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests;
