use std::collections::BTreeSet;
use std::ops::Range;

use anyhow::{Context, Result, bail, ensure};

pub(crate) const SYSTEM_VOLUME_OFFSET: usize = 71_680;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Fat16Layout {
    pub(crate) volume_offset: usize,
    pub(crate) bytes_per_sector: usize,
    pub(crate) sectors_per_cluster: usize,
    pub(crate) reserved_sectors: usize,
    pub(crate) fat_count: usize,
    pub(crate) root_entry_count: usize,
    pub(crate) total_sectors: usize,
    pub(crate) sectors_per_fat: usize,
    pub(crate) fat_offset: usize,
    pub(crate) root_offset: usize,
    pub(crate) data_offset: usize,
    pub(crate) cluster_size: usize,
    pub(crate) cluster_count: usize,
    pub(crate) volume_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectoryEntry {
    pub(crate) raw: [u8; 32],
    pub(crate) short_name: [u8; 11],
    pub(crate) attributes: u8,
    pub(crate) start_cluster: u16,
    pub(crate) size: usize,
}

impl DirectoryEntry {
    pub(crate) fn is_directory(&self) -> bool {
        self.attributes & 0x10 != 0
    }

    pub(crate) fn is_volume_label(&self) -> bool {
        self.attributes & 0x08 != 0
    }

    pub(crate) fn display_name(&self) -> String {
        short_name_text(&self.short_name)
    }
}

impl Fat16Layout {
    pub(crate) fn parse(image: &[u8]) -> Result<Self> {
        let boot = image
            .get(SYSTEM_VOLUME_OFFSET..SYSTEM_VOLUME_OFFSET + 64)
            .context("system HDI is too short to contain the expected FAT16 volume")?;
        let bytes_per_sector = u16::from_le_bytes(boot[11..13].try_into().unwrap()) as usize;
        let sectors_per_cluster = boot[13] as usize;
        let reserved_sectors = u16::from_le_bytes(boot[14..16].try_into().unwrap()) as usize;
        let fat_count = boot[16] as usize;
        let root_entry_count = u16::from_le_bytes(boot[17..19].try_into().unwrap()) as usize;
        let short_total_sectors = u16::from_le_bytes(boot[19..21].try_into().unwrap()) as usize;
        let sectors_per_fat = u16::from_le_bytes(boot[22..24].try_into().unwrap()) as usize;
        let long_total_sectors = u32::from_le_bytes(boot[32..36].try_into().unwrap()) as usize;
        let total_sectors = if short_total_sectors != 0 {
            short_total_sectors
        } else {
            long_total_sectors
        };

        ensure!(
            bytes_per_sector == 1_024,
            "unexpected FAT bytes per sector: {bytes_per_sector}"
        );
        ensure!(
            sectors_per_cluster == 2,
            "unexpected FAT sectors per cluster: {sectors_per_cluster}"
        );
        ensure!(
            reserved_sectors == 1,
            "unexpected FAT reserved sector count: {reserved_sectors}"
        );
        ensure!(fat_count == 2, "unexpected FAT mirror count: {fat_count}");
        ensure!(
            root_entry_count == 3_072,
            "unexpected FAT root entry count: {root_entry_count}"
        );
        ensure!(
            total_sectors == 39_600,
            "unexpected FAT total sector count: {total_sectors}"
        );
        ensure!(
            sectors_per_fat == 39,
            "unexpected FAT sectors per copy: {sectors_per_fat}"
        );
        ensure!(
            u16::from_le_bytes(boot[24..26].try_into().unwrap()) == 33,
            "unexpected FAT sectors per track"
        );
        ensure!(
            u16::from_le_bytes(boot[26..28].try_into().unwrap()) == 8,
            "unexpected FAT head count"
        );

        let fat_offset = SYSTEM_VOLUME_OFFSET + reserved_sectors * bytes_per_sector;
        let root_offset = fat_offset + fat_count * sectors_per_fat * bytes_per_sector;
        let root_bytes = root_entry_count * 32;
        let root_sectors = root_bytes.div_ceil(bytes_per_sector);
        let data_offset = root_offset + root_sectors * bytes_per_sector;
        let cluster_size = sectors_per_cluster * bytes_per_sector;
        let first_data_sector = reserved_sectors + fat_count * sectors_per_fat + root_sectors;
        let cluster_count = (total_sectors - first_data_sector) / sectors_per_cluster;
        ensure!(
            (4_085..65_525).contains(&cluster_count),
            "volume is not FAT16: {cluster_count} data clusters"
        );
        let volume_end = SYSTEM_VOLUME_OFFSET
            .checked_add(total_sectors * bytes_per_sector)
            .context("FAT volume range overflow")?;
        ensure!(
            volume_end <= image.len(),
            "FAT volume exceeds the system HDI"
        );

        let layout = Self {
            volume_offset: SYSTEM_VOLUME_OFFSET,
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            root_entry_count,
            total_sectors,
            sectors_per_fat,
            fat_offset,
            root_offset,
            data_offset,
            cluster_size,
            cluster_count,
            volume_end,
        };
        layout.verify_fat_mirrors(image)?;
        Ok(layout)
    }

    pub(crate) fn fat_bytes_per_copy(&self) -> usize {
        self.sectors_per_fat * self.bytes_per_sector
    }

    pub(crate) fn root_range(&self) -> Range<usize> {
        self.root_offset..self.root_offset + self.root_entry_count * 32
    }

    pub(crate) fn maximum_cluster(&self) -> u16 {
        u16::try_from(self.cluster_count + 1).expect("supported FAT16 cluster count fits u16")
    }

    pub(crate) fn cluster_range(&self, cluster: u16) -> Result<Range<usize>> {
        ensure!(
            (2..=self.maximum_cluster()).contains(&cluster),
            "FAT cluster {cluster} lies outside the data region"
        );
        let relative = usize::from(cluster - 2) * self.cluster_size;
        let start = self.data_offset + relative;
        Ok(start..start + self.cluster_size)
    }

    pub(crate) fn verify_fat_mirrors(&self, image: &[u8]) -> Result<()> {
        let bytes_per_copy = self.fat_bytes_per_copy();
        let first = image
            .get(self.fat_offset..self.fat_offset + bytes_per_copy)
            .context("first FAT copy exceeds the HDI")?;
        for copy_index in 1..self.fat_count {
            let start = self.fat_offset + copy_index * bytes_per_copy;
            let other = image
                .get(start..start + bytes_per_copy)
                .context("FAT mirror exceeds the HDI")?;
            ensure!(
                first == other,
                "FAT copy {copy_index} differs from the first copy"
            );
        }
        Ok(())
    }

    pub(crate) fn fat_entry(&self, image: &[u8], cluster: u16) -> Result<u16> {
        ensure!(
            cluster <= self.maximum_cluster(),
            "FAT cluster is outside the volume"
        );
        let offset = self.fat_offset + usize::from(cluster) * 2;
        let bytes = image
            .get(offset..offset + 2)
            .context("FAT entry exceeds the HDI")?;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub(crate) fn set_fat_entry(&self, image: &mut [u8], cluster: u16, value: u16) -> Result<()> {
        ensure!(
            cluster <= self.maximum_cluster(),
            "FAT cluster is outside the volume"
        );
        let encoded = value.to_le_bytes();
        for copy_index in 0..self.fat_count {
            let offset =
                self.fat_offset + copy_index * self.fat_bytes_per_copy() + usize::from(cluster) * 2;
            image
                .get_mut(offset..offset + 2)
                .context("FAT entry exceeds the HDI")?
                .copy_from_slice(&encoded);
        }
        Ok(())
    }

    pub(crate) fn cluster_chain(&self, image: &[u8], start_cluster: u16) -> Result<Vec<u16>> {
        if start_cluster == 0 {
            return Ok(Vec::new());
        }
        ensure!(
            start_cluster >= 2,
            "invalid FAT chain start {start_cluster}"
        );
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = start_cluster;
        loop {
            ensure!(
                current <= self.maximum_cluster(),
                "FAT chain points outside the data region at cluster {current}"
            );
            ensure!(seen.insert(current), "FAT chain loops at cluster {current}");
            chain.push(current);
            let next = self.fat_entry(image, current)?;
            match next {
                0 => bail!("allocated FAT chain becomes free after cluster {current}"),
                0xfff7 => bail!("allocated FAT chain reaches bad cluster marker after {current}"),
                0xfff8..=0xffff => break,
                0xfff0..=0xfff6 => {
                    bail!("allocated FAT chain reaches reserved marker 0x{next:04x}")
                }
                2.. => current = next,
                _ => bail!("allocated FAT chain reaches invalid cluster {next}"),
            }
        }
        Ok(chain)
    }

    pub(crate) fn read_file(&self, image: &[u8], entry: &DirectoryEntry) -> Result<Vec<u8>> {
        ensure!(
            !entry.is_directory(),
            "attempted to read a directory as a file"
        );
        if entry.size == 0 {
            ensure!(
                entry.start_cluster == 0,
                "zero-byte file has an allocated start cluster"
            );
            return Ok(Vec::new());
        }
        let chain = self.cluster_chain(image, entry.start_cluster)?;
        ensure!(
            chain.len() * self.cluster_size >= entry.size,
            "file {} exceeds its FAT chain capacity",
            entry.display_name()
        );
        let mut bytes = Vec::with_capacity(chain.len() * self.cluster_size);
        for cluster in chain {
            bytes.extend_from_slice(&image[self.cluster_range(cluster)?]);
        }
        bytes.truncate(entry.size);
        Ok(bytes)
    }

    pub(crate) fn root_entries(&self, image: &[u8]) -> Result<Vec<(usize, DirectoryEntry)>> {
        let bytes = image
            .get(self.root_range())
            .context("FAT root directory exceeds the HDI")?;
        parse_directory_slots(bytes)
    }

    pub(crate) fn directory_entries(
        &self,
        image: &[u8],
        start_cluster: u16,
    ) -> Result<Vec<DirectoryEntry>> {
        let chain = self.cluster_chain(image, start_cluster)?;
        let mut bytes = Vec::with_capacity(chain.len() * self.cluster_size);
        for cluster in chain {
            bytes.extend_from_slice(&image[self.cluster_range(cluster)?]);
        }
        Ok(parse_directory_slots(&bytes)?
            .into_iter()
            .map(|(_, entry)| entry)
            .collect())
    }
}

pub(crate) fn make_directory_entry(
    short_name: [u8; 11],
    attributes: u8,
    start_cluster: u16,
    size: usize,
) -> Result<[u8; 32]> {
    let mut raw = [0u8; 32];
    raw[..11].copy_from_slice(&short_name);
    raw[11] = attributes;
    raw[26..28].copy_from_slice(&start_cluster.to_le_bytes());
    raw[28..32].copy_from_slice(
        &u32::try_from(size)
            .context("file is too large for a FAT directory entry")?
            .to_le_bytes(),
    );
    Ok(raw)
}

pub(crate) fn short_name_text(short_name: &[u8; 11]) -> String {
    if short_name == b".          " {
        return ".".to_owned();
    }
    if short_name == b"..         " {
        return "..".to_owned();
    }
    let base = String::from_utf8_lossy(&short_name[..8])
        .trim_end()
        .to_owned();
    let extension = String::from_utf8_lossy(&short_name[8..])
        .trim_end()
        .to_owned();
    if extension.is_empty() {
        base
    } else {
        format!("{base}.{extension}")
    }
}

fn parse_directory_slots(bytes: &[u8]) -> Result<Vec<(usize, DirectoryEntry)>> {
    ensure!(
        bytes.len().is_multiple_of(32),
        "FAT directory byte length is not entry-aligned"
    );
    let mut entries = Vec::new();
    let (slots, remainder) = bytes.as_chunks::<32>();
    ensure!(
        remainder.is_empty(),
        "FAT directory has a partial trailing entry"
    );
    for (slot, raw_bytes) in slots.iter().enumerate() {
        if raw_bytes[0] == 0 {
            break;
        }
        if raw_bytes[0] == 0xe5 || raw_bytes[11] == 0x0f {
            continue;
        }
        let raw = *raw_bytes;
        let short_name = raw[..11].try_into().unwrap();
        let attributes = raw[11];
        let start_cluster = u16::from_le_bytes(raw[26..28].try_into().unwrap());
        let size = u32::from_le_bytes(raw[28..32].try_into().unwrap()) as usize;
        entries.push((
            slot,
            DirectoryEntry {
                raw,
                short_name,
                attributes,
                start_cluster,
                size,
            },
        ));
    }
    Ok(entries)
}

#[cfg(test)]
#[path = "fat16_tests.rs"]
mod tests;
