use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct FixedRangeExpectedWrite {
    pub(crate) owner: &'static str,
    pub(crate) purpose: &'static str,
    pub(crate) offset: usize,
    pub(crate) expected_source: Vec<u8>,
    pub(crate) replacement: Vec<u8>,
}

pub(crate) fn apply_fixed_range_expected_writes(
    source: &[u8],
    writes: &[FixedRangeExpectedWrite],
) -> Result<Vec<u8>> {
    let mut ordered = writes.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|write| write.offset);

    let mut previous_end = 0usize;
    for (index, write) in ordered.iter().enumerate() {
        ensure!(
            write.expected_source.len() == write.replacement.len(),
            "{} expected write for {} changes fixed-range length from {} to {}",
            write.owner,
            write.purpose,
            write.expected_source.len(),
            write.replacement.len()
        );
        let end = write
            .offset
            .checked_add(write.expected_source.len())
            .with_context(|| {
                format!(
                    "{} expected write for {} overflows its source range",
                    write.owner, write.purpose
                )
            })?;
        let actual = source.get(write.offset..end).with_context(|| {
            format!(
                "{} expected write for {} lies outside the immutable source",
                write.owner, write.purpose
            )
        })?;
        ensure!(
            actual == write.expected_source,
            "{} expected write for {} does not match the immutable source",
            write.owner,
            write.purpose
        );
        if index > 0 {
            ensure!(
                previous_end <= write.offset,
                "expected writes overlap between immutable-source offsets 0x{:X} and 0x{:X}",
                write.offset,
                previous_end
            );
        }
        previous_end = end;
    }

    let mut output = source.to_vec();
    for write in ordered {
        let end = write.offset + write.replacement.len();
        output[write.offset..end].copy_from_slice(&write.replacement);
    }
    Ok(output)
}

#[cfg(test)]
#[path = "expected_write_tests.rs"]
mod tests;
