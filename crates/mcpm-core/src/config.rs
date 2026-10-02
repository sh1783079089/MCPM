use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{LoaderKind, Side, SourceKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackConfig {
    pub pack: PackMeta,
    pub environment: Environment,
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    pub mods: indexmap::IndexMap<String, ModEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<crate::compat::CompatibilityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMeta {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub minecraft: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_range: Option<String>,
    pub loader: LoaderSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_candidates: Option<Vec<LoaderKind>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub side: Side,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoaderSpec {
    Fixed(LoaderKind),
    Candidates(Vec<LoaderKind>),
}

impl LoaderSpec {
    pub fn as_fixed(&self) -> Option<LoaderKind> {
        match self {
            LoaderSpec::Fixed(loader) => Some(*loader),
            LoaderSpec::Candidates(loaders) if loaders.len() == 1 => Some(loaders[0]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_release_type")]
    pub release_type: crate::ReleaseType,
    #[serde(default)]
    pub allow_beta: bool,
    #[serde(default = "default_source")]
    pub source: Vec<SourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_strategy: Option<EnvironmentStrategy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStrategy {
    #[default]
    MostCompatible,
    Newest,
}

fn default_release_type() -> crate::ReleaseType {
    crate::ReleaseType::Release
}

fn default_source() -> Vec<SourceKind> {
    vec![SourceKind::Modrinth]
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            release_type: default_release_type(),
            allow_beta: false,
            source: default_source(),
            environment_strategy: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModEntry {
    Simple(String),
    Detailed(DetailedModEntry),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedModEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl ModEntry {
    pub fn version(&self) -> &str {
        match self {
            ModEntry::Simple(v) => v,
            ModEntry::Detailed(d) => d.version.as_deref().unwrap_or("latest"),
        }
    }

    pub fn project_id(&self) -> Option<&str> {
        match self {
            ModEntry::Simple(_) => None,
            ModEntry::Detailed(d) => d.project_id.as_deref(),
        }
    }

    pub fn source(&self) -> Option<SourceKind> {
        match self {
            ModEntry::Simple(_) => None,
            ModEntry::Detailed(d) => d.source,
        }
    }

    pub fn side(&self) -> Option<Side> {
        match self {
            ModEntry::Simple(_) => None,
            ModEntry::Detailed(d) => d.side,
        }
    }
}

impl fmt::Display for LoaderSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoaderSpec::Fixed(loader) => write!(f, "{}", loader),
            LoaderSpec::Candidates(loaders) => {
                let strs: Vec<String> = loaders.iter().map(|l| l.to_string()).collect();
                write!(f, "[{}]", strs.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let config = PackConfig {
            pack: PackMeta {
                name: "test-pack".to_string(),
                author: Some("author".to_string()),
                description: None,
            },
            environment: Environment {
                minecraft: "1.20.1".to_string(),
                minecraft_range: None,
                loader: LoaderSpec::Fixed(LoaderKind::Fabric),
                loader_candidates: None,
                loader_version: None,
                side: Side::Both,
            },
            preferences: Preferences::default(),
            mods: {
                let mut m = indexmap::IndexMap::new();
                m.insert(
                    "sodium".to_string(),
                    ModEntry::Detailed(DetailedModEntry {
                        version: Some("latest".to_string()),
                        source: Some(SourceKind::Modrinth),
                        side: None,
                        project_id: Some("AANobbMI".to_string()),
                    }),
                );
                m.insert(
                    "lithium".to_string(),
                    ModEntry::Simple("latest".to_string()),
                );
                m
            },
            compatibility: None,
        };

        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: PackConfig = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(parsed.pack.name, "test-pack");
        assert_eq!(parsed.environment.minecraft, "1.20.1");
        assert!(matches!(
            parsed.environment.loader,
            LoaderSpec::Fixed(LoaderKind::Fabric)
        ));
        assert_eq!(parsed.mods.len(), 2);
        assert!(parsed.mods.contains_key("sodium"));
        assert!(parsed.mods.contains_key("lithium"));
    }

    #[test]
    fn test_simple_mod_entry() {
        let entry = ModEntry::Simple("latest".to_string());
        assert_eq!(entry.version(), "latest");
        assert!(entry.project_id().is_none());
    }

    #[test]
    fn test_detailed_mod_entry() {
        let entry = ModEntry::Detailed(DetailedModEntry {
            version: Some("1.0.0".to_string()),
            source: Some(SourceKind::Modrinth),
            side: Some(Side::Client),
            project_id: Some("abc123".to_string()),
        });
        assert_eq!(entry.version(), "1.0.0");
        assert_eq!(entry.project_id(), Some("abc123"));
        assert_eq!(entry.source(), Some(SourceKind::Modrinth));
        assert_eq!(entry.side(), Some(Side::Client));
    }

    #[test]
    fn test_locked_environment_roundtrip() {
        let config = PackConfig {
            pack: PackMeta {
                name: "pack".to_string(),
                author: None,
                description: None,
            },
            environment: Environment {
                minecraft: "1.20.1".to_string(),
                minecraft_range: Some(">=1.20 <1.22".to_string()),
                loader: LoaderSpec::Fixed(LoaderKind::Forge),
                loader_candidates: Some(vec![LoaderKind::Fabric, LoaderKind::Forge]),
                loader_version: Some("47.2.20".to_string()),
                side: Side::Client,
            },
            preferences: Preferences::default(),
            mods: indexmap::IndexMap::new(),
            compatibility: None,
        };

        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        assert!(toml_str.contains("loader = \"forge\""));
        assert!(toml_str.contains("minecraft_range = \">=1.20 <1.22\""));

        let parsed: PackConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(
            parsed.environment.minecraft_range.as_deref(),
            Some(">=1.20 <1.22")
        );
        assert_eq!(
            parsed.environment.loader_version.as_deref(),
            Some("47.2.20")
        );
        assert_eq!(
            parsed.environment.loader_candidates,
            Some(vec![LoaderKind::Fabric, LoaderKind::Forge])
        );
    }

    #[test]
    fn test_explore_environment_roundtrip() {
        let toml_str = r#"
[pack]
name = "explore-pack"

[environment]
minecraft = ">=1.20 <1.22"
loader = ["fabric", "forge"]
side = "client"

[mods]
sodium = "latest"
"#;
        let parsed: PackConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(matches!(
            parsed.environment.loader,
            LoaderSpec::Candidates(_)
        ));
        assert_eq!(parsed.environment.loader.as_fixed(), None);
    }

    #[test]
    fn test_loader_spec_display() {
        assert_eq!(LoaderSpec::Fixed(LoaderKind::Fabric).to_string(), "fabric");
        assert_eq!(
            LoaderSpec::Candidates(vec![LoaderKind::Fabric, LoaderKind::Forge]).to_string(),
            "[fabric, forge]"
        );
    }

    #[test]
    fn test_loader_spec_fixed() {
        assert_eq!(
            LoaderSpec::Fixed(LoaderKind::Fabric).as_fixed(),
            Some(LoaderKind::Fabric)
        );
        assert_eq!(
            LoaderSpec::Candidates(vec![LoaderKind::Fabric]).as_fixed(),
            Some(LoaderKind::Fabric)
        );
        assert_eq!(
            LoaderSpec::Candidates(vec![LoaderKind::Fabric, LoaderKind::Forge]).as_fixed(),
            None
        );
    }
}
