use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_DECODED_STREAM_BYTES: usize = 16 * 1024 * 1024;
const MAX_LITERAL_BYTES: usize = 0x7f;
const MAX_COPY_BYTES: usize = 0x82;
const MAX_COPY_DISTANCE: usize = 0x100;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct CompileLzStreamReport {
    pub(crate) input_offset: usize,
    pub(crate) packed_size: usize,
    pub(crate) decoded_size: usize,
    pub(crate) decoded_sha256: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct ExactCompileLzReport {
    pub(crate) streams: Vec<CompileLzStreamReport>,
    pub(crate) decoded_size: usize,
}

pub(crate) struct DecodedCompileLz {
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    pub(crate) report: ExactCompileLzReport,
    pub(crate) streams: Vec<Vec<u8>>,
}

/// Decode only when every input byte belongs to a nonempty Compile-LZ stream.
///
/// The command grammar is revalidated against the target's 166 exact streams:
/// `0x01..=0x7f` literals, `0x80..=0xff` back-references, and `0x00`
/// termination. A back-reference before the output start yields zero bytes,
/// matching the DORI-BIOS consumer used by the game.
pub(crate) fn decode_exact_compile_lz(input: &[u8]) -> Option<DecodedCompileLz> {
    let mut input_offset = 0usize;
    let mut streams = Vec::new();
    let mut reports = Vec::new();

    while input_offset < input.len() {
        let decoded = decode_stream(&input[input_offset..]).ok()?;
        if decoded.bytes.is_empty() {
            return None;
        }
        reports.push(CompileLzStreamReport {
            input_offset,
            packed_size: decoded.bytes_consumed,
            decoded_size: decoded.bytes.len(),
            decoded_sha256: sha256_hex(&decoded.bytes),
        });
        input_offset = input_offset.checked_add(decoded.bytes_consumed)?;
        streams.push(decoded.bytes);
    }

    if input_offset != input.len() || streams.is_empty() {
        return None;
    }
    let decoded_size = streams.iter().map(Vec::len).sum();
    Some(DecodedCompileLz {
        report: ExactCompileLzReport {
            streams: reports,
            decoded_size,
        },
        streams,
    })
}

/// Encode one Compile-LZ stream using the target command grammar.
///
/// This encoder guarantees decoder-compatible output. It does not claim that
/// its greedy match selection is the original program's canonical packing.
pub(crate) fn encode_compile_lz(bytes: &[u8]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(bytes.len());
    let mut literals = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        let (copy_bytes, distance) = longest_previous_match(bytes, cursor);
        if copy_bytes >= 3 {
            append_literals(&mut packed, &mut literals);
            packed.push(0x80 | u8::try_from(copy_bytes - 3).expect("copy length fits u8"));
            packed.push(u8::try_from(distance - 1).expect("copy distance fits u8"));
            cursor += copy_bytes;
        } else {
            literals.push(bytes[cursor]);
            cursor += 1;
            if literals.len() == MAX_LITERAL_BYTES {
                append_literals(&mut packed, &mut literals);
            }
        }
    }
    append_literals(&mut packed, &mut literals);
    packed.push(0);
    packed
}

fn longest_previous_match(bytes: &[u8], cursor: usize) -> (usize, usize) {
    let maximum_bytes = MAX_COPY_BYTES.min(bytes.len() - cursor);
    let maximum_distance = MAX_COPY_DISTANCE.min(cursor);
    let mut best = (0usize, 0usize);
    for distance in 1..=maximum_distance {
        let mut matched = 0usize;
        while matched < maximum_bytes
            && bytes[cursor + matched] == bytes[cursor - distance + matched]
        {
            matched += 1;
        }
        if matched >= best.0 {
            best = (matched, distance);
        }
    }
    best
}

fn append_literals(packed: &mut Vec<u8>, literals: &mut Vec<u8>) {
    if literals.is_empty() {
        return;
    }
    packed.push(u8::try_from(literals.len()).expect("literal length fits u8"));
    packed.append(literals);
}

struct DecodedStream {
    bytes: Vec<u8>,
    bytes_consumed: usize,
}

fn decode_stream(input: &[u8]) -> Result<DecodedStream> {
    let mut bytes = Vec::new();
    let mut cursor = 0usize;

    loop {
        let command = *input
            .get(cursor)
            .context("Compile-LZ stream has no terminator")?;
        cursor += 1;
        if command == 0 {
            return Ok(DecodedStream {
                bytes,
                bytes_consumed: cursor,
            });
        }

        if command < 0x80 {
            let literal_bytes = usize::from(command);
            let end = cursor
                .checked_add(literal_bytes)
                .context("Compile-LZ literal offset overflow")?;
            let literal = input
                .get(cursor..end)
                .context("Compile-LZ literal exceeds the packed stream")?;
            ensure_decoded_capacity(bytes.len(), literal.len())?;
            bytes.extend_from_slice(literal);
            cursor = end;
            continue;
        }

        let distance = usize::from(
            *input
                .get(cursor)
                .context("Compile-LZ back-reference lacks a distance")?,
        ) + 1;
        cursor += 1;
        let copy_bytes = usize::from(command & 0x7f) + 3;
        ensure_decoded_capacity(bytes.len(), copy_bytes)?;
        let source_start = bytes.len() as isize - distance as isize;
        for index in 0..copy_bytes {
            let source = source_start + index as isize;
            let byte = if source < 0 {
                0
            } else {
                bytes[usize::try_from(source).expect("non-negative source fits usize")]
            };
            bytes.push(byte);
        }
    }
}

fn ensure_decoded_capacity(current: usize, additional: usize) -> Result<()> {
    let total = current
        .checked_add(additional)
        .context("Compile-LZ decoded size overflow")?;
    if total > MAX_DECODED_STREAM_BYTES {
        bail!("Compile-LZ stream exceeds the decoded-size safety limit");
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "compile_lz_tests.rs"]
mod tests;
