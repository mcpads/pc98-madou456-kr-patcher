//! User-supplied inputs that the public tree does not contain.
//!
//! Font files with their OFL texts, the graphic-text and `MADO456.COM` UI
//! translation assets and the graphic-text surface ledger are read at run time
//! from `MADOU456_ASSET_DIR`, or from `assets/` in the crate root when it is
//! unset. A missing input is an error, never an empty default.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};

const ASSET_DIRECTORY_VARIABLE: &str = "MADOU456_ASSET_DIR";

fn asset_directory() -> PathBuf {
    std::env::var_os(ASSET_DIRECTORY_VARIABLE)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"))
}

pub(crate) fn read_bytes(relative_path: &str) -> Result<&'static [u8]> {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, &'static [u8]>>> = OnceLock::new();
    let path = asset_directory().join(relative_path);
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(bytes) = cache.get(&path) {
        return Ok(bytes);
    }
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "required input {} is unavailable (set {ASSET_DIRECTORY_VARIABLE} or place it under assets/)",
            path.display()
        )
    })?;
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    cache.insert(path, bytes);
    Ok(bytes)
}

pub(crate) fn read_text(relative_path: &str) -> Result<&'static str> {
    std::str::from_utf8(read_bytes(relative_path)?)
        .with_context(|| format!("{relative_path} is not UTF-8 text"))
}
