//! IMG V2 CLI
//!
//! Command-line tool for creating and managing IMG V2 archives - simple,
//! sector-aligned binary containers for bundling named files.

use clap::{Command, arg};
use std::mem::MaybeUninit;
use std::path::PathBuf;
use wincode::config::Config;
use wincode::io::{Reader, Writer};
use wincode::{ReadResult, SchemaRead, SchemaWrite, WriteResult};

/// Sector size in bytes. All file payloads are padded to this boundary.
const SECTOR_SIZE: usize = 2048;

fn cli() -> Command {
    Command::new("img")
        .about("IMG V2 Archive Processor")
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

/// On-disk archive header.
///
/// ```text
/// offset  size  field
/// 0x00    4     signature (ASCII "VER2")
/// 0x04    4     entry count (u32, little-endian)
/// ```
struct IMGHeader {
    sig: [u8; 4],
    count: u32,
}

impl Default for IMGHeader {
    fn default() -> Self {
        Self {
            sig: [b'V', b'E', b'R', b'2'],
            count: 0,
        }
    }
}

/// In-memory view of a full archive.
///
/// Entries hold both metadata and payload in one struct for convenience, but
/// serialization writes metadata first and payloads second (see
/// [`SchemaWrite`] for [`IMGArchive`]).
#[derive(Default)]
struct IMGArchive {
    header: IMGHeader,
    directory: Vec<IMGEntry>,
}

impl IMGArchive {
    fn from_path(path: &PathBuf) -> Self {
        wincode::deserialize::<IMGArchive>(&std::fs::read(path).expect("failed to read archive"))
            .expect("failed to deserialize archive")
    }

    fn write(&self, path: &PathBuf) {
        std::fs::write(
            path,
            wincode::serialize(self).expect("failed to serialize archive"),
        )
        .expect("failed to write archive");
    }

    /// Appends a file to the directory. Payload is zero-padded to the next
    /// sector boundary; `offset` is left at 0 until [`Self::rebase`] runs.
    fn add_file(&mut self, buf: &[u8], name: &[u8; 24]) {
        let sectors = (buf.len() + SECTOR_SIZE - 1) / SECTOR_SIZE;

        let mut aligned_buf = Vec::with_capacity(sectors * SECTOR_SIZE);
        aligned_buf.extend_from_slice(buf);
        aligned_buf.resize(sectors * SECTOR_SIZE, 0);

        self.directory.push(IMGEntry {
            sectors: sectors as u16,
            offset: 0,
            size: 0,
            name: name.clone(),
            data: aligned_buf,
        });
    }

    fn remove_file(&mut self, name: &[u8; 24]) {
        self.directory.retain(|entry| entry.name != *name);
    }

    /// Recomputes byte offsets and entry count after adds/removes.
    ///
    /// Entries are sorted by name (null-padded, lexicographic) so listing
    /// and on-disk layout stay deterministic.
    fn rebase(&mut self) {
        let header_size = std::mem::size_of::<IMGHeader>()
            + self.directory.len() * std::mem::size_of::<IMGEntry>();
        let mut offset = header_size;
        self.directory.sort_by_key(|entry| entry.name);
        self.header.count = self.directory.len() as u32;

        for entry in &mut self.directory {
            entry.offset = offset as u32;
            offset += entry.sectors as usize * SECTOR_SIZE;
        }
    }
}

/// Per-file directory record.
///
/// On disk, only the first five fields are stored in the directory table;
/// the `data` payload is written separately after all directory records.
///
/// ```text
/// offset  size  field
/// 0x00    4     offset (byte position of payload in archive)
/// 0x04    2     sectors (payload length in 2048-byte sectors)
/// 0x06    4     size (reserved; always 0 in this implementation)
/// 0x0A   24     name (null-padded ASCII, max 23 chars + NUL)
/// ```
#[derive(Default, SchemaWrite, SchemaRead)]
struct IMGEntry {
    offset: u32,
    sectors: u16,
    size: u32,
    name: [u8; 24],
    data: Vec<u8>,
}

// Custom serialization: directory table (metadata only) then data blobs
unsafe impl<C: Config> SchemaWrite<C> for IMGArchive {
    type Src = Self;

    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        let mut total = <[u8; 4] as SchemaWrite<C>>::size_of(&src.header.sig)?;
        total += <u32 as SchemaWrite<C>>::size_of(&src.header.count)?;
        if src.header.count > 0 {
            total += <Vec<IMGEntry> as SchemaWrite<C>>::size_of(&src.directory)?;
        }
        Ok(total)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        <[u8; 4] as SchemaWrite<C>>::write(writer.by_ref(), &src.header.sig)?;
        <u32 as SchemaWrite<C>>::write(writer.by_ref(), &src.header.count)?;
        if src.header.count > 0 {
            for entry in &src.directory {
                <u32 as SchemaWrite<C>>::write(writer.by_ref(), &entry.offset)?;
                <u16 as SchemaWrite<C>>::write(writer.by_ref(), &entry.sectors)?;
                <u32 as SchemaWrite<C>>::write(writer.by_ref(), &entry.size)?;
                <[u8; 24] as SchemaWrite<C>>::write(writer.by_ref(), &entry.name)?;
            }
            for entry in &src.directory {
                <Vec<u8> as SchemaWrite<C>>::write(writer.by_ref(), &entry.data)?;
            }
        }
        Ok(())
    }
}

unsafe impl<'de, C: Config> SchemaRead<'de, C> for IMGArchive {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let sig = <[u8; 4] as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let count = <u32 as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let directory = if count > 0 {
            let mut directory = Vec::new();
            for _ in 0..count {
                let offset = <u32 as SchemaRead<'de, C>>::get(reader.by_ref())?;
                let sectors = <u16 as SchemaRead<'de, C>>::get(reader.by_ref())?;
                let size = <u32 as SchemaRead<'de, C>>::get(reader.by_ref())?;
                let name = <[u8; 24] as SchemaRead<'de, C>>::get(reader.by_ref())?;
                directory.push(IMGEntry {
                    offset,
                    sectors,
                    size,
                    name,
                    data: Vec::new(),
                });
            }
            for entry in &mut directory {
                entry.data = vec![0u8; entry.sectors as usize * SECTOR_SIZE];
                for byte in &mut entry.data {
                    *byte = <u8 as SchemaRead<'de, C>>::get(reader.by_ref())?;
                }
            }
            directory
        } else {
            Vec::new()
        };
        dst.write(Self {
            header: IMGHeader { sig, count },
            directory,
        });
        Ok(())
    }
}

/// Truncates `name` to 23 bytes and stores it in a fixed 24-byte, null-padded buffer.
fn name_to_array(name: &str) -> [u8; 24] {
    let mut array = [0; 24];
    for (i, c) in name.as_bytes().iter().take(23).enumerate() {
        array[i] = *c;
    }
    array
}

fn create_archive(img: &str) {
    let archive = IMGArchive::default();
    let img_path = PathBuf::from(img);
    std::fs::create_dir_all(img_path.parent().expect("failed to get parent directory"))
        .expect("failed to create directory");
    archive.write(&img_path);
}

fn list_archive(img: &str) {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path);
    // Rebase recalculates offsets for display; changes are not written back.
    archive.rebase();
    for entry in archive.directory {
        println!(
            "{:08}: {} ({} bytes)",
            entry.offset,
            String::from_utf8_lossy(&entry.name),
            if entry.size == 0 {
                entry.sectors as u32 * SECTOR_SIZE as u32
            } else {
                entry.size
            }
        );
    }
}

fn add_files(img: &str, paths: Vec<&str>) {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path);
    for path in paths {
        let path = PathBuf::from(path);
        let basename = path.file_name().expect("failed to get basename");
        let name = name_to_array(basename.to_str().expect("failed to convert basename to string"));
        let data = std::fs::read(path).expect("failed to read file");
        archive.add_file(&data, &name);
    }
    archive.rebase();
    archive.write(&img_path);
}

fn remove_files(img: &str, names: Vec<&str>) {
    let img_path = PathBuf::from(img);
    let mut archive = IMGArchive::from_path(&img_path);
    for name in names {
        archive.remove_file(&name_to_array(name));
    }
    archive.rebase();
    archive.write(&img_path);
}

fn main() {
    let matches = cli().get_matches();

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
            let img = sub_matches.get_one::<String>("IMG").expect("required");
            create_archive(img);
        }
        Some(("list", sub_matches)) => {
            let img = sub_matches.get_one::<String>("IMG").expect("required");
            list_archive(img);
        }
        Some(("add", sub_matches)) => {
            let img = sub_matches.get_one::<String>("IMG").expect("required");
            add_files(img, matches_to_vec_str(sub_matches, "PATH"));
        }
        Some(("remove", sub_matches)) => {
            let img = sub_matches.get_one::<String>("IMG").expect("required");
            remove_files(img, matches_to_vec_str(sub_matches, "NAME"));
        }
        _ => unreachable!(),
    }
}
