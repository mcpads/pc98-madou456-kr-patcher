use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::expected_write::FixedRangeExpectedWrite;
use crate::localization::message_analysis::RebuiltMessageFile;

const MADO456_REQUIRED_DOS_PARAGRAPHS_FILE_OFFSET: usize = 0x0113;
const MADO456_ALLOCATED_DOS_PARAGRAPHS_FILE_OFFSET: usize = 0x0120;
const MADO456_MESSAGE_REGION_PARAGRAPHS_FILE_OFFSET: usize = 0x015a;
const MADO456_SECONDARY_MESSAGE_SEGMENT_FILE_OFFSET: usize = 0x0e64;
const MADO456_SECONDARY_MESSAGE_OFFSET_FILE_OFFSET: usize = 0x12c8;

const SOURCE_DOS_ALLOCATION_PARAGRAPHS: u16 = 0x5400;
const LOCALIZED_DOS_ALLOCATION_PARAGRAPHS: u16 = 0x5500;
const SOURCE_MESSAGE_REGION_PARAGRAPHS: u16 = 0x03c0;
const LOCALIZED_MESSAGE_REGION_PARAGRAPHS: u16 = 0x04c0;
const SOURCE_SECONDARY_MESSAGE_PARAGRAPHS: u16 = 0x0180;
const LOCALIZED_SECONDARY_MESSAGE_PARAGRAPHS: u16 = 0x01b0;

const BYTES_PER_PARAGRAPH: usize = 16;
const PRIMARY_MESSAGE_BUFFER_BYTES: usize =
    LOCALIZED_SECONDARY_MESSAGE_PARAGRAPHS as usize * BYTES_PER_PARAGRAPH;
const MESSAGE_REGION_BYTES: usize =
    LOCALIZED_MESSAGE_REGION_PARAGRAPHS as usize * BYTES_PER_PARAGRAPH;
const SECONDARY_MESSAGE_BUFFER_BYTES: usize = MESSAGE_REGION_BYTES - PRIMARY_MESSAGE_BUFFER_BYTES;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MessageBufferLayoutReport {
    primary_file: &'static str,
    primary_capacity: usize,
    primary_used: usize,
    primary_remaining: usize,
    secondary_file_count: usize,
    secondary_capacity: usize,
    largest_secondary_file: String,
    largest_secondary_used: usize,
    largest_secondary_remaining: usize,
    message_region_size: usize,
    added_dos_memory_bytes: usize,
}

pub(crate) fn validate_message_buffer_layout(
    files: &[RebuiltMessageFile],
) -> Result<MessageBufferLayoutReport> {
    let mut primary_files = files.iter().filter(|file| file.name == "MSG.DAT");
    let primary = primary_files
        .next()
        .context("rebuilt messages lack the primary MSG.DAT buffer occupant")?;
    ensure!(
        primary_files.next().is_none(),
        "rebuilt messages repeat the primary MSG.DAT buffer occupant"
    );
    ensure!(
        primary.decoded.len() <= PRIMARY_MESSAGE_BUFFER_BYTES,
        "decoded MSG.DAT needs {} bytes but the localized primary message buffer holds {PRIMARY_MESSAGE_BUFFER_BYTES}",
        primary.decoded.len()
    );

    let secondary_files = files
        .iter()
        .filter(|file| file.name != "MSG.DAT")
        .collect::<Vec<_>>();
    let largest_secondary = secondary_files
        .iter()
        .max_by_key(|file| file.decoded.len())
        .context("rebuilt messages lack secondary buffer occupants")?;
    ensure!(
        largest_secondary.decoded.len() <= SECONDARY_MESSAGE_BUFFER_BYTES,
        "decoded {} needs {} bytes but the localized secondary message buffer holds {SECONDARY_MESSAGE_BUFFER_BYTES}",
        largest_secondary.name,
        largest_secondary.decoded.len()
    );

    Ok(MessageBufferLayoutReport {
        primary_file: "MSG.DAT",
        primary_capacity: PRIMARY_MESSAGE_BUFFER_BYTES,
        primary_used: primary.decoded.len(),
        primary_remaining: PRIMARY_MESSAGE_BUFFER_BYTES - primary.decoded.len(),
        secondary_file_count: secondary_files.len(),
        secondary_capacity: SECONDARY_MESSAGE_BUFFER_BYTES,
        largest_secondary_file: largest_secondary.name.clone(),
        largest_secondary_used: largest_secondary.decoded.len(),
        largest_secondary_remaining: SECONDARY_MESSAGE_BUFFER_BYTES
            - largest_secondary.decoded.len(),
        message_region_size: MESSAGE_REGION_BYTES,
        added_dos_memory_bytes: usize::from(
            LOCALIZED_DOS_ALLOCATION_PARAGRAPHS - SOURCE_DOS_ALLOCATION_PARAGRAPHS,
        ) * BYTES_PER_PARAGRAPH,
    })
}

pub(crate) fn mado456_message_buffer_expected_writes() -> [FixedRangeExpectedWrite; 5] {
    [
        FixedRangeExpectedWrite {
            owner: "mado456-message-buffer-allocation",
            purpose: "require enough DOS memory for the expanded localized message region",
            offset: MADO456_REQUIRED_DOS_PARAGRAPHS_FILE_OFFSET,
            expected_source: [0x81, 0xfb]
                .into_iter()
                .chain(SOURCE_DOS_ALLOCATION_PARAGRAPHS.to_le_bytes())
                .collect(),
            replacement: [0x81, 0xfb]
                .into_iter()
                .chain(LOCALIZED_DOS_ALLOCATION_PARAGRAPHS.to_le_bytes())
                .collect(),
        },
        FixedRangeExpectedWrite {
            owner: "mado456-message-buffer-allocation",
            purpose: "allocate enough DOS memory for the expanded localized message region",
            offset: MADO456_ALLOCATED_DOS_PARAGRAPHS_FILE_OFFSET,
            expected_source: [0xbb]
                .into_iter()
                .chain(SOURCE_DOS_ALLOCATION_PARAGRAPHS.to_le_bytes())
                .collect(),
            replacement: [0xbb]
                .into_iter()
                .chain(LOCALIZED_DOS_ALLOCATION_PARAGRAPHS.to_le_bytes())
                .collect(),
        },
        FixedRangeExpectedWrite {
            owner: "mado456-message-buffer-allocation",
            purpose: "move the packed-file scratch segment after the expanded message region",
            offset: MADO456_MESSAGE_REGION_PARAGRAPHS_FILE_OFFSET,
            expected_source: [0x05]
                .into_iter()
                .chain(SOURCE_MESSAGE_REGION_PARAGRAPHS.to_le_bytes())
                .collect(),
            replacement: [0x05]
                .into_iter()
                .chain(LOCALIZED_MESSAGE_REGION_PARAGRAPHS.to_le_bytes())
                .collect(),
        },
        FixedRangeExpectedWrite {
            owner: "mado456-message-buffer-split",
            purpose: "decompress secondary message files after the expanded primary buffer",
            offset: MADO456_SECONDARY_MESSAGE_SEGMENT_FILE_OFFSET,
            expected_source: [0x81, 0xc2]
                .into_iter()
                .chain(SOURCE_SECONDARY_MESSAGE_PARAGRAPHS.to_le_bytes())
                .collect(),
            replacement: [0x81, 0xc2]
                .into_iter()
                .chain(LOCALIZED_SECONDARY_MESSAGE_PARAGRAPHS.to_le_bytes())
                .collect(),
        },
        FixedRangeExpectedWrite {
            owner: "mado456-message-buffer-split",
            purpose: "resolve secondary message offsets from the relocated buffer",
            offset: MADO456_SECONDARY_MESSAGE_OFFSET_FILE_OFFSET,
            expected_source: (SOURCE_SECONDARY_MESSAGE_PARAGRAPHS * BYTES_PER_PARAGRAPH as u16)
                .to_le_bytes()
                .to_vec(),
            replacement: (LOCALIZED_SECONDARY_MESSAGE_PARAGRAPHS * BYTES_PER_PARAGRAPH as u16)
                .to_le_bytes()
                .to_vec(),
        },
    ]
}

#[cfg(test)]
#[path = "message_buffers_tests.rs"]
mod tests;
