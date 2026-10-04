use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::fat16::{DirectoryEntry, Fat16Layout, make_directory_entry, short_name_text};
use crate::source_cd::{EXPECTED_VOLUME_ID, GameFile, encode_short_name, extract_game_files};

const SOURCE_CD_SIZE: usize = 46_901_232;
const SOURCE_CD_SHA256: &str = "a288832c3f1ff2ff457f457d2db450eb27d8bb24f902e1dd886322aeb097c7db";
const SYSTEM_HDI_SIZE: usize = 41_568_256;
const SYSTEM_HDI_SHA256: &str = "8808a0da11959721a588d07e6c9973a8707ee01f0849d8a3e9d5a1e7e7caef2c";

const RETAINED_ROOT_NAMES: [&str; 8] = [
    "IO.SYS",
    "MSDOS.SYS",
    "COMMAND.COM",
    "DBLSPACE.BIN",
    "DOS",
    "VEM486.EXE",
    "VEMEMM.SYS",
    "CONFIG.SYS",
];

const AUTOEXEC_BYTES: &[u8] = b"@ECHO OFF\r\n\
PATH A:\\DOS;A:\\MADOU456;A:\\\r\n\
SET TEMP=A:\\DOS\r\n\
SET DOSDIR=A:\\DOS\r\n\
SET QBGRPNEC=1\r\n\
CD \\MADOU456\r\n\
CALL 456.BAT\r\n\
CD \\\r\n\
\x1a";

#[derive(Debug, Serialize)]
pub struct SourceReport {
    source_cd_sha256: String,
    source_cd_size: usize,
    source_cd_volume_id: &'static str,
    game_file_count: usize,
    game_total_size: usize,
    system_hdi_sha256: String,
    system_hdi_size: usize,
    fat16_volume_offset: usize,
    fat16_volume_end: usize,
    retained_system_clusters: usize,
}

#[derive(Debug, Serialize)]
pub struct BuildReport {
    output: PathBuf,
    output_sha256: String,
    output_size: usize,
    game_file_count: usize,
    game_total_size: usize,
    replacement_file_count: usize,
    retained_system_clusters: usize,
    removed_non_system_clusters_zeroed: usize,
    fat_mirrors_identical: bool,
    bytes_outside_fat16_volume_preserved: bool,
}

struct VerifiedInputs {
    source_cd: Vec<u8>,
    system_hdi: Vec<u8>,
    game_files: Vec<GameFile>,
    layout: Fat16Layout,
    retained_root_entries: Vec<(usize, DirectoryEntry)>,
    volume_label_entries: Vec<(usize, DirectoryEntry)>,
    retained_clusters: BTreeSet<u16>,
    autoexec_slot: usize,
    game_directory_slot: usize,
}

pub(crate) fn load_verified_game_files(
    source_cd_path: &Path,
    system_hdi_path: &Path,
) -> Result<Vec<GameFile>> {
    Ok(load_verified_game_files_with_report(source_cd_path, system_hdi_path)?.1)
}

pub(crate) fn load_verified_game_files_with_report(
    source_cd_path: &Path,
    system_hdi_path: &Path,
) -> Result<(SourceReport, Vec<GameFile>)> {
    let inputs = load_verified_inputs(source_cd_path, system_hdi_path)?;
    let report = source_report(&inputs);
    Ok((report, inputs.game_files))
}

pub fn verify_sources(source_cd_path: &Path, system_hdi_path: &Path) -> Result<SourceReport> {
    let inputs = load_verified_inputs(source_cd_path, system_hdi_path)?;
    Ok(source_report(&inputs))
}

fn source_report(inputs: &VerifiedInputs) -> SourceReport {
    SourceReport {
        source_cd_sha256: SOURCE_CD_SHA256.to_owned(),
        source_cd_size: inputs.source_cd.len(),
        source_cd_volume_id: EXPECTED_VOLUME_ID,
        game_file_count: inputs.game_files.len(),
        game_total_size: inputs.game_files.iter().map(|file| file.bytes.len()).sum(),
        system_hdi_sha256: SYSTEM_HDI_SHA256.to_owned(),
        system_hdi_size: inputs.system_hdi.len(),
        fat16_volume_offset: inputs.layout.volume_offset,
        fat16_volume_end: inputs.layout.volume_end,
        retained_system_clusters: inputs.retained_clusters.len(),
    }
}

pub fn build_reassembled_hdi(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_path: &Path,
) -> Result<BuildReport> {
    build_reassembled_hdi_with_replacements(
        source_cd_path,
        system_hdi_path,
        output_path,
        &BTreeMap::new(),
    )
}

pub(crate) fn build_reassembled_hdi_with_replacements(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_path: &Path,
    replacements: &BTreeMap<String, Vec<u8>>,
) -> Result<BuildReport> {
    ensure!(
        !output_path.exists(),
        "refusing to overwrite existing output {}",
        output_path.display()
    );
    let inputs = load_verified_inputs(source_cd_path, system_hdi_path)?;
    let game_files = apply_game_file_replacements(&inputs.game_files, replacements)?;
    let retained_system_clusters = inputs.retained_clusters.len();
    let mut output = inputs.system_hdi.clone();

    let removed_non_system_clusters_zeroed = clear_non_system_allocation(&mut output, &inputs)?;
    prepare_root_directory(&mut output, &inputs)?;

    let autoexec_chain = allocate_bytes(&mut output, &inputs.layout, AUTOEXEC_BYTES)?;
    let autoexec_start = autoexec_chain.first().copied().unwrap_or(0);

    let directory_byte_size = (2 + game_files.len()) * 32;
    let directory_cluster_count = directory_byte_size.div_ceil(inputs.layout.cluster_size);
    let game_directory_chain =
        allocate_clusters(&mut output, &inputs.layout, directory_cluster_count)?;
    let game_directory_start = game_directory_chain[0];

    let mut inserted_files = Vec::with_capacity(game_files.len());
    for file in &game_files {
        let chain = allocate_bytes(&mut output, &inputs.layout, &file.bytes)?;
        inserted_files.push((file, chain.first().copied().unwrap_or(0)));
    }

    write_game_directory(
        &mut output,
        &inputs.layout,
        &game_directory_chain,
        &inserted_files,
    )?;
    write_root_entry(
        &mut output,
        &inputs.layout,
        inputs.autoexec_slot,
        make_directory_entry(
            encode_short_name("AUTOEXEC.BAT")?,
            0x20,
            autoexec_start,
            AUTOEXEC_BYTES.len(),
        )?,
    )?;
    write_root_entry(
        &mut output,
        &inputs.layout,
        inputs.game_directory_slot,
        make_directory_entry(
            encode_short_name("MADOU456")?,
            0x10,
            game_directory_start,
            0,
        )?,
    )?;

    verify_reassembled_output(&inputs, &game_files, &output)?;
    let output_sha256 = sha256_hex(&output);
    write_new_file(output_path, &output)?;

    Ok(BuildReport {
        output: output_path.to_path_buf(),
        output_sha256,
        output_size: output.len(),
        game_file_count: game_files.len(),
        game_total_size: game_files.iter().map(|file| file.bytes.len()).sum(),
        replacement_file_count: replacements.len(),
        retained_system_clusters,
        removed_non_system_clusters_zeroed,
        fat_mirrors_identical: true,
        bytes_outside_fat16_volume_preserved: true,
    })
}

fn apply_game_file_replacements(
    source_files: &[GameFile],
    replacements: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<GameFile>> {
    let source_names: BTreeSet<_> = source_files
        .iter()
        .map(|file| file.display_name.as_str())
        .collect();
    for name in replacements.keys() {
        ensure!(
            source_names.contains(name.as_str()),
            "localized replacement {name} is not a verified game file"
        );
    }

    Ok(source_files
        .iter()
        .map(|file| GameFile {
            display_name: file.display_name.clone(),
            short_name: file.short_name,
            bytes: replacements
                .get(&file.display_name)
                .cloned()
                .unwrap_or_else(|| file.bytes.clone()),
        })
        .collect())
}

fn load_verified_inputs(source_cd_path: &Path, system_hdi_path: &Path) -> Result<VerifiedInputs> {
    let source_cd = read_exact_source(
        source_cd_path,
        "Disc Station Vol. 09 raw CD image",
        SOURCE_CD_SIZE,
        SOURCE_CD_SHA256,
    )?;
    let system_hdi = read_exact_source(
        system_hdi_path,
        "installed Disc Station Vol. 09-10-11 system HDI",
        SYSTEM_HDI_SIZE,
        SYSTEM_HDI_SHA256,
    )?;
    let game_files = extract_game_files(&source_cd)?;
    let layout = Fat16Layout::parse(&system_hdi)?;
    let root_entries = layout.root_entries(&system_hdi)?;

    let mut retained_root_entries = Vec::new();
    let mut volume_label_entries = Vec::new();
    let mut retained_clusters = BTreeSet::new();
    for wanted_name in RETAINED_ROOT_NAMES {
        let (slot, entry) = find_unique_root_entry(&root_entries, wanted_name)?;
        claim_entry_tree(
            &system_hdi,
            &layout,
            entry,
            wanted_name,
            &mut retained_clusters,
        )?;
        retained_root_entries.push((*slot, entry.clone()));
    }
    for (slot, entry) in &root_entries {
        if entry.is_volume_label() {
            volume_label_entries.push((*slot, entry.clone()));
        }
    }

    let (autoexec_slot, autoexec) = find_unique_root_entry(&root_entries, "AUTOEXEC.BAT")?;
    ensure!(!autoexec.is_directory(), "donor AUTOEXEC.BAT is not a file");
    let (game_directory_slot, game_directory) = find_unique_root_entry(&root_entries, "MADOU456")?;
    ensure!(
        game_directory.is_directory(),
        "donor MADOU456 is not a directory"
    );

    Ok(VerifiedInputs {
        source_cd,
        system_hdi,
        game_files,
        layout,
        retained_root_entries,
        volume_label_entries,
        retained_clusters,
        autoexec_slot: *autoexec_slot,
        game_directory_slot: *game_directory_slot,
    })
}

fn read_exact_source(
    path: &Path,
    label: &str,
    expected_size: usize,
    expected_hash: &str,
) -> Result<Vec<u8>> {
    let bytes =
        fs::read(path).with_context(|| format!("could not read {label} at {}", path.display()))?;
    ensure!(
        bytes.len() == expected_size,
        "{label} size mismatch: expected {expected_size}, got {}",
        bytes.len()
    );
    let observed_hash = sha256_hex(&bytes);
    ensure!(
        observed_hash == expected_hash,
        "{label} SHA-256 mismatch: expected {expected_hash}, got {observed_hash}"
    );
    Ok(bytes)
}

fn find_unique_root_entry<'a>(
    entries: &'a [(usize, DirectoryEntry)],
    wanted_name: &str,
) -> Result<&'a (usize, DirectoryEntry)> {
    let encoded = encode_short_name(wanted_name)?;
    let mut matches = entries
        .iter()
        .filter(|(_, entry)| entry.short_name == encoded);
    let found = matches
        .next()
        .with_context(|| format!("system HDI root entry {wanted_name} is missing"))?;
    ensure!(
        matches.next().is_none(),
        "system HDI root entry {wanted_name} is duplicated"
    );
    Ok(found)
}

fn claim_entry_tree(
    image: &[u8],
    layout: &Fat16Layout,
    entry: &DirectoryEntry,
    path: &str,
    claimed: &mut BTreeSet<u16>,
) -> Result<()> {
    if entry.is_volume_label() {
        return Ok(());
    }
    if entry.display_name() == "." || entry.display_name() == ".." {
        return Ok(());
    }
    if entry.size == 0 && !entry.is_directory() {
        ensure!(
            entry.start_cluster == 0,
            "zero-byte file {path} has an allocated cluster"
        );
        return Ok(());
    }
    let chain = layout
        .cluster_chain(image, entry.start_cluster)
        .with_context(|| format!("invalid FAT chain for {path}"))?;
    if !entry.is_directory() {
        ensure!(
            chain.len() * layout.cluster_size >= entry.size,
            "file {path} exceeds its FAT chain capacity"
        );
    }
    for cluster in chain {
        ensure!(
            claimed.insert(cluster),
            "FAT cluster {cluster} is cross-linked at {path}"
        );
    }
    if entry.is_directory() {
        for child in layout.directory_entries(image, entry.start_cluster)? {
            let child_name = child.display_name();
            if child_name == "." || child_name == ".." || child.is_volume_label() {
                continue;
            }
            let child_path = format!("{path}/{child_name}");
            claim_entry_tree(image, layout, &child, &child_path, claimed)?;
        }
    }
    Ok(())
}

fn clear_non_system_allocation(output: &mut [u8], inputs: &VerifiedInputs) -> Result<usize> {
    let mut zeroed = 0usize;
    for cluster in 2..=inputs.layout.maximum_cluster() {
        if inputs.retained_clusters.contains(&cluster) {
            continue;
        }
        output[inputs.layout.cluster_range(cluster)?].fill(0);
        inputs.layout.set_fat_entry(output, cluster, 0)?;
        zeroed += 1;
    }
    Ok(zeroed)
}

fn prepare_root_directory(output: &mut [u8], inputs: &VerifiedInputs) -> Result<()> {
    output[inputs.layout.root_range()].fill(0);
    let final_active_slot = inputs
        .retained_root_entries
        .iter()
        .chain(inputs.volume_label_entries.iter())
        .map(|(slot, _)| *slot)
        .chain([inputs.autoexec_slot, inputs.game_directory_slot])
        .max()
        .context("no root entries selected for output")?;
    for slot in 0..final_active_slot {
        output[inputs.layout.root_offset + slot * 32] = 0xe5;
    }
    for (slot, entry) in inputs
        .retained_root_entries
        .iter()
        .chain(inputs.volume_label_entries.iter())
    {
        write_root_entry(output, &inputs.layout, *slot, entry.raw)?;
    }
    Ok(())
}

fn allocate_bytes(output: &mut [u8], layout: &Fat16Layout, bytes: &[u8]) -> Result<Vec<u16>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let cluster_count = bytes.len().div_ceil(layout.cluster_size);
    let chain = allocate_clusters(output, layout, cluster_count)?;
    write_chain_bytes(output, layout, &chain, bytes)?;
    Ok(chain)
}

fn allocate_clusters(
    output: &mut [u8],
    layout: &Fat16Layout,
    cluster_count: usize,
) -> Result<Vec<u16>> {
    ensure!(cluster_count > 0, "cannot allocate an empty FAT chain");
    let mut chain = Vec::with_capacity(cluster_count);
    for cluster in 2..=layout.maximum_cluster() {
        if layout.fat_entry(output, cluster)? == 0 {
            chain.push(cluster);
            if chain.len() == cluster_count {
                break;
            }
        }
    }
    ensure!(
        chain.len() == cluster_count,
        "FAT16 volume has insufficient free clusters: needed {cluster_count}, found {}",
        chain.len()
    );
    for (index, cluster) in chain.iter().copied().enumerate() {
        let next = chain.get(index + 1).copied().unwrap_or(0xffff);
        layout.set_fat_entry(output, cluster, next)?;
    }
    Ok(chain)
}

fn write_chain_bytes(
    output: &mut [u8],
    layout: &Fat16Layout,
    chain: &[u16],
    bytes: &[u8],
) -> Result<()> {
    ensure!(
        bytes.len() <= chain.len() * layout.cluster_size,
        "payload exceeds allocated FAT chain"
    );
    for cluster in chain {
        output[layout.cluster_range(*cluster)?].fill(0);
    }
    for (chunk, cluster) in bytes.chunks(layout.cluster_size).zip(chain.iter().copied()) {
        let range = layout.cluster_range(cluster)?;
        output[range.start..range.start + chunk.len()].copy_from_slice(chunk);
    }
    Ok(())
}

fn write_game_directory(
    output: &mut [u8],
    layout: &Fat16Layout,
    directory_chain: &[u16],
    files: &[(&GameFile, u16)],
) -> Result<()> {
    let mut bytes = vec![0u8; directory_chain.len() * layout.cluster_size];
    let dot = make_directory_entry(*b".          ", 0x10, directory_chain[0], 0)?;
    let dot_dot = make_directory_entry(*b"..         ", 0x10, 0, 0)?;
    bytes[..32].copy_from_slice(&dot);
    bytes[32..64].copy_from_slice(&dot_dot);
    for (index, (file, start_cluster)) in files.iter().enumerate() {
        let entry = make_directory_entry(file.short_name, 0x20, *start_cluster, file.bytes.len())?;
        let start = (index + 2) * 32;
        bytes[start..start + 32].copy_from_slice(&entry);
    }
    write_chain_bytes(output, layout, directory_chain, &bytes)
}

fn write_root_entry(
    output: &mut [u8],
    layout: &Fat16Layout,
    slot: usize,
    raw: [u8; 32],
) -> Result<()> {
    ensure!(
        slot < layout.root_entry_count,
        "FAT root directory slot is out of range"
    );
    let start = layout.root_offset + slot * 32;
    output[start..start + 32].copy_from_slice(&raw);
    Ok(())
}

fn verify_reassembled_output(
    inputs: &VerifiedInputs,
    expected_game_files: &[GameFile],
    output: &[u8],
) -> Result<()> {
    ensure!(
        output.len() == inputs.system_hdi.len(),
        "output HDI size changed"
    );
    ensure!(
        output[..inputs.layout.fat_offset] == inputs.system_hdi[..inputs.layout.fat_offset],
        "output changed the HDI header, IPL, partition gap, or FAT boot sector"
    );
    ensure!(
        output[inputs.layout.volume_end..] == inputs.system_hdi[inputs.layout.volume_end..],
        "output changed bytes after the supported FAT16 volume"
    );
    inputs.layout.verify_fat_mirrors(output)?;

    for (slot, entry) in &inputs.retained_root_entries {
        let start = inputs.layout.root_offset + slot * 32;
        ensure!(
            output[start..start + 32] == entry.raw,
            "retained root entry {} changed",
            entry.display_name()
        );
    }
    for cluster in &inputs.retained_clusters {
        let range = inputs.layout.cluster_range(*cluster)?;
        ensure!(
            output[range.clone()] == inputs.system_hdi[range.clone()],
            "retained system cluster {cluster} changed"
        );
        ensure!(
            inputs.layout.fat_entry(output, *cluster)?
                == inputs.layout.fat_entry(&inputs.system_hdi, *cluster)?,
            "retained system FAT chain changed at cluster {cluster}"
        );
    }

    let root_entries = inputs.layout.root_entries(output)?;
    let visible_names: BTreeSet<_> = root_entries
        .iter()
        .filter(|(_, entry)| !entry.is_volume_label())
        .map(|(_, entry)| entry.display_name())
        .collect();
    let expected_names: BTreeSet<_> = RETAINED_ROOT_NAMES
        .into_iter()
        .chain(["AUTOEXEC.BAT", "MADOU456"])
        .map(str::to_owned)
        .collect();
    ensure!(
        visible_names == expected_names,
        "output root file set differs: expected {expected_names:?}, got {visible_names:?}"
    );

    let (_, autoexec) = find_unique_root_entry(&root_entries, "AUTOEXEC.BAT")?;
    ensure!(
        inputs.layout.read_file(output, autoexec)? == AUTOEXEC_BYTES,
        "output AUTOEXEC.BAT differs from the standalone launch script"
    );
    let (_, game_directory) = find_unique_root_entry(&root_entries, "MADOU456")?;
    ensure!(
        game_directory.is_directory(),
        "output MADOU456 is not a directory"
    );
    verify_game_directory(inputs, expected_game_files, output, game_directory)?;

    let mut referenced_clusters = BTreeSet::new();
    for (_, entry) in &root_entries {
        claim_entry_tree(
            output,
            &inputs.layout,
            entry,
            &entry.display_name(),
            &mut referenced_clusters,
        )?;
    }
    for cluster in 2..=inputs.layout.maximum_cluster() {
        let value = inputs.layout.fat_entry(output, cluster)?;
        if referenced_clusters.contains(&cluster) {
            ensure!(value != 0, "referenced cluster {cluster} is marked free");
        } else {
            ensure!(
                value == 0,
                "unreferenced cluster {cluster} remains allocated"
            );
            ensure!(
                output[inputs.layout.cluster_range(cluster)?]
                    .iter()
                    .all(|byte| *byte == 0),
                "free cluster {cluster} still contains donor data"
            );
        }
    }
    Ok(())
}

fn verify_game_directory(
    inputs: &VerifiedInputs,
    expected_game_files: &[GameFile],
    output: &[u8],
    directory: &DirectoryEntry,
) -> Result<()> {
    let mut observed = BTreeMap::new();
    for entry in inputs
        .layout
        .directory_entries(output, directory.start_cluster)?
    {
        let name = entry.display_name();
        if name == "." || name == ".." {
            continue;
        }
        ensure!(
            !entry.is_directory(),
            "output MADOU456 contains subdirectory {name}"
        );
        ensure!(
            observed
                .insert(entry.short_name, inputs.layout.read_file(output, &entry)?)
                .is_none(),
            "output MADOU456 duplicates {}",
            short_name_text(&entry.short_name)
        );
    }
    let expected: BTreeMap<_, _> = expected_game_files
        .iter()
        .map(|file| (file.short_name, file.bytes.clone()))
        .collect();
    ensure!(
        observed == expected,
        "output MADOU456 payload differs from the verified CD"
    );
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("could not create output directory {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("could not create temporary output in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .context("could not write temporary HDI output")?;
    temporary
        .as_file()
        .sync_all()
        .context("could not sync temporary HDI output")?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "could not create output {} without overwriting",
                path.display()
            )
        })?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "reassembly_tests.rs"]
mod tests;
