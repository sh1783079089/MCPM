use serde::{Deserialize, Serialize};

use mcpm_core::{LoaderKind, Lockfile};

/// CurseForge modpack manifest (`manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfManifest {
    pub minecraft: CfMinecraft,
    #[serde(default = "default_manifest_type")]
    pub manifest_type: String,
    #[serde(default = "default_manifest_version")]
    pub manifest_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub files: Vec<CfManifestFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfMinecraft {
    pub version: String,
    #[serde(rename = "modLoaders")]
    pub mod_loaders: Vec<CfModLoader>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfModLoader {
    pub id: String,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_manifest_type() -> String {
    "minecraftModpack".to_string()
}

fn default_manifest_version() -> u32 {
    1
}

fn default_required() -> bool {
    true
}

pub fn parse_cf_manifest(content: &str) -> Result<CfManifest, String> {
    serde_json::from_str(content).map_err(|e| format!("failed to parse manifest: {}", e))
}

pub fn write_cf_manifest(manifest: &CfManifest) -> Result<String, String> {
    serde_json::to_string_pretty(manifest).map_err(|e| format!("failed to write manifest: {}", e))
}

/// Extracts loader kind and loader version from a CurseForge mod loader id
/// such as `fabric-0.15.11` or `forge-47.2.20`.
pub fn split_loader_id(loader_id: &str) -> Option<(LoaderKind, Option<String>)> {
    let (kind, version) = match loader_id.split_once('-') {
        Some((kind, version)) => (kind, Some(version.to_string())),
        None => (loader_id, None),
    };
    let loader = match kind.to_lowercase().as_str() {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        _ => return None,
    };
    Some((loader, version))
}

/// Builds a CurseForge manifest from a lockfile. Only CurseForge entries can
/// be represented; entries from other sources are returned so the caller
/// can warn about them.
pub fn lockfile_to_cf_manifest(
    lockfile: &Lockfile,
    name: &str,
    version: &str,
    author: Option<&str>,
) -> (CfManifest, Vec<String>) {
    let loader_id = match lockfile.environment.loader_version.as_deref() {
        Some(loader_version) => format!("{}-{}", lockfile.environment.loader, loader_version),
        None => lockfile.environment.loader.to_string(),
    };

    let mut files = Vec::new();
    let mut skipped = Vec::new();

    for m in &lockfile.mods {
        if m.source != mcpm_core::SourceKind::CurseForge {
            skipped.push(m.name.clone());
            continue;
        }
        let (Ok(project_id), Ok(file_id)) =
            (m.project_id.parse::<u64>(), m.version_id.parse::<u64>())
        else {
            skipped.push(m.name.clone());
            continue;
        };
        files.push(CfManifestFile {
            project_id,
            file_id,
            required: m.reason == mcpm_core::LockReason::Requested,
        });
    }

    let manifest = CfManifest {
        minecraft: CfMinecraft {
            version: lockfile.environment.minecraft.clone(),
            mod_loaders: vec![CfModLoader {
                id: loader_id,
                primary: true,
            }],
        },
        manifest_type: default_manifest_type(),
        manifest_version: default_manifest_version(),
        name: name.to_string(),
        version: Some(version.to_string()),
        author: author.map(str::to_string),
        files,
        overrides: Some("overrides".to_string()),
    };

    (manifest, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_roundtrip() {
        let content = r#"{
            "minecraft": {
                "version": "1.20.1",
                "modLoaders": [{ "id": "forge-47.2.20", "primary": true }]
            },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Tech Adventure",
            "version": "1.0.0",
            "author": "example",
            "files": [
                { "projectID": 238222, "fileID": 5000000, "required": true },
                { "projectID": 999, "fileID": 1000 }
            ],
            "overrides": "overrides"
        }"#;

        let manifest = parse_cf_manifest(content).expect("parse");
        assert_eq!(manifest.name, "Tech Adventure");
        assert_eq!(manifest.minecraft.version, "1.20.1");
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.files[0].required);
        assert!(manifest.files[1].required); // defaults to true

        let written = write_cf_manifest(&manifest).expect("write");
        let reparsed = parse_cf_manifest(&written).expect("reparse");
        assert_eq!(reparsed.files.len(), 2);
        assert_eq!(reparsed.minecraft.mod_loaders[0].id, "forge-47.2.20");
    }

    #[test]
    fn test_split_loader_id() {
        let (loader, version) = split_loader_id("fabric-0.15.11").unwrap();
        assert_eq!(loader, LoaderKind::Fabric);
        assert_eq!(version.as_deref(), Some("0.15.11"));

        let (loader, version) = split_loader_id("forge").unwrap();
        assert_eq!(loader, LoaderKind::Forge);
        assert!(version.is_none());

        assert!(split_loader_id("unknown-1.0").is_none());
    }
}
