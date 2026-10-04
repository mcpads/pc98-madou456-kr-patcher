use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use pc98_madou456_builder::{build_reassembled_hdi, build_verified_localized_hdi, verify_sources};

#[derive(Parser)]
#[command(about = "PC-98 Madou 456 standalone HDI reassembler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify the exact Disc Station Vol. 09 CD and system HDI inputs.
    VerifySources {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
    },
    /// Reassemble a standalone HDI without modifying either input.
    Build {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "OUTPUT.HDI")]
        output: PathBuf,
    },
    /// Build a standalone HDI with all source-bound Korean game-text replacements.
    BuildLocalized {
        #[arg(long, value_name = "DISC_STATION_VOL09.IMG")]
        source_cd: PathBuf,
        #[arg(long, value_name = "SYSTEM.HDI")]
        system_hdi: PathBuf,
        #[arg(long, value_name = "TRANSLATION_CATALOG.JSON")]
        translation_catalog: PathBuf,
        #[arg(long, value_name = "OUTPUT.HDI")]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let report = match Cli::parse().command {
        Command::VerifySources {
            source_cd,
            system_hdi,
        } => serde_json::to_value(verify_sources(&source_cd, &system_hdi)?)?,
        Command::Build {
            source_cd,
            system_hdi,
            output,
        } => serde_json::to_value(build_reassembled_hdi(&source_cd, &system_hdi, &output)?)?,
        Command::BuildLocalized {
            source_cd,
            system_hdi,
            translation_catalog,
            output,
        } => serde_json::to_value(build_verified_localized_hdi(
            &source_cd,
            &system_hdi,
            &translation_catalog,
            &output,
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
