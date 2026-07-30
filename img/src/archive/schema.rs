use super::IMGArchive;
use crate::metadata::{IMGDirectory, IMGDirectoryEntry, IMGMetadata};
use std::mem::MaybeUninit;
use wincode::config::Config;
use wincode::io::{Reader, Writer};
use wincode::{ReadResult, SchemaRead, SchemaWrite, WriteResult};

unsafe impl<C: Config> SchemaWrite<C> for IMGArchive {
    type Src = Self;

    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        let metadata = IMGMetadata {
            header: src.header.clone(),
            directory: IMGDirectory {
                entries: src.directory.iter().map(IMGDirectoryEntry::from).collect(),
            },
        };
        let mut total = <IMGMetadata as SchemaWrite<C>>::size_of(&metadata)?;
        if let Some(blob) = &src.payload_blob {
            total += blob.len();
        }
        Ok(total)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        let metadata = IMGMetadata {
            header: src.header.clone(),
            directory: IMGDirectory {
                entries: src.directory.iter().map(IMGDirectoryEntry::from).collect(),
            },
        };
        <IMGMetadata as SchemaWrite<C>>::write(writer.by_ref(), &metadata)?;
        if let Some(blob) = &src.payload_blob {
            for byte in blob {
                <u8 as SchemaWrite<C>>::write(writer.by_ref(), byte)?;
            }
        }
        Ok(())
    }
}

unsafe impl<'de, C: Config> SchemaRead<'de, C> for IMGArchive {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let metadata = <IMGMetadata as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let payload_base = metadata.payload_base();
        let payload_len = metadata.directory.on_disk_payload_len();
        let directory = metadata.directory.into_entries();
        let payload_blob = if payload_len > 0 {
            let mut blob = vec![0u8; payload_len];
            for byte in &mut blob {
                *byte = <u8 as SchemaRead<'de, C>>::get(reader.by_ref())?;
            }
            Some(blob)
        } else {
            None
        };

        dst.write(IMGArchive {
            header: metadata.header,
            directory,
            payload_base,
            payload_blob,
            scratch: Vec::new(),
            source: None,
        });
        Ok(())
    }
}
