use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use crate::graphic_localization::{
    GraphicSurfaceComparisonImage, build_graphic_surface_comparison_images,
};
use crate::localization::sha256_hex;
use crate::reassembly::load_verified_game_files;

fn surface_ledger_json() -> Result<&'static str> {
    crate::private_input::read_text("analysis/graphic-text-surfaces.json")
}

fn graphic_translations_json() -> Result<&'static str> {
    crate::private_input::read_text("translations/graphic-text.json")
}
const SURFACE_LEDGER_SCHEMA: &str = "pc98_madou456.graphic_text_surface_ledger";

#[derive(Debug, Serialize)]
pub struct GraphicTextComparisonExportReport {
    output_directory: PathBuf,
    surface_ledger: &'static str,
    surface_ledger_sha256: String,
    graphic_translation_asset: &'static str,
    graphic_translation_asset_sha256: String,
    tracked_group_count: usize,
    tracked_translation_unit_count: usize,
    visible_unowned_group_count: usize,
    rendered_pair_count: usize,
    comparison_kind: &'static str,
    limitation: &'static str,
    pairs: Vec<GraphicTextComparisonPairReport>,
    visible_unowned_groups: Vec<VisibleUnownedGroupReport>,
}

#[derive(Debug, Serialize)]
struct GraphicTextComparisonPairReport {
    id: String,
    unit_id: Option<&'static str>,
    group_id: String,
    source_asset: &'static str,
    consumer_program: &'static str,
    classification: &'static str,
    width: usize,
    height: usize,
    minimum_ink_height: Option<usize>,
    source_ink_bounds: Option<[usize; 4]>,
    localized_ink_bounds: Option<[usize; 4]>,
    source_ink_size: Option<[usize; 2]>,
    localized_ink_size: Option<[usize; 2]>,
    source_ink_palette_indices: Vec<u8>,
    localized_ink_palette_indices: Vec<u8>,
    localized_ink_palette_matches_source: Option<bool>,
    protected_background_pixels_changed: Option<usize>,
    localized_height_meets_minimum: Option<bool>,
    localized_top_matches_source: Option<bool>,
    localized_vertical_covers_source: Option<bool>,
    pair_file: String,
    pair_sha256: String,
    source_rgb_sha256: String,
    localized_rgb_sha256: String,
    pixels_changed: bool,
}

#[derive(Debug, Serialize)]
struct VisibleUnownedGroupReport {
    id: String,
    source_assets: Vec<String>,
    consumer_programs: Vec<String>,
    comparison_surfaces: Vec<String>,
    review_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphicTextSurfaceLedger {
    schema: String,
    scope: String,
    groups: Vec<GraphicTextSurfaceGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphicTextSurfaceGroup {
    id: String,
    profile: String,
    semantic_role: String,
    source_assets: Vec<String>,
    consumer_programs: Vec<String>,
    translation_unit_ids: Vec<String>,
    consumer_composition: String,
    source_presentation: String,
    localized_presentation: String,
    tracking_status: String,
    current_assessment: String,
    recommended_strategy: String,
    comparison_surfaces: Vec<String>,
    review_note: String,
}

pub fn export_verified_graphic_text_comparisons(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_directory: &Path,
) -> Result<GraphicTextComparisonExportReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing graphic comparison directory {}",
        output_directory.display()
    );
    let ledger = load_and_audit_surface_ledger()?;
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let images = build_graphic_surface_comparison_images(&files)?;
    let unit_groups = unit_group_map(&ledger)?;
    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create graphic comparison parent {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-graphic-comparison-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create graphic comparison staging directory in {}",
                parent.display()
            )
        })?;

    let mut pair_reports = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let group_id = image
            .unit_id
            .and_then(|unit_id| unit_groups.get(unit_id).copied())
            .unwrap_or("configuration-lettering");
        let file_name = format!("{:02}-{}.bmp", index + 1, image.id);
        let pair_rgb = side_by_side_rgb(image)?;
        let pair_width = image.width * 2 + 16;
        let bmp = encode_bmp(pair_width, image.height, &pair_rgb)?;
        write_new_file(staging.path(), &file_name, &bmp)?;
        pair_reports.push(GraphicTextComparisonPairReport {
            id: image.id.clone(),
            unit_id: image.unit_id,
            group_id: group_id.to_owned(),
            source_asset: image.source_asset,
            consumer_program: image.consumer_program,
            classification: image.classification,
            width: image.width,
            height: image.height,
            minimum_ink_height: image.minimum_ink_height,
            source_ink_bounds: image.source_ink_bounds,
            localized_ink_bounds: image.localized_ink_bounds,
            source_ink_size: image.source_ink_size,
            localized_ink_size: image.localized_ink_size,
            source_ink_palette_indices: image.source_ink_palette_indices.clone(),
            localized_ink_palette_indices: image.localized_ink_palette_indices.clone(),
            localized_ink_palette_matches_source: (!image.source_ink_palette_indices.is_empty()
                && !image.localized_ink_palette_indices.is_empty())
            .then_some(image.source_ink_palette_indices == image.localized_ink_palette_indices),
            protected_background_pixels_changed: image.protected_background_pixels_changed,
            localized_height_meets_minimum: image
                .minimum_ink_height
                .zip(image.localized_ink_size)
                .map(|(minimum, localized)| localized[1] >= minimum),
            localized_top_matches_source: image
                .source_ink_bounds
                .zip(image.localized_ink_bounds)
                .map(|(source, localized)| source[1] == localized[1]),
            localized_vertical_covers_source: image
                .source_ink_bounds
                .zip(image.localized_ink_bounds)
                .map(|(source, localized)| {
                    localized[1] <= source[1]
                        && localized[1] + localized[3] >= source[1] + source[3]
                }),
            pair_file: file_name,
            pair_sha256: sha256_hex(&bmp),
            source_rgb_sha256: sha256_hex(&image.source_rgb),
            localized_rgb_sha256: sha256_hex(&image.localized_rgb),
            pixels_changed: image.source_rgb != image.localized_rgb,
        });
    }

    let visible_unowned_groups = ledger
        .groups
        .iter()
        .filter(|group| group.tracking_status == "visible_unowned")
        .map(|group| VisibleUnownedGroupReport {
            id: group.id.clone(),
            source_assets: group.source_assets.clone(),
            consumer_programs: group.consumer_programs.clone(),
            comparison_surfaces: group.comparison_surfaces.clone(),
            review_note: group.review_note.clone(),
        })
        .collect::<Vec<_>>();
    let report = GraphicTextComparisonExportReport {
        output_directory: output_directory.to_path_buf(),
        surface_ledger: "assets/analysis/graphic-text-surfaces.json",
        surface_ledger_sha256: sha256_hex(surface_ledger_json()?.as_bytes()),
        graphic_translation_asset: "assets/translations/graphic-text.json",
        graphic_translation_asset_sha256: sha256_hex(graphic_translations_json()?.as_bytes()),
        tracked_group_count: ledger.groups.len(),
        tracked_translation_unit_count: unit_groups.len(),
        visible_unowned_group_count: visible_unowned_groups.len(),
        rendered_pair_count: pair_reports.len(),
        comparison_kind: "static descriptor-and-atlas source versus localized mask composition",
        limitation: "palette indexes use an arbitrary diagnostic RGB mapping; protected background changes are checked by source palette index, but this is not runtime evidence, artistic approval, or distribution eligibility",
        pairs: pair_reports,
        visible_unowned_groups,
    };
    let manifest = serde_json::to_vec_pretty(&report)?;
    write_new_file(staging.path(), "manifest.json", &manifest)?;
    let html = render_review_html(&ledger, &report);
    write_new_file(staging.path(), "index.html", html.as_bytes())?;

    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish graphic comparison directory {}",
            output_directory.display()
        )
    })?;
    Ok(report)
}

fn load_and_audit_surface_ledger() -> Result<GraphicTextSurfaceLedger> {
    let ledger: GraphicTextSurfaceLedger = serde_json::from_str(surface_ledger_json()?)
        .context("could not parse graphic-text surface ledger")?;
    ensure!(
        ledger.schema == SURFACE_LEDGER_SCHEMA && !ledger.scope.trim().is_empty(),
        "graphic-text surface ledger schema or scope changed"
    );
    let expected_groups = BTreeSet::from([
        "configuration-lettering",
        "ending-calligraphy-and-credits",
        "map-labels",
        "selection-lettering",
        "title-menu-lettering",
        "title-wordmark",
    ]);
    let observed_groups = ledger
        .groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        ledger.groups.len() == observed_groups.len() && observed_groups == expected_groups,
        "graphic-text surface group population changed"
    );
    for group in &ledger.groups {
        ensure!(
            !group.profile.trim().is_empty()
                && !group.semantic_role.trim().is_empty()
                && !group.source_assets.is_empty()
                && !group.consumer_programs.is_empty()
                && !group.consumer_composition.trim().is_empty()
                && !group.source_presentation.trim().is_empty()
                && !group.localized_presentation.trim().is_empty()
                && !group.current_assessment.trim().is_empty()
                && !group.recommended_strategy.trim().is_empty()
                && !group.comparison_surfaces.is_empty()
                && !group.review_note.trim().is_empty(),
            "graphic-text surface group {} is incomplete",
            group.id
        );
        ensure!(
            group.source_assets.iter().all(|name| safe_asset_name(name))
                && group
                    .consumer_programs
                    .iter()
                    .all(|name| safe_asset_name(name)),
            "graphic-text surface group {} contains an unsafe filename",
            group.id
        );
    }
    let visible_unowned = ledger
        .groups
        .iter()
        .filter(|group| group.tracking_status == "visible_unowned")
        .collect::<Vec<_>>();
    ensure!(
        visible_unowned.is_empty(),
        "the visible unowned graphic-text boundary changed"
    );
    let title_wordmark = ledger
        .groups
        .iter()
        .find(|group| group.id == "title-wordmark")
        .context("title wordmark surface group is missing")?;
    ensure!(
        title_wordmark.tracking_status == "mapped_preserve_source"
            && title_wordmark.translation_unit_ids.is_empty()
            && title_wordmark
                .localized_presentation
                .contains("byte-for-byte"),
        "the title wordmark source-preservation policy changed"
    );

    let catalog: serde_json::Value = serde_json::from_str(graphic_translations_json()?)
        .context("could not parse graphic-text translation catalog")?;
    let expected_units = catalog["units"]
        .as_array()
        .context("graphic-text translation catalog has no units")?
        .iter()
        .map(|unit| {
            unit["id"]
                .as_str()
                .context("graphic-text translation unit has no id")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let unit_groups = unit_group_map(&ledger)?;
    let observed_units = unit_groups.keys().copied().collect::<BTreeSet<_>>();
    ensure!(
        expected_units.len() == 32 && observed_units == expected_units,
        "graphic-text surface ledger does not own the exact 32-unit translation catalog"
    );
    Ok(ledger)
}

fn unit_group_map(ledger: &GraphicTextSurfaceLedger) -> Result<BTreeMap<&str, &str>> {
    let mut unit_groups = BTreeMap::new();
    for group in &ledger.groups {
        for unit_id in &group.translation_unit_ids {
            ensure!(
                unit_groups
                    .insert(unit_id.as_str(), group.id.as_str())
                    .is_none(),
                "graphic-text translation unit {unit_id} is owned by two surface groups"
            );
        }
    }
    Ok(unit_groups)
}

fn side_by_side_rgb(image: &GraphicSurfaceComparisonImage) -> Result<Vec<u8>> {
    let expected_size = image.width * image.height * 3;
    ensure!(
        image.source_rgb.len() == expected_size && image.localized_rgb.len() == expected_size,
        "graphic comparison {} has invalid RGB geometry",
        image.id
    );
    let pair_width = image.width * 2 + 16;
    let mut pair = vec![0_u8; pair_width * image.height * 3];
    for y in 0..image.height {
        let source_start = y * image.width * 3;
        let left_start = y * pair_width * 3;
        pair[left_start..left_start + image.width * 3]
            .copy_from_slice(&image.source_rgb[source_start..source_start + image.width * 3]);
        let separator_start = left_start + image.width * 3;
        pair[separator_start..separator_start + 16 * 3].fill(0x30);
        let right_start = separator_start + 16 * 3;
        pair[right_start..right_start + image.width * 3]
            .copy_from_slice(&image.localized_rgb[source_start..source_start + image.width * 3]);
    }
    Ok(pair)
}

fn encode_bmp(width: usize, height: usize, rgb: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        width > 0 && height > 0 && rgb.len() == width * height * 3,
        "invalid BMP geometry"
    );
    let row_size = (width * 3).div_ceil(4) * 4;
    let pixel_size = row_size
        .checked_mul(height)
        .context("BMP pixel size overflow")?;
    let file_size = 54usize
        .checked_add(pixel_size)
        .context("BMP file size overflow")?;
    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&u32::try_from(file_size)?.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54_u32.to_le_bytes());
    bmp.extend_from_slice(&40_u32.to_le_bytes());
    bmp.extend_from_slice(&i32::try_from(width)?.to_le_bytes());
    bmp.extend_from_slice(&i32::try_from(height)?.to_le_bytes());
    bmp.extend_from_slice(&1_u16.to_le_bytes());
    bmp.extend_from_slice(&24_u16.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&u32::try_from(pixel_size)?.to_le_bytes());
    bmp.extend_from_slice(&2835_i32.to_le_bytes());
    bmp.extend_from_slice(&2835_i32.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    let padding = row_size - width * 3;
    for y in (0..height).rev() {
        for x in 0..width {
            let start = (y * width + x) * 3;
            bmp.extend_from_slice(&[rgb[start + 2], rgb[start + 1], rgb[start]]);
        }
        bmp.extend(std::iter::repeat_n(0, padding));
    }
    ensure!(
        bmp.len() == file_size,
        "BMP encoder produced the wrong size"
    );
    Ok(bmp)
}

fn write_new_file(directory: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let path = directory.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("could not write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("could not sync {}", path.display()))?;
    Ok(())
}

fn render_review_html(
    ledger: &GraphicTextSurfaceLedger,
    report: &GraphicTextComparisonExportReport,
) -> String {
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>마도사오륙 그래픽 문구 원본 vs 한글화</title>\
<style>body{font-family:system-ui,sans-serif;background:#181818;color:#eee;margin:24px}h1,h2{margin-bottom:.4em}.note{color:#bbb;max-width:80em}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(420px,1fr));gap:20px}.card{background:#242424;border:1px solid #444;padding:14px}.labels{display:grid;grid-template-columns:1fr 1fr;text-align:center;font-weight:700;margin:8px 0}.card img{display:block;max-width:100%;height:auto;margin:auto;background:#000;image-rendering:pixelated}.meta{font-family:ui-monospace,monospace;color:#bbb;font-size:.85em}.gap{border-color:#b55}</style>\
<h1>마도사오륙 그래픽 문구: 원본 vs 한글화</h1>\
<p class=\"note\">왼쪽은 검증된 Disc Station Vol. 09 원본, 오른쪽은 현재 추적 번역과 동일한 그래픽 재조립 결과다. 실제 팔레트 RGB가 아닌 색인 구분용 진단색을 사용한다. 마스크 합성 표면은 글자 밖 배경 색인이 바뀌지 않았는지 별도로 검사하지만 실제 런타임 색·전환·미감 승인을 증명하지 않는다.</p>",
    );
    for group in &ledger.groups {
        html.push_str(&format!(
            "<h2>{}</h2><p class=\"note\"><b>소비 방식:</b> {}<br><b>JP:</b> {}<br><b>현재 KR:</b> {}<br><b>판정:</b> {}<br><b>권장 전략:</b> {}</p>",
            escape_html(&group.id),
            escape_html(&group.consumer_composition),
            escape_html(&group.source_presentation),
            escape_html(&group.localized_presentation),
            escape_html(&group.current_assessment),
            escape_html(&group.recommended_strategy),
        ));
        if group.tracking_status == "visible_unowned" {
            html.push_str("<div class=\"card gap\"><b>비교 이미지 미생성: 현재 번역 카탈로그 밖의 보이는 에셋</b></div>");
            continue;
        }
        if group.translation_unit_ids.is_empty() {
            html.push_str("<div class=\"card\"><b>원본 보존: 번역 범위에서 제외된 에셋</b></div>");
            continue;
        }
        html.push_str("<div class=\"grid\">");
        for pair in report.pairs.iter().filter(|pair| pair.group_id == group.id) {
            let palette_profile = format!(
                "ink palette JP {:?} / KR {:?} / protected background changes {:?}",
                pair.source_ink_palette_indices,
                pair.localized_ink_palette_indices,
                pair.protected_background_pixels_changed,
            );
            html.push_str(&format!(
                "<figure class=\"card\"><figcaption><b>{}</b></figcaption><div class=\"labels\"><span>원본</span><span>한글화</span></div><img src=\"{}\" width=\"{}\" height=\"{}\"><p class=\"meta\">{} / {} / {}<br>{}</p></figure>",
                escape_html(&pair.id),
                escape_html(&pair.pair_file),
                pair.width * 2 + 16,
                pair.height,
                escape_html(pair.source_asset),
                escape_html(pair.consumer_program),
                escape_html(pair.classification),
                escape_html(&palette_profile),
            ));
        }
        html.push_str("</div>");
    }
    html
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name.bytes().all(|byte| byte.is_ascii())
}

#[cfg(test)]
#[path = "graphic_comparison_tests.rs"]
mod tests;
