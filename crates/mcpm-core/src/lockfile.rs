use crate::{LoaderKind, ReleaseType, Side, SourceKind};
use serde::{Deserialize, Serialize};

pub const LOCKFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    pub lockfile_version: u32,
    pub mcpm_version: String,
    pub environment: LockEnvironment,
    pub mods: Vec<LockedMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockEnvironment {
    pub minecraft: String,
    pub loader: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub side: Side,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedMod {
    pub name: String,
    pub source: SourceKind,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub version_id: String,
    pub version_number: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_type: Option<ReleaseType>,
    pub filename: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub side: Side,
    pub reason: LockReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockReason {
    Requested,
    Dependency,
}

impl Lockfile {
    pub fn new(mcpm_version: &str, environment: LockEnvironment) -> Self {
        Self {
            lockfile_version: LOCKFILE_VERSION,
            mcpm_version: mcpm_version.to_string(),
            environment,
            mods: Vec::new(),
        }
    }

    pub fn find_mod_by_project_id(&self, project_id: &str) -> Option<&LockedMod> {
        self.mods.iter().find(|m| m.project_id == project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lockfile_roundtrip() {
        let lock_env = LockEnvironment {
            minecraft: "1.20.1".to_string(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.15.11".to_string()),
            side: Side::Both,
        };
        let mut lockfile = Lockfile::new("0.1.0", lock_env);
        lockfile.mods.push(LockedMod {
            name: "Sodium".to_string(),
            source: SourceKind::Modrinth,
            project_id: "AANobbMI".to_string(),
            slug: Some("sodium".to_string()),
            version_id: "abc123".to_string(),
            version_number: "0.5.8".to_string(),
            version_type: Some(ReleaseType::Release),
            filename: "sodium-fabric-0.5.8.jar".to_string(),
            url: "https://cdn.modrinth.com/sodium.jar".to_string(),
            sha512: Some("deadbeef".to_string()),
            sha1: None,
            size: Some(12345),
            side: Side::Both,
            reason: LockReason::Requested,
            dependencies: vec![],
        });

        let json = serde_json::to_string_pretty(&lockfile).expect("serialize");
        let parsed: Lockfile = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.lockfile_version, 1);
        assert_eq!(parsed.mcpm_version, "0.1.0");
        assert_eq!(parsed.environment.minecraft, "1.20.1");
        assert_eq!(parsed.environment.loader, LoaderKind::Fabric);
        assert_eq!(parsed.mods.len(), 1);
        assert_eq!(parsed.mods[0].name, "Sodium");
        assert_eq!(parsed.mods[0].reason, LockReason::Requested);
    }

    #[test]
    fn test_find_mod_by_project_id() {
        let lock_env = LockEnvironment {
            minecraft: "1.20.1".to_string(),
            loader: LoaderKind::Fabric,
            loader_version: None,
            side: Side::Both,
        };
        let mut lockfile = Lockfile::new("0.1.0", lock_env);
        lockfile.mods.push(LockedMod {
            name: "Sodium".to_string(),
            source: SourceKind::Modrinth,
            project_id: "AANobbMI".to_string(),
            slug: None,
            version_id: "v1".to_string(),
            version_number: "0.5.8".to_string(),
            version_type: None,
            filename: "sodium.jar".to_string(),
            url: "https://example.com".to_string(),
            sha512: None,
            sha1: None,
            size: None,
            side: Side::Both,
            reason: LockReason::Requested,
            dependencies: vec![],
        });

        assert!(lockfile.find_mod_by_project_id("AANobbMI").is_some());
        assert!(lockfile.find_mod_by_project_id("nonexistent").is_none());
    }
}
