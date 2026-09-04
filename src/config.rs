//! Where things live on disk. XDG paths on every platform, because that is what the rest of
//! a CLI user's dotfiles look like, macOS included.

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};

/// `$NEOTYPE_DATA_DIR`, else `~/.local/share/neotype` (honouring `$XDG_DATA_HOME`).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("NEOTYPE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let strategy = choose_app_strategy(AppStrategyArgs {
        top_level_domain: "io".into(),
        author: "kilian".into(),
        app_name: "neotype".into(),
    })
    .context("no home directory")?;
    Ok(strategy.data_dir())
}

pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("neotype.db"))
}
