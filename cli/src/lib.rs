//! IMG V2 command-line interface.

use anyhow::Context;
use clap::{Command, arg};
use img::{AddFileResult, IMGArchive, MAX_NAME_LEN};
use std::io::{self, Write};
use std::path::PathBuf;

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
                .arg(arg!(<PATH> ... "Files to add").value_parser(clap::value_parser!(String))),
        )
        .subcommand(
            Command::new("list")
                .about("lists the files in the archive")
                .arg_required_else_help(false)
                .arg(arg!(<IMG> "IMG file to process").value_parser(clap::value_parser!(String))),
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

pub fn list_archive(img: &str) -> anyhow::Result<Vec<String>> {
    let img_path = PathBuf::from(img);
    let archive = IMGArchive::from_path(&img_path)?;
    Ok(archive
        .list_entries()
        .into_iter()
        .map(|(offset, entry)| {
            format!(
                "{:08}: {} ({} bytes)",
                offset,
                entry.name,
                entry.stored_size()
            )
        })
        .collect())
}

pub fn read_files(img: &str, names: &[&str]) -> anyhow::Result<Vec<Vec<u8>>> {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path)?;
    names
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
    for name in names {
        let content = archive.read_file(name)?;
        let out_path = output_dir.join(name);
        std::fs::write(&out_path, content).with_context(|| {
            format!("failed to write extracted file {}", out_path.display())
        })?;
    }

    Ok(())
}

pub fn add_files(img: &str, paths: Vec<&str>) -> anyhow::Result<()> {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path)?;
    for path in paths {
        let path = PathBuf::from(path);
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

pub fn remove_files(img: &str, names: Vec<&str>) -> anyhow::Result<()> {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path)?;
    for name in names {
        let removed = archive.remove_file(name);
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
            for line in list_archive(img)? {
                println!("{line}");
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
            add_files(img, matches_to_vec_str(sub_matches, "PATH"))?;
        }
        Some(("remove", sub_matches)) => {
            let img = sub_matches
                .get_one::<String>("IMG")
                .context("missing IMG argument")?;
            remove_files(img, matches_to_vec_str(sub_matches, "NAME"))?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests;
