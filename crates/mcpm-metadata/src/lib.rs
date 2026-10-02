use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use mcpm_core::{LoaderKind, Side, VersionRange};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("no mod metadata found in archive")]
    NoMetadata,
}

#[derive(Debug, Clone)]
pub struct DependencyRange {
    pub id: String,
    pub range: Option<VersionRange>,
}

#[derive(Debug, Clone)]
pub struct ModMetadata {
    pub id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub environment: Side,
    pub loader: LoaderKind,
    pub depends: Vec<DependencyRange>,
    pub conflicts: Vec<String>,
    pub loader_version: Option<String>,
}

impl ModMetadata {
    pub fn is_client_only(&self) -> bool {
        self.environment == Side::Client
    }

    pub fn is_server_only(&self) -> bool {
        self.environment == Side::Server
    }
}

const RESERVED_DEPENDENCIES: &[&str] = &[
    "minecraft",
    "java",
    "fabricloader",
    "fabric",
    "forge",
    "neoforge",
    "quilt_loader",
    "quilt_base",
];

fn parse_version_range(raw: &str) -> Option<VersionRange> {
    VersionRange::parse(raw).ok()
}

/// Parses all mod metadata declarations contained in a jar file.
pub fn parse_jar(path: &Path) -> Result<Vec<ModMetadata>, MetadataError> {
    let file = std::fs::File::open(path)?;
    parse_reader(file)
}

pub fn parse_reader<R: Read + std::io::Seek>(reader: R) -> Result<Vec<ModMetadata>, MetadataError> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut result = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        match name.as_str() {
            "fabric.mod.json" => {
                let text = read_entry(&mut entry)?;
                result.push(parse_fabric(&text)?);
            }
            "quilt.mod.json" => {
                let text = read_entry(&mut entry)?;
                if let Some(metadata) = parse_quilt(&text)? {
                    result.push(metadata);
                }
            }
            "META-INF/mods.toml" => {
                let text = read_entry(&mut entry)?;
                result.extend(parse_forge_like(&text, LoaderKind::Forge)?);
            }
            "META-INF/neoforge.mods.toml" => {
                let text = read_entry(&mut entry)?;
                result.extend(parse_forge_like(&text, LoaderKind::NeoForge)?);
            }
            _ => {}
        }
    }

    if result.is_empty() {
        return Err(MetadataError::NoMetadata);
    }
    Ok(result)
}

fn read_entry<R: Read>(entry: &mut R) -> Result<String, MetadataError> {
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(text)
}

fn normalize_side(raw: Option<&str>) -> Side {
    match raw.unwrap_or("*").trim().to_lowercase().as_str() {
        "client" => Side::Client,
        "server" => Side::Server,
        _ => Side::Both,
    }
}

fn normalize_forge_side(raw: Option<&str>) -> Side {
    match raw.unwrap_or("BOTH").trim().to_lowercase().as_str() {
        "client" => Side::Client,
        "server" => Side::Server,
        _ => Side::Both,
    }
}

#[derive(Debug, Deserialize)]
struct FabricModJson {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    depends: HashMap<String, String>,
    #[serde(default)]
    conflicts: HashMap<String, String>,
}

fn parse_fabric(text: &str) -> Result<ModMetadata, MetadataError> {
    let raw: FabricModJson = serde_json::from_str(text)?;

    let loader_version = raw.depends.get("fabricloader").cloned();

    let depends = raw
        .depends
        .into_iter()
        .map(|(id, version)| DependencyRange {
            id,
            range: parse_version_range(&version),
        })
        .collect();

    Ok(ModMetadata {
        id: raw.id,
        name: raw.name,
        version: raw.version,
        environment: normalize_side(raw.environment.as_deref()),
        loader: LoaderKind::Fabric,
        depends,
        conflicts: raw.conflicts.into_keys().collect(),
        loader_version,
    })
}

#[derive(Debug, Deserialize)]
struct QuiltModJson {
    #[serde(default)]
    quilt_loader: Option<QuiltLoader>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuiltLoader {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    metadata: Option<QuiltMetadata>,
    #[serde(default)]
    depends: Vec<QuiltDependency>,
}

#[derive(Debug, Deserialize)]
struct QuiltMetadata {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuiltDependency {
    id: String,
    #[serde(default)]
    versions: Option<String>,
}

fn parse_quilt(text: &str) -> Result<Option<ModMetadata>, MetadataError> {
    let raw: QuiltModJson = serde_json::from_str(text)?;
    let Some(loader) = raw.quilt_loader else {
        return Ok(None);
    };

    let depends = loader
        .depends
        .into_iter()
        .map(|dep| DependencyRange {
            id: dep.id,
            range: dep.versions.as_deref().and_then(parse_version_range),
        })
        .collect();

    Ok(Some(ModMetadata {
        id: loader.id,
        name: loader.metadata.and_then(|m| m.name).or(raw.id),
        version: loader.version.or(raw.version),
        environment: Side::Both,
        loader: LoaderKind::Quilt,
        depends,
        conflicts: Vec::new(),
        loader_version: None,
    }))
}

#[derive(Debug, Deserialize)]
struct ForgeModsToml {
    #[serde(default, rename = "loaderVersion")]
    loader_version: Option<String>,
    #[serde(default)]
    mods: Vec<ForgeMod>,
    #[serde(default)]
    dependencies: HashMap<String, Vec<ForgeDependency>>,
}

#[derive(Debug, Deserialize)]
struct ForgeMod {
    #[serde(rename = "modId")]
    mod_id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    side: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForgeDependency {
    #[serde(rename = "modId")]
    mod_id: String,
    #[serde(default)]
    mandatory: bool,
    #[serde(default, rename = "versionRange")]
    version_range: Option<String>,
    #[serde(default, rename = "side")]
    _side: Option<String>,
}

fn parse_forge_like(text: &str, loader: LoaderKind) -> Result<Vec<ModMetadata>, MetadataError> {
    let raw: ForgeModsToml = toml::from_str(text)?;

    let mut dependencies: Vec<DependencyRange> = Vec::new();
    for deps in raw.dependencies.values() {
        for dep in deps {
            if !dep.mandatory {
                continue;
            }
            dependencies.push(DependencyRange {
                id: dep.mod_id.clone(),
                range: dep.version_range.as_deref().and_then(parse_version_range),
            });
        }
    }

    Ok(raw
        .mods
        .into_iter()
        .map(|m| ModMetadata {
            id: m.mod_id,
            name: m.display_name,
            version: m.version,
            environment: normalize_forge_side(m.side.as_deref()),
            loader,
            depends: dependencies.clone(),
            conflicts: Vec::new(),
            loader_version: raw.loader_version.clone(),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct ParsedMod {
    pub filename: String,
    pub metadata: ModMetadata,
}

#[derive(Debug, Clone)]
pub struct SideConflict {
    pub filename: String,
    pub id: String,
    pub mod_side: Side,
    pub expected: Side,
}

#[derive(Debug, Clone)]
pub struct MissingDependency {
    pub filename: String,
    pub id: String,
    pub missing: String,
}

#[derive(Debug, Clone)]
pub struct LoaderMismatch {
    pub filename: String,
    pub id: String,
    pub expected: LoaderKind,
    pub actual: LoaderKind,
}

#[derive(Debug, Clone, Default)]
pub struct VerifyReport {
    pub mods: Vec<ParsedMod>,
    pub parse_errors: Vec<(String, String)>,
    pub duplicate_ids: Vec<String>,
    pub side_conflicts: Vec<SideConflict>,
    pub missing_dependencies: Vec<MissingDependency>,
    pub loader_mismatches: Vec<LoaderMismatch>,
    pub loader_version_requirements: Vec<String>,
}

/// Scans a `mods/` directory, parses each jar, and cross-checks the
/// installed set against the expected side and loader.
pub fn verify_dir(
    mods_dir: &Path,
    expected_side: Side,
    expected_loader: LoaderKind,
) -> VerifyReport {
    let mut report = VerifyReport::default();

    let entries = match std::fs::read_dir(mods_dir) {
        Ok(entries) => entries,
        Err(_) => return report,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jar") {
            continue;
        }
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        match parse_jar(&path) {
            Ok(metadatas) => {
                for metadata in metadatas {
                    report.mods.push(ParsedMod {
                        filename: filename.clone(),
                        metadata,
                    });
                }
            }
            Err(e) => report.parse_errors.push((filename, e.to_string())),
        }
    }

    let mut id_counts: HashMap<String, usize> = HashMap::new();
    for parsed in &report.mods {
        *id_counts.entry(parsed.metadata.id.clone()).or_insert(0) += 1;
    }
    let mut duplicate_ids: Vec<String> = id_counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, _)| id.clone())
        .collect();
    duplicate_ids.sort();
    report.duplicate_ids = duplicate_ids;

    let known_ids: HashSet<&str> = report.mods.iter().map(|m| m.metadata.id.as_str()).collect();

    for parsed in &report.mods {
        let metadata = &parsed.metadata;

        if (expected_side == Side::Server && metadata.is_client_only())
            || (expected_side == Side::Client && metadata.is_server_only())
        {
            report.side_conflicts.push(SideConflict {
                filename: parsed.filename.clone(),
                id: metadata.id.clone(),
                mod_side: metadata.environment,
                expected: expected_side,
            });
        }

        if metadata.loader != expected_loader {
            report.loader_mismatches.push(LoaderMismatch {
                filename: parsed.filename.clone(),
                id: metadata.id.clone(),
                expected: expected_loader,
                actual: metadata.loader,
            });
        }

        if let Some(ref requirement) = metadata.loader_version {
            report
                .loader_version_requirements
                .push(format!("{}: {}", metadata.id, requirement));
        }

        for dep in &metadata.depends {
            if RESERVED_DEPENDENCIES.contains(&dep.id.as_str()) {
                continue;
            }
            if !known_ids.contains(dep.id.as_str()) {
                report.missing_dependencies.push(MissingDependency {
                    filename: parsed.filename.clone(),
                    id: metadata.id.clone(),
                    missing: dep.id.clone(),
                });
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fabric_metadata() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "sodium",
            "version": "0.5.8",
            "name": "Sodium",
            "environment": "client",
            "depends": { "fabricloader": ">=0.15", "minecraft": "1.20.1" },
            "conflicts": { "embeddium": "*" }
        }"#;
        let metadata = parse_fabric(json).unwrap();
        assert_eq!(metadata.id, "sodium");
        assert_eq!(metadata.environment, Side::Client);
        assert!(metadata.is_client_only());
        assert_eq!(metadata.loader_version.as_deref(), Some(">=0.15"));
        assert_eq!(metadata.conflicts, vec!["embeddium".to_string()]);
    }

    #[test]
    fn test_parse_forge_metadata() {
        let toml_str = r#"
modLoader = "javafml"
loaderVersion = "[47,)"
license = "MIT"

[[mods]]
modId = "create"
version = "0.5.1.f"
displayName = "Create"

[[dependencies.create]]
modId = "minecraft"
mandatory = true
versionRange = "[1.20.1,1.21)"
ordering = "NONE"
side = "BOTH"
"#;
        let mods = parse_forge_like(toml_str, LoaderKind::Forge).unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].id, "create");
        assert_eq!(mods[0].loader, LoaderKind::Forge);
        assert_eq!(mods[0].loader_version.as_deref(), Some("[47,)"));
        assert_eq!(mods[0].depends.len(), 1);
        assert_eq!(mods[0].depends[0].id, "minecraft");
        assert!(mods[0].depends[0].range.as_ref().unwrap().matches("1.20.1"));
    }

    #[test]
    fn test_metadata_error_without_files() {
        assert!(matches!(
            parse_reader(std::io::Cursor::new(Vec::new())),
            Err(MetadataError::Zip(_))
        ));
    }
}
