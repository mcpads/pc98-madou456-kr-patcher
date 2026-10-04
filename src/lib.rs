mod compile_lz;
mod expected_write;
mod fat16;
mod font;
#[cfg(feature = "analysis")]
mod game_files;
#[cfg(feature = "analysis")]
mod graphic_comparison;
#[cfg(feature = "analysis")]
mod graphic_export;
mod graphic_localization;
mod josa;
mod localization;
mod mado456_ui;
mod masked_tile;
mod message_buffers;
mod private_input;
mod reassembly;
mod source_cd;
#[cfg(feature = "analysis")]
mod static_analysis;
mod translation_assets;
mod translation_font;
mod translation_renderer;

#[cfg(feature = "analysis")]
pub use game_files::{
    DecodedGameFileExtractionReport, GameFileExtractionReport, extract_verified_decoded_game_files,
    extract_verified_game_files,
};
#[cfg(feature = "analysis")]
pub use graphic_comparison::{
    GraphicTextComparisonExportReport, export_verified_graphic_text_comparisons,
};
#[cfg(feature = "analysis")]
pub use graphic_export::{GraphicAtlasExportReport, export_verified_masked_tile_atlases};
pub use localization::{
    LocalizedGameFilesWriteReport, LocalizedHdiBuildReport, TranslationContextAuditReport,
    TranslationDraftAuditReport, TranslationOverlayAuditReport,
    TranslationOverlayCatalogAuditReport, audit_verified_translation_contexts,
    audit_verified_translation_draft, audit_verified_translation_overlay,
    audit_verified_translation_overlay_catalog, build_verified_localized_hdi,
    write_verified_localized_game_files,
};
#[cfg(feature = "analysis")]
pub use localization::{MessageOverflowDetectionReport, detect_verified_message_overflow};
pub use reassembly::{BuildReport, SourceReport, build_reassembled_hdi, verify_sources};
#[cfg(feature = "analysis")]
pub use static_analysis::{
    GameAssetCatalogWriteReport, GameStaticAnalysisReport, InlineTextCatalogWriteReport,
    MessageCatalogWriteReport, MessageReinsertionWriteReport, TranslationWorkspaceWriteReport,
    analyze_verified_game, write_verified_game_asset_catalog, write_verified_inline_text_catalog,
    write_verified_message_catalog, write_verified_rebuilt_message_files,
    write_verified_translation_workspace,
};
pub use translation_assets::{TranslationAssetAuditReport, audit_translation_assets};
