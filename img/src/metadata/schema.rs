use super::{DIRECTORY_ENTRY_SIZE, IMGDirectory, IMGDirectoryEntry, IMGMetadata, NAME_FIELD_SIZE};
use std::mem::MaybeUninit;
use wincode::config::Config;
use wincode::io::{Reader, Writer};
use wincode::{ReadResult, SchemaRead, SchemaWrite, WriteResult};

unsafe impl<C: Config> SchemaWrite<C> for IMGDirectoryEntry {
    type Src = Self;

    fn size_of(_src: &Self::Src) -> WriteResult<usize> {
        Ok(DIRECTORY_ENTRY_SIZE)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        <u32 as SchemaWrite<C>>::write(writer.by_ref(), &src.offset)?;
        <u16 as SchemaWrite<C>>::write(writer.by_ref(), &src.sectors)?;
        <u16 as SchemaWrite<C>>::write(writer.by_ref(), &src.size)?;
        for byte in IMGDirectoryEntry::encode_name_for_disk(&src.name) {
            <u8 as SchemaWrite<C>>::write(writer.by_ref(), &byte)?;
        }
        Ok(())
    }
}

unsafe impl<'de, C: Config> SchemaRead<'de, C> for IMGDirectoryEntry {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let offset = <u32 as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let sectors = <u16 as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let size = <u16 as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let name_bytes = <[u8; NAME_FIELD_SIZE] as SchemaRead<'de, C>>::get(reader.by_ref())?;
        dst.write(IMGDirectoryEntry {
            offset,
            sectors,
            size,
            name: IMGDirectoryEntry::decode_name_from_disk(&name_bytes),
        });
        Ok(())
    }
}

unsafe impl<C: Config> SchemaWrite<C> for IMGDirectory {
    type Src = Self;

    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        Ok(src.entries.len() * DIRECTORY_ENTRY_SIZE)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        for entry in &src.entries {
            <IMGDirectoryEntry as SchemaWrite<C>>::write(writer.by_ref(), entry)?;
        }
        Ok(())
    }
}

unsafe impl<C: Config> SchemaWrite<C> for IMGMetadata {
    type Src = Self;

    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        let mut total = <crate::IMGHeader as SchemaWrite<C>>::size_of(&src.header)?;
        total += <IMGDirectory as SchemaWrite<C>>::size_of(&src.directory)?;
        Ok(total)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        <crate::IMGHeader as SchemaWrite<C>>::write(writer.by_ref(), &src.header)?;
        <IMGDirectory as SchemaWrite<C>>::write(writer.by_ref(), &src.directory)?;
        Ok(())
    }
}

unsafe impl<'de, C: Config> SchemaRead<'de, C> for IMGMetadata {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let header = <crate::IMGHeader as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let directory = IMGDirectory::read_from::<C>(reader.by_ref(), header.count)?;
        dst.write(IMGMetadata { header, directory });
        Ok(())
    }
}
