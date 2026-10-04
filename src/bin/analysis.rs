use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use pc98_madou456_builder::{
    analyze_verified_game, audit_translation_assets, audit_verified_translation_contexts,
    audit_verified_translation_draft, audit_verified_translation_overlay,
    audit_verified_translation_overlay_catalog, detect_verified_message_overflow,
    export_verified_graphic_text_comparisons, export_verified_masked_tile_atlases,
    extract_verified_decoded_game_files, extract_verified_game_files,
    write_verified_game_asset_catalog, write_verified_inline_text_catalog,
    write_verified_localized_game_files, write_verified_message_catalog,
    write_verified_rebuilt_message_files, write_verified_translation_workspace,
};

#[derive(Parser)]
#[command(about = "PC-98 Madou 456 explicit static-analysis tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract the verified game files for local static analysis.
    ExtractGameFiles {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
    /// Decode every exact Compile-LZ game file for local static analysis.
    ExtractDecodedGameFiles {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
    /// Render verified 40,960-byte masked tile atlases to private diagnostic PPMs.
    ExportMaskedTileAtlases {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        /// Exact game filename to export; omit to export every matching atlas.
        #[arg(long = "name", value_name = "ASSET.DAT")]
        names: Vec<String>,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
    /// Export private original-versus-localized graphic-text surface comparisons.
    ExportGraphicTextComparisons {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
    /// Survey verified files, exact Compile-LZ streams, and game structures.
    AnalyzeGame {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
    },
    /// Write a source-free catalog of verified game assets and structures.
    CatalogGameAssets {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT.JSON")]
        output: PathBuf,
    },
    /// Write a source-preserving, decoded message catalog for local analysis.
    CatalogMessages {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT.JSON")]
        output: PathBuf,
    },
    /// Write a source-preserving catalog of executable inline game text.
    CatalogInlineText {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT.JSON")]
        output: PathBuf,
    },
    /// Prepare one private, source-bound workspace for game-text translation.
    PrepareTranslationWorkspace {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
    /// Audit a source-bound, source-free Korean translation draft.
    AuditTranslationDraft {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "TRANSLATION_DRAFT.JSON")]
        draft: PathBuf,
    },
    /// Audit source-free translation policy and terminology assets.
    AuditTranslationAssets {
        #[arg(long, value_name = "POLICY.JSON")]
        policy: PathBuf,
        #[arg(long, value_name = "TERMINOLOGY.JSON")]
        terminology: PathBuf,
    },
    /// Audit source-bound review contexts and the representative first-draft sample.
    AuditTranslationContexts {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "CONTEXTS.JSON")]
        contexts: PathBuf,
        #[arg(long, value_name = "REPRESENTATIVE_SAMPLE.JSON")]
        representative_sample: PathBuf,
    },
    /// Audit the source-free Korean wording selected for the representative sample.
    AuditTranslationOverlay {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "CONTEXTS.JSON")]
        contexts: PathBuf,
        #[arg(long, value_name = "REPRESENTATIVE_SAMPLE.JSON")]
        selection: PathBuf,
        #[arg(long, value_name = "TRANSLATION_OVERLAY.JSON")]
        overlay: PathBuf,
    },
    /// Audit every non-overlapping Korean translation batch in a source-bound catalog.
    AuditTranslationCatalog {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "TRANSLATION_CATALOG.JSON")]
        catalog: PathBuf,
    },
    /// Detect horizontal and vertical overflow without changing translations or inserting line breaks.
    DetectMessageOverflow {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "TRANSLATION_CATALOG.JSON")]
        catalog: PathBuf,
    },
    /// Rebuild all fifteen packed message files from a source-bound translation draft.
    RebuildMessageFiles {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "TRANSLATION_DRAFT.JSON")]
        draft: PathBuf,
        #[arg(long, value_name = "TRANSLATION_CATALOG.JSON")]
        translation_catalog: Option<PathBuf>,
        #[arg(long, value_name = "CODEBOOK.JSON")]
        codebook: Option<PathBuf>,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
    /// Build every tracked text translation and its renderer support as private game files.
    RebuildLocalizedGameFiles {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "TRANSLATION_CATALOG.JSON")]
        translation_catalog: PathBuf,
        #[arg(long, value_name = "OUTPUT_DIRECTORY")]
        output_dir: PathBuf,
    },
}

fn main() -> Result<()> {
    let report = match Cli::parse().command {
        Command::ExtractGameFiles {
            source_cd,
            system_hdi,
            output_dir,
        } => serde_json::to_value(extract_verified_game_files(
            &source_cd,
            &system_hdi,
            &output_dir,
        )?)?,
        Command::ExtractDecodedGameFiles {
            source_cd,
            system_hdi,
            output_dir,
        } => serde_json::to_value(extract_verified_decoded_game_files(
            &source_cd,
            &system_hdi,
            &output_dir,
        )?)?,
        Command::ExportMaskedTileAtlases {
            source_cd,
            system_hdi,
            names,
            output_dir,
        } => serde_json::to_value(export_verified_masked_tile_atlases(
            &source_cd,
            &system_hdi,
            &names,
            &output_dir,
        )?)?,
        Command::ExportGraphicTextComparisons {
            source_cd,
            system_hdi,
            output_dir,
        } => serde_json::to_value(export_verified_graphic_text_comparisons(
            &source_cd,
            &system_hdi,
            &output_dir,
        )?)?,
        Command::AnalyzeGame {
            source_cd,
            system_hdi,
        } => serde_json::to_value(analyze_verified_game(&source_cd, &system_hdi)?)?,
        Command::CatalogGameAssets {
            source_cd,
            system_hdi,
            output,
        } => serde_json::to_value(write_verified_game_asset_catalog(
            &source_cd,
            &system_hdi,
            &output,
        )?)?,
        Command::CatalogMessages {
            source_cd,
            system_hdi,
            output,
        } => serde_json::to_value(write_verified_message_catalog(
            &source_cd,
            &system_hdi,
            &output,
        )?)?,
        Command::CatalogInlineText {
            source_cd,
            system_hdi,
            output,
        } => serde_json::to_value(write_verified_inline_text_catalog(
            &source_cd,
            &system_hdi,
            &output,
        )?)?,
        Command::PrepareTranslationWorkspace {
            source_cd,
            system_hdi,
            output_dir,
        } => serde_json::to_value(write_verified_translation_workspace(
            &source_cd,
            &system_hdi,
            &output_dir,
        )?)?,
        Command::AuditTranslationDraft {
            source_cd,
            system_hdi,
            draft,
        } => serde_json::to_value(audit_verified_translation_draft(
            &source_cd,
            &system_hdi,
            &draft,
        )?)?,
        Command::AuditTranslationAssets {
            policy,
            terminology,
        } => serde_json::to_value(audit_translation_assets(&policy, &terminology)?)?,
        Command::AuditTranslationContexts {
            source_cd,
            system_hdi,
            contexts,
            representative_sample,
        } => serde_json::to_value(audit_verified_translation_contexts(
            &source_cd,
            &system_hdi,
            &contexts,
            &representative_sample,
        )?)?,
        Command::AuditTranslationOverlay {
            source_cd,
            system_hdi,
            contexts,
            selection,
            overlay,
        } => serde_json::to_value(audit_verified_translation_overlay(
            &source_cd,
            &system_hdi,
            &contexts,
            &selection,
            &overlay,
        )?)?,
        Command::AuditTranslationCatalog {
            source_cd,
            system_hdi,
            catalog,
        } => serde_json::to_value(audit_verified_translation_overlay_catalog(
            &source_cd,
            &system_hdi,
            &catalog,
        )?)?,
        Command::DetectMessageOverflow {
            source_cd,
            system_hdi,
            catalog,
        } => serde_json::to_value(detect_verified_message_overflow(
            &source_cd,
            &system_hdi,
            &catalog,
        )?)?,
        Command::RebuildMessageFiles {
            source_cd,
            system_hdi,
            draft,
            translation_catalog,
            codebook,
            output_dir,
        } => serde_json::to_value(write_verified_rebuilt_message_files(
            &source_cd,
            &system_hdi,
            &draft,
            translation_catalog.as_deref(),
            codebook.as_deref(),
            &output_dir,
        )?)?,
        Command::RebuildLocalizedGameFiles {
            source_cd,
            system_hdi,
            translation_catalog,
            output_dir,
        } => serde_json::to_value(write_verified_localized_game_files(
            &source_cd,
            &system_hdi,
            &translation_catalog,
            &output_dir,
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
#[path = "../analysis_cli_tests.rs"]
mod tests;
