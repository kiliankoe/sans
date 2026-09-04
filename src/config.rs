//! Where things live on disk, and the few settings there are. XDG paths on every platform,
//! because that is what the rest of a CLI user's dotfiles look like, macOS included.

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use serde::Deserialize;

use crate::text::file::Indent;

fn strategy() -> Result<impl AppStrategy> {
    choose_app_strategy(AppStrategyArgs {
        top_level_domain: "io".into(),
        author: "kilian".into(),
        app_name: "neotype".into(),
    })
    .context("no home directory")
}

/// `$NEOTYPE_DATA_DIR`, else `~/.local/share/neotype` (honouring `$XDG_DATA_HOME`).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("NEOTYPE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(strategy()?.data_dir())
}

pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("neotype.db"))
}

/// `~/.config/neotype/config.toml` (honouring `$XDG_CONFIG_HOME`).
pub fn config_path() -> Result<PathBuf> {
    Ok(strategy()?.config_dir().join("config.toml"))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Practice target per day, shown against today's minutes.
    pub daily_minutes: u32,
    /// Whether leading whitespace in files is typed or inserted for you.
    pub indent: IndentSetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndentSetting {
    Skip,
    Type,
}

impl From<IndentSetting> for Indent {
    fn from(setting: IndentSetting) -> Self {
        match setting {
            IndentSetting::Skip => Indent::Skip,
            IndentSetting::Type => Indent::Type,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            daily_minutes: 15,
            indent: IndentSetting::Skip,
        }
    }
}

impl Config {
    /// The config file if there is one, defaults otherwise.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_values_fall_back_to_defaults_and_unknown_keys_are_errors() {
        assert_eq!(Config::parse("").unwrap().daily_minutes, 15);
        assert_eq!(
            Config::parse("daily_minutes = 20\n").unwrap().daily_minutes,
            20
        );
        assert!(Config::parse("daily_minute = 20\n").is_err());
        assert_eq!(
            Config::parse("indent = \"type\"\n").unwrap().indent,
            IndentSetting::Type
        );
        assert!(Config::parse("indent = \"auto\"\n").is_err());
    }
}
