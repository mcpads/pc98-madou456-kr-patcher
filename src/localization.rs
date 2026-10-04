use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use crate::source_cd::GameFile;

pub(crate) const COM_LOAD_ORIGIN: usize = 0x100;
pub(crate) const MADO456_SHA256: &str =
    "df31cddd940f33169a5c812a68000258ec0350cdcecbe4fdb036a5dc30597966";
pub(crate) const OPENING_SHA256: &str =
    "c9dc5839685f9a4d505659b656594bee8948d852e5d00ed657a6a71037d10db2";
pub(crate) const ENDING_SHA256: &str =
    "a95d2a854eb813adea064233a7215fdbae2e7461b732957bef9e1b7464315460";
#[cfg(feature = "analysis")]
pub(crate) const DRBIOS_SHA256: &str =
    "6762cd03975e26c07f98b78632adf8418d3648a518a0fbfc4815e96ae5b0d5f8";

#[path = "inline_text_analysis.rs"]
pub(crate) mod inline_text_analysis;
#[path = "localization_reinsertion.rs"]
mod localization_reinsertion;
#[path = "message_analysis.rs"]
pub(crate) mod message_analysis;
#[cfg(feature = "analysis")]
#[path = "message_layout.rs"]
mod message_layout;
#[path = "translation_context.rs"]
pub(crate) mod translation_context;
#[path = "translation_draft.rs"]
pub(crate) mod translation_draft;
#[path = "translation_overlay.rs"]
pub(crate) mod translation_overlay;

pub use localization_reinsertion::{
    LocalizedGameFilesWriteReport, LocalizedHdiBuildReport, build_verified_localized_hdi,
    write_verified_localized_game_files,
};
#[cfg(feature = "analysis")]
pub use message_layout::{MessageOverflowDetectionReport, detect_verified_message_overflow};
pub use translation_context::{TranslationContextAuditReport, audit_verified_translation_contexts};
pub use translation_draft::{TranslationDraftAuditReport, audit_verified_translation_draft};
pub use translation_overlay::{
    TranslationOverlayAuditReport, TranslationOverlayCatalogAuditReport,
    audit_verified_translation_overlay, audit_verified_translation_overlay_catalog,
};

pub(crate) use crate::reassembly::load_verified_game_files;
#[cfg(feature = "analysis")]
pub(crate) use inline_text_analysis::InlineTextAnalysisReport;
pub(crate) use inline_text_analysis::{InlineTextCatalog, catalog_inline_text};
pub(crate) use message_analysis::MessageCatalog;
#[cfg(not(feature = "analysis"))]
pub(crate) use message_analysis::catalog_messages;
#[cfg(feature = "analysis")]
pub(crate) use message_analysis::{
    FIRST_GLYPH_TABLE_FILE_OFFSET, MessageControlLayoutReport, MessageFileSummary,
    MessageFormatReport, SECOND_GLYPH_TABLE_FILE_OFFSET, catalog_messages, message_format_report,
};

pub(crate) fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16> {
    let word = bytes
        .get(offset..offset + 2)
        .with_context(|| format!("16-bit value at file offset {offset:#x} is truncated"))?;
    Ok(u16::from_le_bytes([word[0], word[1]]))
}

pub(crate) fn require_file<'a>(files: &'a [GameFile], wanted_name: &str) -> Result<&'a GameFile> {
    let mut matching = files.iter().filter(|file| file.display_name == wanted_name);
    let file = matching
        .next()
        .with_context(|| format!("verified game file {wanted_name} is missing"))?;
    ensure!(
        matching.next().is_none(),
        "verified game file {wanted_name} is duplicated"
    );
    Ok(file)
}

pub(crate) fn require_sha256(name: &str, bytes: &[u8], expected: &str) -> Result<()> {
    let observed = sha256_hex(bytes);
    ensure!(
        observed == expected,
        "{name} SHA-256 mismatch: expected {expected}, got {observed}"
    );
    Ok(())
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
