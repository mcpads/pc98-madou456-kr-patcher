use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};

const RAW_SECTOR_SIZE: usize = 2_352;
const MODE1_DATA_OFFSET: usize = 16;
const ISO_SECTOR_SIZE: usize = 2_048;
const PRIMARY_VOLUME_DESCRIPTOR_LBA: usize = 16;

pub(crate) const EXPECTED_VOLUME_ID: &str = "DS_VOL.9";
pub(crate) const EXPECTED_GAME_FILE_COUNT: usize = 176;
pub(crate) const EXPECTED_GAME_TOTAL_SIZE: usize = 1_686_992;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GameFile {
    pub(crate) display_name: String,
    pub(crate) short_name: [u8; 11],
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct IsoDirectoryRecord {
    name: String,
    extent_lba: usize,
    data_length: usize,
    is_directory: bool,
}

pub(crate) fn extract_game_files(image: &[u8]) -> Result<Vec<GameFile>> {
    ensure!(
        image.len().is_multiple_of(RAW_SECTOR_SIZE),
        "CD image size is not a whole number of 2352-byte raw sectors"
    );
    verify_raw_sector(image, PRIMARY_VOLUME_DESCRIPTOR_LBA)?;
    let pvd = user_sector(image, PRIMARY_VOLUME_DESCRIPTOR_LBA)?;
    ensure!(
        pvd[0] == 1 && &pvd[1..6] == b"CD001" && pvd[6] == 1,
        "CD image has no ISO 9660 primary volume descriptor at raw LBA 16"
    );
    let volume_id = String::from_utf8_lossy(&pvd[40..72]).trim().to_owned();
    ensure!(
        volume_id == EXPECTED_VOLUME_ID,
        "unexpected CD volume identifier: expected {EXPECTED_VOLUME_ID}, got {volume_id:?}"
    );

    let root = parse_directory_record(&pvd[156..])?
        .context("ISO 9660 primary volume descriptor has no root directory record")?;
    ensure!(root.is_directory, "ISO 9660 root record is not a directory");
    let data = find_child(image, &root, "DS9_DATA", true)?;
    let game = find_child(image, &data, "MADOU456", true)?;

    let mut files = Vec::new();
    for record in read_directory(image, &game)? {
        if record.name == "." || record.name == ".." {
            continue;
        }
        ensure!(
            !record.is_directory,
            "unexpected subdirectory in DS9_DATA/MADOU456: {}",
            record.name
        );
        let display_name = record
            .name
            .split(';')
            .next()
            .unwrap_or(&record.name)
            .to_owned();
        let short_name = encode_short_name(&display_name)
            .with_context(|| format!("unsupported game filename {display_name:?}"))?;
        let bytes = read_extent(image, record.extent_lba, record.data_length)
            .with_context(|| format!("could not read game file {display_name}"))?;
        files.push(GameFile {
            display_name,
            short_name,
            bytes,
        });
    }
    files.sort_by_key(|file| file.short_name);

    let unique_names: BTreeSet<_> = files.iter().map(|file| file.short_name).collect();
    ensure!(
        unique_names.len() == files.len(),
        "CD game directory contains duplicate DOS short names"
    );
    ensure!(
        files.len() == EXPECTED_GAME_FILE_COUNT,
        "unexpected game file count: expected {EXPECTED_GAME_FILE_COUNT}, got {}",
        files.len()
    );
    let total_size: usize = files.iter().map(|file| file.bytes.len()).sum();
    ensure!(
        total_size == EXPECTED_GAME_TOTAL_SIZE,
        "unexpected game payload size: expected {EXPECTED_GAME_TOTAL_SIZE}, got {total_size}"
    );
    Ok(files)
}

pub(crate) fn encode_short_name(name: &str) -> Result<[u8; 11]> {
    ensure!(name.is_ascii(), "DOS short name is not ASCII");
    let upper = name.to_ascii_uppercase();
    let mut parts = upper.split('.');
    let base = parts.next().unwrap_or_default();
    let extension = parts.next().unwrap_or_default();
    ensure!(
        parts.next().is_none(),
        "DOS short name has more than one dot"
    );
    ensure!(
        !base.is_empty() && base.len() <= 8,
        "DOS base name must contain 1 to 8 bytes"
    );
    ensure!(
        extension.len() <= 3,
        "DOS extension must contain at most 3 bytes"
    );
    for byte in base.bytes().chain(extension.bytes()) {
        ensure!(
            byte.is_ascii_alphanumeric() || b"$%'-_@~`!(){}^#&".contains(&byte),
            "DOS short name contains unsupported byte 0x{byte:02x}"
        );
    }
    let mut encoded = [b' '; 11];
    encoded[..base.len()].copy_from_slice(base.as_bytes());
    encoded[8..8 + extension.len()].copy_from_slice(extension.as_bytes());
    Ok(encoded)
}

fn find_child(
    image: &[u8],
    parent: &IsoDirectoryRecord,
    wanted_name: &str,
    wanted_directory: bool,
) -> Result<IsoDirectoryRecord> {
    let mut matches = read_directory(image, parent)?.into_iter().filter(|record| {
        record.name.split(';').next() == Some(wanted_name)
            && record.is_directory == wanted_directory
    });
    let found = matches
        .next()
        .with_context(|| format!("ISO 9660 path component {wanted_name:?} is missing"))?;
    ensure!(
        matches.next().is_none(),
        "ISO 9660 path component {wanted_name:?} is ambiguous"
    );
    Ok(found)
}

fn read_directory(image: &[u8], directory: &IsoDirectoryRecord) -> Result<Vec<IsoDirectoryRecord>> {
    ensure!(
        directory.is_directory,
        "attempted to read a non-directory ISO record"
    );
    let bytes = read_extent(image, directory.extent_lba, directory.data_length)?;
    let mut records = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let record_length = bytes[offset] as usize;
        if record_length == 0 {
            offset = ((offset / ISO_SECTOR_SIZE) + 1) * ISO_SECTOR_SIZE;
            continue;
        }
        let end = offset
            .checked_add(record_length)
            .context("ISO directory record range overflow")?;
        ensure!(
            end <= bytes.len(),
            "ISO directory record exceeds its extent"
        );
        if let Some(record) = parse_directory_record(&bytes[offset..end])? {
            records.push(record);
        }
        offset = end;
    }
    Ok(records)
}

fn parse_directory_record(bytes: &[u8]) -> Result<Option<IsoDirectoryRecord>> {
    if bytes.is_empty() || bytes[0] == 0 {
        return Ok(None);
    }
    let record_length = bytes[0] as usize;
    ensure!(record_length >= 34, "ISO directory record is too short");
    ensure!(
        record_length <= bytes.len(),
        "truncated ISO directory record"
    );
    let name_length = bytes[32] as usize;
    ensure!(
        33 + name_length <= record_length,
        "truncated ISO directory identifier"
    );
    let name_bytes = &bytes[33..33 + name_length];
    let name = match name_bytes {
        [0] => ".".to_owned(),
        [1] => "..".to_owned(),
        _ if name_bytes.is_ascii() => String::from_utf8(name_bytes.to_vec())?,
        _ => bail!("non-ASCII ISO directory identifier is unsupported"),
    };
    let extent_lba = u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize;
    let data_length = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    Ok(Some(IsoDirectoryRecord {
        name,
        extent_lba,
        data_length,
        is_directory: bytes[25] & 0x02 != 0,
    }))
}

fn read_extent(image: &[u8], start_lba: usize, data_length: usize) -> Result<Vec<u8>> {
    let sector_count = data_length.div_ceil(ISO_SECTOR_SIZE);
    let mut output = Vec::with_capacity(sector_count * ISO_SECTOR_SIZE);
    for relative_lba in 0..sector_count {
        output.extend_from_slice(user_sector(image, start_lba + relative_lba)?);
    }
    output.truncate(data_length);
    Ok(output)
}

fn user_sector(image: &[u8], lba: usize) -> Result<&[u8]> {
    verify_raw_sector(image, lba)?;
    let start = lba
        .checked_mul(RAW_SECTOR_SIZE)
        .and_then(|offset| offset.checked_add(MODE1_DATA_OFFSET))
        .context("raw CD sector offset overflow")?;
    let end = start + ISO_SECTOR_SIZE;
    image
        .get(start..end)
        .with_context(|| format!("raw CD image is missing LBA {lba}"))
}

fn verify_raw_sector(image: &[u8], lba: usize) -> Result<()> {
    let start = lba
        .checked_mul(RAW_SECTOR_SIZE)
        .context("raw CD LBA overflow")?;
    let header = image
        .get(start..start + MODE1_DATA_OFFSET)
        .with_context(|| format!("raw CD image is missing sector header at LBA {lba}"))?;
    ensure!(
        header[0] == 0
            && header[1..11].iter().all(|byte| *byte == 0xff)
            && header[11] == 0
            && header[15] == 1,
        "LBA {lba} is not a raw Mode 1 sector"
    );
    Ok(())
}

#[cfg(test)]
#[path = "source_cd_tests.rs"]
mod tests;
