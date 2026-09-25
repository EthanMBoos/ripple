//! Claim a fresh output directory without overwriting another run.

use anyhow::{Context, Result};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) fn create_directory(explicit: Option<&Path>, runs: &Path) -> Result<PathBuf> {
    if let Some(output) = explicit {
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("create output parent {}", parent.display()))?;
        }
        fs::create_dir(output).with_context(|| {
            format!(
                "create new run {}; output must not already exist",
                output.display()
            )
        })?;
        return Ok(output.to_path_buf());
    }

    fs::create_dir_all(runs)
        .with_context(|| format!("create runs directory {}", runs.display()))?;
    let mut next = 0_u64;
    for entry in fs::read_dir(runs)? {
        let name = entry?.file_name();
        let Some(number) = name
            .to_str()
            .and_then(|name| name.strip_prefix("run"))
            .and_then(|number| number.parse::<u64>().ok())
        else {
            continue;
        };
        next = next.max(number.checked_add(1).context("run numbers exhausted")?);
    }

    loop {
        let output = runs.join(format!("run{next:03}"));
        // Creation is atomic: another process may have claimed our candidate
        // since we scanned the directory. Keep its run and try the next number.
        match fs::create_dir(&output) {
            Ok(()) => return Ok(output),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                next = next.checked_add(1).context("run numbers exhausted")?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("create new run {}", output.display()));
            }
        }
    }
}

#[cfg(test)]
mod tests;
