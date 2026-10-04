use std::sync::OnceLock;

use anyhow::{Context, Result, bail, ensure};
use fontdue::{Font, FontSettings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const GLYPH_WIDTH: usize = 16;
pub(crate) const GLYPH_HEIGHT: usize = 16;
pub(crate) const GLYPH_BYTES: usize = 32;

const PROFILE_SCHEMA: &str = "pc98_madou456.font_profile";
const BODY_PROFILE_JSON: &str = include_str!("../assets/fonts/galmuri14-pc98-16x16.json");
const UTILITY_LETTERING_PROFILE_JSON: &str =
    include_str!("../assets/fonts/neodunggeunmo-utility-lettering-16x16.json");
const SELECTION_PROMPT_PROFILE_JSON: &str =
    include_str!("../assets/fonts/bmjua-selection-prompt-16x16.json");

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum FontRole {
    Body,
    UtilityLettering,
    SelectionPrompt,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct FontReport {
    pub(crate) profile_id: String,
    pub(crate) font: String,
    pub(crate) font_sha256: String,
    pub(crate) font_version: String,
    pub(crate) license: String,
    pub(crate) source: String,
    pub(crate) upstream_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FontProfile {
    schema: String,
    id: String,
    font: String,
    font_sha256: String,
    font_version: String,
    font_size: u16,
    baseline_y: u8,
    vertical_fit: String,
    threshold: u8,
    rasterizer: String,
    license: String,
    source: String,
    upstream_revision: String,
}

struct EmbeddedFont {
    font: Font,
    profile: FontProfile,
}

static BODY_FONT: OnceLock<Result<EmbeddedFont, String>> = OnceLock::new();
static UTILITY_LETTERING_FONT: OnceLock<Result<EmbeddedFont, String>> = OnceLock::new();
static SELECTION_PROMPT_FONT: OnceLock<Result<EmbeddedFont, String>> = OnceLock::new();

pub(crate) fn font_report() -> Result<FontReport> {
    font_report_for_role(FontRole::Body)
}

pub(crate) fn font_report_for_role(role: FontRole) -> Result<FontReport> {
    let embedded = embedded_font(role)?;
    Ok(FontReport {
        profile_id: embedded.profile.id.clone(),
        font: embedded.profile.font.clone(),
        font_sha256: embedded.profile.font_sha256.clone(),
        font_version: embedded.profile.font_version.clone(),
        license: embedded.profile.license.clone(),
        source: embedded.profile.source.clone(),
        upstream_revision: embedded.profile.upstream_revision.clone(),
    })
}

pub(crate) fn rasterize_character(character: char) -> Result<[u8; GLYPH_BYTES]> {
    rasterize_character_for_role(character, FontRole::Body)
}

pub(crate) fn rasterize_character_for_role(
    character: char,
    role: FontRole,
) -> Result<[u8; GLYPH_BYTES]> {
    embedded_font(role)?.rasterize(character)
}

pub(crate) fn rasterize_character_in_cell(
    character: char,
    role: FontRole,
    font_size: u16,
    cell_width: usize,
    cell_height: usize,
) -> Result<Vec<bool>> {
    embedded_font(role)?.rasterize_in_cell(character, font_size, cell_width, cell_height)
}

fn embedded_font(role: FontRole) -> Result<&'static EmbeddedFont> {
    let embedded = match role {
        FontRole::Body => BODY_FONT.get_or_init(|| {
            EmbeddedFont::load_supplied(BODY_PROFILE_JSON, "Galmuri14.ttf", "Galmuri-OFL.txt")
                .map_err(|error| format!("{error:#}"))
        }),
        FontRole::UtilityLettering => UTILITY_LETTERING_FONT.get_or_init(|| {
            EmbeddedFont::load_supplied(
                UTILITY_LETTERING_PROFILE_JSON,
                "NeoDunggeunmo.ttf",
                "NeoDunggeunmo-OFL.txt",
            )
            .map_err(|error| format!("{error:#}"))
        }),
        FontRole::SelectionPrompt => SELECTION_PROMPT_FONT.get_or_init(|| {
            EmbeddedFont::load_supplied(SELECTION_PROMPT_PROFILE_JSON, "BMJUA.ttf", "BMJUA-OFL.txt")
                .map_err(|error| format!("{error:#}"))
        }),
    };
    match embedded {
        Ok(font) => Ok(font),
        Err(error) => bail!("load embedded font: {error}"),
    }
}

impl EmbeddedFont {
    fn load_supplied(
        profile_json: &str,
        expected_font_name: &str,
        expected_license_name: &str,
    ) -> Result<Self> {
        let font_bytes = crate::private_input::read_bytes(&format!("fonts/{expected_font_name}"))?;
        let font_license =
            crate::private_input::read_text(&format!("fonts/{expected_license_name}"))?;
        Self::load(
            font_bytes,
            profile_json,
            font_license,
            expected_font_name,
            expected_license_name,
        )
    }

    fn load(
        font_bytes: &'static [u8],
        profile_json: &str,
        font_license: &str,
        expected_font_name: &str,
        expected_license_name: &str,
    ) -> Result<Self> {
        let profile: FontProfile = serde_json::from_str(profile_json)?;
        verify_profile(
            &profile,
            font_bytes,
            font_license,
            expected_font_name,
            expected_license_name,
        )?;
        let font = Font::from_bytes(font_bytes, FontSettings::default())
            .map_err(|error| anyhow::anyhow!("parse embedded font: {error}"))?;
        Ok(Self { font, profile })
    }

    fn rasterize(&self, character: char) -> Result<[u8; GLYPH_BYTES]> {
        ensure!(
            self.font.has_glyph(character),
            "embedded font has no glyph for {character:?} (U+{:04X})",
            character as u32
        );
        let (metrics, coverage) = self
            .font
            .rasterize(character, f32::from(self.profile.font_size));
        ensure!(
            metrics.width > 0 && metrics.height > 0 && !coverage.is_empty(),
            "embedded font rendered no pixels for {character:?}"
        );
        ensure!(
            metrics.width <= GLYPH_WIDTH && metrics.height <= GLYPH_HEIGHT,
            "glyph {character:?} is {}x{} and does not fit {GLYPH_WIDTH}x{GLYPH_HEIGHT}",
            metrics.width,
            metrics.height
        );

        let left = (GLYPH_WIDTH - metrics.width) / 2;
        let baseline_top =
            i32::from(self.profile.baseline_y) - (metrics.ymin + metrics.height as i32);
        let top = baseline_top.clamp(0, (GLYPH_HEIGHT - metrics.height) as i32) as usize;
        let mut bitmap = [0_u8; GLYPH_BYTES];
        for source_y in 0..metrics.height {
            for source_x in 0..metrics.width {
                if coverage[source_y * metrics.width + source_x] < self.profile.threshold {
                    continue;
                }
                let x = left + source_x;
                let y = top + source_y;
                bitmap[y * 2 + x / 8] |= 1 << (7 - x % 8);
            }
        }
        ensure!(
            bitmap.iter().any(|byte| *byte != 0),
            "embedded font rendered an empty bitmap for {character:?}"
        );
        Ok(bitmap)
    }

    fn rasterize_in_cell(
        &self,
        character: char,
        font_size: u16,
        cell_width: usize,
        cell_height: usize,
    ) -> Result<Vec<bool>> {
        ensure!(
            (1..=96).contains(&font_size) && cell_width > 0 && cell_height > 0,
            "invalid native glyph cell or font size"
        );
        ensure!(
            self.font.has_glyph(character),
            "embedded font has no glyph for {character:?} (U+{:04X})",
            character as u32
        );
        let pixel_size = f32::from(font_size);
        let (metrics, coverage) = self.font.rasterize(character, pixel_size);
        ensure!(
            metrics.width > 0 && metrics.height > 0 && !coverage.is_empty(),
            "embedded font rendered no pixels for {character:?}"
        );
        ensure!(
            metrics.width <= cell_width && metrics.height <= cell_height,
            "glyph {character:?} is {}x{} and does not fit {cell_width}x{cell_height}",
            metrics.width,
            metrics.height
        );

        let ascent = self
            .font
            .horizontal_line_metrics(pixel_size)
            .context("embedded font has no horizontal line metrics")?
            .ascent as i32;
        let left = (cell_width - metrics.width) / 2;
        let centered_baseline = (cell_height as i32 + ascent) / 2;
        let baseline_top = centered_baseline - (metrics.ymin + metrics.height as i32);
        let top = baseline_top.clamp(0, (cell_height - metrics.height) as i32) as usize;
        let mut pixels = vec![false; cell_width * cell_height];
        for source_y in 0..metrics.height {
            for source_x in 0..metrics.width {
                if coverage[source_y * metrics.width + source_x] < self.profile.threshold {
                    continue;
                }
                pixels[(top + source_y) * cell_width + left + source_x] = true;
            }
        }
        ensure!(
            pixels.iter().any(|pixel| *pixel),
            "embedded font rendered an empty native-cell bitmap for {character:?}"
        );
        Ok(pixels)
    }
}

fn verify_profile(
    profile: &FontProfile,
    font_bytes: &[u8],
    font_license: &str,
    expected_font_name: &str,
    expected_license_name: &str,
) -> Result<()> {
    ensure!(
        profile.schema == PROFILE_SCHEMA,
        "unsupported embedded font profile schema"
    );
    ensure!(
        profile.font == expected_font_name,
        "embedded font profile names an unexpected font"
    );
    ensure!(
        profile.font_sha256 == sha256_hex(font_bytes),
        "embedded font SHA-256 does not match its profile"
    );
    ensure!(
        profile.license == expected_license_name && font_license.contains("SIL OPEN FONT LICENSE"),
        "embedded font profile has no matching OFL license"
    );
    ensure!(
        profile.rasterizer == "fontdue 0.9.3",
        "embedded font profile names an unexpected rasterizer"
    );
    ensure!(
        profile.vertical_fit == "baseline_then_clamp",
        "embedded font profile names an unexpected vertical-fit policy"
    );
    ensure!(
        (1..=32).contains(&profile.font_size),
        "embedded font size is outside 1..=32"
    );
    ensure!(
        usize::from(profile.baseline_y) <= GLYPH_HEIGHT,
        "embedded font baseline is outside the glyph cell"
    );
    ensure!(
        profile.threshold > 0,
        "embedded font threshold must be nonzero"
    );
    ensure!(
        !profile.font_version.is_empty()
            && !profile.source.is_empty()
            && !profile.upstream_revision.is_empty(),
        "embedded font profile has incomplete provenance"
    );
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "font_tests.rs"]
mod tests;
