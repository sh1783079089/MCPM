use serde::{Deserialize, Serialize};

use crate::LoaderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    #[default]
    Hard,
    Soft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModConstraint {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConflictRule {
    pub mods: Vec<ModConstraint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderKind>,
    #[serde(default)]
    pub severity: RuleSeverity,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workaround {
    pub mods: Vec<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<WorkaroundAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkaroundAction {
    ConfigChange {
        file: String,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<toml::Value>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatibilityConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<LocalConflictRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workarounds: Vec<Workaround>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compatibility_toml() {
        let toml_str = r#"
[[conflicts]]
minecraft = "1.20.1"
loader = "fabric"
severity = "hard"
reason = "Crashes on startup"
mods = [
  { id = "mod_a", version = ">=1.4 <1.5" },
  { id = "mod_b", version = ">=3.2 <3.3" },
]

[[workarounds]]
mods = ["mod_a", "mod_b"]
reason = "Disable feature X"

[[workarounds.actions]]
type = "config_change"
file = "config/mod_a.toml"
key = "features.x"
value = false
"#;
        let config: CompatibilityConfig = toml::from_str(toml_str).expect("parse");
        assert_eq!(config.conflicts.len(), 1);
        assert_eq!(config.conflicts[0].severity, RuleSeverity::Hard);
        assert_eq!(config.conflicts[0].mods.len(), 2);
        assert_eq!(config.workarounds.len(), 1);
        assert_eq!(config.workarounds[0].actions.len(), 1);
    }
}
