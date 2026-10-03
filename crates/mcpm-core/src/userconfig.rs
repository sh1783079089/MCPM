use std::path::PathBuf;

use serde::Deserialize;

/// Global, user-level configuration (not part of a modpack).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserConfig {
    #[serde(default)]
    pub curseforge_api_key: Option<String>,
}

pub const CURSEFORGE_API_KEY_ENV: &str = "CURSEFORGE_API_KEY";

pub fn config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("mcpm").join("config.toml"))
}

pub fn load() -> UserConfig {
    let Some(path) = config_file() else {
        return UserConfig::default();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return UserConfig::default();
    };
    toml::from_str(&content).unwrap_or_default()
}

/// Resolves the CurseForge API key, preferring the environment variable
/// over the persisted user configuration.
pub fn curseforge_api_key() -> Option<String> {
    if let Ok(key) = std::env::var(CURSEFORGE_API_KEY_ENV) {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    load()
        .curseforge_api_key
        .filter(|key| !key.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_config_parse() {
        let config: UserConfig =
            toml::from_str("curseforge_api_key = \"secret\"\n").expect("parse");
        assert_eq!(config.curseforge_api_key.as_deref(), Some("secret"));
    }

    #[test]
    fn test_user_config_default() {
        let config: UserConfig = toml::from_str("").expect("parse");
        assert!(config.curseforge_api_key.is_none());
    }
}
