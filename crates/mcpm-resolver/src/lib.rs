mod env;

pub use env::{
    default_minecraft_candidates, solve_env, CandidateEvaluation, EnvironmentCandidate,
    DEFAULT_MINECRAFT_VERSIONS,
};

use std::collections::{HashMap, VecDeque};

use mcpm_core::{
    compare_versions, platform_dependencies, CompatibilityConfig, DependencyKind, Environment,
    LoaderKind, LockReason, McpmError, ModEntry, ModVersion, Preferences, Registry, ReleaseType,
    ResolveConflict, ResolvePlan, ResolveWarning, ResolvedMod,
};

fn release_rank(release_type: ReleaseType) -> u8 {
    match release_type {
        ReleaseType::Alpha => 0,
        ReleaseType::Beta => 1,
        ReleaseType::Release => 2,
    }
}

/// Selects the best compatible version, honoring an optional pinned
/// version/id and preferring stable releases over pre-releases.
pub fn select_best_version<'a>(
    versions: &'a [ModVersion],
    game_version: &str,
    loader: LoaderKind,
    preferences: &Preferences,
    pin: Option<&str>,
) -> Option<&'a ModVersion> {
    let mut candidates: Vec<&ModVersion> = versions
        .iter()
        .filter(|v| {
            mcpm_core::version_matches_environment(v, game_version, loader)
                && mcpm_core::version_matches_preferences(v, preferences)
        })
        .collect();

    if let Some(pin) = pin {
        if !pin.is_empty() && !pin.eq_ignore_ascii_case("latest") {
            candidates.retain(|v| v.id == pin || v.version_number.eq_ignore_ascii_case(pin));
        }
    }

    candidates.sort_by(|a, b| {
        release_rank(a.version_type)
            .cmp(&release_rank(b.version_type))
            .then_with(|| compare_versions(&a.version_number, &b.version_number))
    });

    candidates.last().copied()
}

pub async fn resolve(
    registry: &dyn Registry,
    mods: &indexmap::IndexMap<String, ModEntry>,
    environment: &Environment,
    preferences: &Preferences,
) -> Result<ResolvePlan, McpmError> {
    resolve_impl(registry, mods, environment, preferences, false, None).await
}

/// Like [`resolve`], but applies pack-local compatibility rules.
pub async fn resolve_with_compat(
    registry: &dyn Registry,
    mods: &indexmap::IndexMap<String, ModEntry>,
    environment: &Environment,
    preferences: &Preferences,
    compatibility: Option<&CompatibilityConfig>,
) -> Result<ResolvePlan, McpmError> {
    resolve_impl(
        registry,
        mods,
        environment,
        preferences,
        false,
        compatibility,
    )
    .await
}

/// Like [`resolve`], but records requested mods that cannot be resolved
/// (missing or incompatible) instead of failing. Used when evaluating
/// candidate environments.
pub async fn resolve_partial(
    registry: &dyn Registry,
    mods: &indexmap::IndexMap<String, ModEntry>,
    environment: &Environment,
    preferences: &Preferences,
) -> Result<ResolvePlan, McpmError> {
    resolve_impl(registry, mods, environment, preferences, true, None).await
}

/// Like [`resolve_partial`], but applies pack-local compatibility rules.
pub async fn resolve_partial_with_compat(
    registry: &dyn Registry,
    mods: &indexmap::IndexMap<String, ModEntry>,
    environment: &Environment,
    preferences: &Preferences,
    compatibility: Option<&CompatibilityConfig>,
) -> Result<ResolvePlan, McpmError> {
    resolve_impl(
        registry,
        mods,
        environment,
        preferences,
        true,
        compatibility,
    )
    .await
}

async fn resolve_impl(
    registry: &dyn Registry,
    mods: &indexmap::IndexMap<String, ModEntry>,
    environment: &Environment,
    preferences: &Preferences,
    collect_missing: bool,
    compatibility: Option<&CompatibilityConfig>,
) -> Result<ResolvePlan, McpmError> {
    let game_version = &environment.minecraft;
    let loader = environment.loader.as_fixed().ok_or_else(|| {
        McpmError::Config(
            "environment loader must be fixed (not a list) for resolution".to_string(),
        )
    })?;

    let mut resolved: HashMap<String, ResolvedMod> = HashMap::new();
    let mut warnings: Vec<ResolveWarning> = Vec::new();
    let mut conflicts: Vec<ResolveConflict> = Vec::new();
    let mut missing: Vec<mcpm_core::MissingRequested> = Vec::new();

    struct QueueItem {
        name: String,
        project_id: Option<String>,
        reason: LockReason,
        version: Option<String>,
    }

    let mut queue: VecDeque<QueueItem> = VecDeque::new();

    for (name, entry) in mods {
        queue.push_back(QueueItem {
            name: name.clone(),
            project_id: entry.project_id().map(|s| s.to_string()),
            reason: LockReason::Requested,
            version: Some(entry.version().to_string()),
        });
    }

    for (slug, project_id) in platform_dependencies(loader) {
        queue.push_back(QueueItem {
            name: slug.to_string(),
            project_id: Some(project_id.to_string()),
            reason: LockReason::Dependency,
            version: None,
        });
    }

    while let Some(item) = queue.pop_front() {
        let project = if let Some(ref project_id) = item.project_id {
            registry.get_project(project_id).await?
        } else {
            let results = registry.search_projects(&item.name).await?;
            if results.is_empty() {
                if collect_missing {
                    missing.push(mcpm_core::MissingRequested {
                        name: item.name.clone(),
                        project_id: item.project_id.clone(),
                        message: format!("no project found for '{}'", item.name),
                    });
                    continue;
                }
                return Err(McpmError::SearchNoResults(item.name.clone()));
            }
            let best = &results[0];
            registry.get_project(&best.id).await?
        };

        if resolved.contains_key(&project.id) {
            continue;
        }

        if !preferences.source.is_empty() && !preferences.source.contains(&project.source) {
            if item.reason == LockReason::Requested {
                if collect_missing {
                    missing.push(mcpm_core::MissingRequested {
                        name: item.name.clone(),
                        project_id: item.project_id.clone(),
                        message: format!(
                            "source '{}' for '{}' is not allowed by preferences",
                            project.source, project.title
                        ),
                    });
                    continue;
                }
                return Err(McpmError::Config(format!(
                    "source '{}' for '{}' is not allowed by preferences",
                    project.source, project.title
                )));
            }
            warnings.push(ResolveWarning {
                project_id: project.id.clone(),
                name: project.title.clone(),
                message: format!(
                    "dependency '{}' from source '{}' skipped by source preferences",
                    project.title, project.source
                ),
            });
            continue;
        }

        let versions = registry.get_versions(&project.id).await?;
        let best_version = select_best_version(
            &versions,
            game_version,
            loader,
            preferences,
            item.version.as_deref(),
        );

        let best_version = match best_version {
            Some(v) => v,
            None => {
                if item.reason == LockReason::Requested {
                    if collect_missing {
                        missing.push(mcpm_core::MissingRequested {
                            name: item.name.clone(),
                            project_id: item.project_id.clone(),
                            message: format!(
                                "no compatible version for '{}' on Minecraft {} + {}",
                                project.title, game_version, loader
                            ),
                        });
                        continue;
                    }
                    return Err(McpmError::NoCompatibleVersion {
                        project: project.title.clone(),
                    });
                } else {
                    warnings.push(ResolveWarning {
                        project_id: project.id.clone(),
                        name: project.title.clone(),
                        message: format!(
                            "no compatible version found for dependency '{}' on Minecraft {} + {}",
                            project.title, game_version, loader
                        ),
                    });
                    continue;
                }
            }
        };

        let file = mcpm_core::select_primary_file(&best_version.files).ok_or_else(|| {
            McpmError::NoCompatibleVersion {
                project: project.title.clone(),
            }
        })?;

        let dep_project_ids: Vec<String> = best_version
            .dependencies
            .iter()
            .filter(|d| d.kind == DependencyKind::Required)
            .filter_map(|d| d.project_id.clone())
            .collect();

        let resolved_mod = ResolvedMod {
            name: project.title.clone(),
            source: project.source,
            project_id: project.id.clone(),
            slug: project.slug.clone(),
            version_id: best_version.id.clone(),
            version_number: best_version.version_number.clone(),
            version_type: best_version.version_type,
            filename: file.filename.clone(),
            url: file.url.clone(),
            sha512: file.hashes.sha512.clone(),
            sha1: file.hashes.sha1.clone(),
            size: file.size,
            side: mcpm_core::infer_mod_side(&project.client_side, &project.server_side),
            reason: item.reason,
            dependencies: dep_project_ids,
        };

        resolved.insert(project.id.clone(), resolved_mod);

        for dep in &best_version.dependencies {
            match dep.kind {
                DependencyKind::Required => {
                    if let Some(ref dep_project_id) = dep.project_id {
                        if !resolved.contains_key(dep_project_id) {
                            queue.push_back(QueueItem {
                                name: dep_project_id.clone(),
                                project_id: Some(dep_project_id.clone()),
                                reason: LockReason::Dependency,
                                version: dep.version_id.clone(),
                            });
                        }
                    }
                }
                DependencyKind::Optional => {
                    if let Some(ref dep_project_id) = dep.project_id {
                        warnings.push(ResolveWarning {
                            project_id: dep_project_id.clone(),
                            name: dep_project_id.clone(),
                            message: format!(
                                "optional dependency '{}' of '{}'",
                                dep_project_id, project.title
                            ),
                        });
                    }
                }
                DependencyKind::Incompatible => {
                    if let Some(ref dep_project_id) = dep.project_id {
                        let conflicting_name = resolved
                            .get(dep_project_id)
                            .map(|m| m.name.clone())
                            .unwrap_or_else(|| dep_project_id.clone());
                        conflicts.push(ResolveConflict {
                            project_id: project.id.clone(),
                            name: project.title.clone(),
                            conflicting_project_id: dep_project_id.clone(),
                            conflicting_name,
                            message: format!(
                                "'{}' declares incompatibility with '{}'",
                                project.title, dep_project_id
                            ),
                        });
                    }
                }
            }
        }
    }

    let success = conflicts.is_empty() && missing.is_empty();
    let resolved_mods: Vec<ResolvedMod> = resolved.into_values().collect();

    let mut plan = ResolvePlan {
        mods: resolved_mods,
        warnings,
        conflicts,
        missing,
        workarounds: Vec::new(),
        success,
    };

    if let Some(compatibility) = compatibility {
        mcpm_compat::apply_rules(&mut plan, environment, compatibility);
    }

    Ok(plan)
}

pub fn plan_to_lockfile(
    plan: &ResolvePlan,
    environment: &Environment,
    mcpm_version: &str,
) -> Result<mcpm_core::Lockfile, McpmError> {
    let loader = environment.loader.as_fixed().ok_or_else(|| {
        McpmError::Config("environment loader must be fixed for lockfile generation".to_string())
    })?;

    let lock_env = mcpm_core::LockEnvironment {
        minecraft: environment.minecraft.clone(),
        loader,
        loader_version: environment.loader_version.clone(),
        side: environment.side,
    };

    let mut lockfile = mcpm_core::Lockfile::new(mcpm_version, lock_env);

    for r in &plan.mods {
        lockfile.mods.push(mcpm_core::LockedMod {
            name: r.name.clone(),
            source: r.source,
            project_id: r.project_id.clone(),
            slug: r.slug.clone(),
            version_id: r.version_id.clone(),
            version_number: r.version_number.clone(),
            version_type: Some(r.version_type),
            filename: r.filename.clone(),
            url: r.url.clone(),
            sha512: r.sha512.clone(),
            sha1: r.sha1.clone(),
            size: r.size,
            side: r.side,
            reason: r.reason,
            dependencies: r.dependencies.clone(),
        });
    }

    Ok(lockfile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpm_core::*;
    use std::collections::HashMap;

    struct MockRegistry {
        projects: HashMap<String, Project>,
        versions: HashMap<String, Vec<ModVersion>>,
        search_results: HashMap<String, Vec<Project>>,
    }

    impl MockRegistry {
        fn new() -> Self {
            Self {
                projects: HashMap::new(),
                versions: HashMap::new(),
                search_results: HashMap::new(),
            }
        }

        fn add_project(&mut self, project: Project) {
            self.search_results
                .entry(project.slug.clone().unwrap_or_default().to_lowercase())
                .or_default()
                .push(project.clone());
            self.projects.insert(project.id.clone(), project);
        }

        fn add_version(&mut self, project_id: &str, version: ModVersion) {
            self.versions
                .entry(project_id.to_string())
                .or_default()
                .push(version);
        }
    }

    #[async_trait::async_trait]
    impl Registry for MockRegistry {
        async fn search_projects(&self, query: &str) -> Result<Vec<Project>, McpmError> {
            let key = query.to_lowercase();
            Ok(self.search_results.get(&key).cloned().unwrap_or_default())
        }

        async fn get_project(&self, id: &str) -> Result<Project, McpmError> {
            self.projects
                .get(id)
                .cloned()
                .ok_or_else(|| McpmError::ProjectNotFound(id.to_string()))
        }

        async fn get_versions(&self, project_id: &str) -> Result<Vec<ModVersion>, McpmError> {
            Ok(self.versions.get(project_id).cloned().unwrap_or_default())
        }
    }

    fn make_project(id: &str, slug: &str, title: &str) -> Project {
        Project {
            id: id.to_string(),
            slug: Some(slug.to_string()),
            title: title.to_string(),
            description: None,
            source: SourceKind::Modrinth,
            client_side: Side::Both,
            server_side: Side::Both,
        }
    }

    fn make_version(
        project_id: &str,
        version_number: &str,
        game_versions: &[&str],
        loaders: &[LoaderKind],
    ) -> ModVersion {
        ModVersion {
            id: format!("{}-{}", project_id, version_number),
            project_id: project_id.to_string(),
            name: Some(format!("{} {}", project_id, version_number)),
            version_number: version_number.to_string(),
            version_type: ReleaseType::Release,
            game_versions: game_versions.iter().map(|s| s.to_string()).collect(),
            loaders: loaders.to_vec(),
            files: vec![FileArtifact {
                filename: format!("{}-{}.jar", project_id, version_number),
                url: format!("https://example.com/{}-{}.jar", project_id, version_number),
                hashes: Hashes {
                    sha512: Some(format!("sha512-{}-{}", project_id, version_number)),
                    sha1: Some(format!("sha1-{}-{}", project_id, version_number)),
                },
                size: Some(12345),
                primary: true,
            }],
            dependencies: vec![],
        }
    }

    fn make_env(mc: &str, loader: LoaderKind) -> Environment {
        Environment {
            minecraft: mc.to_string(),
            minecraft_range: None,
            loader: LoaderSpec::Fixed(loader),
            loader_candidates: None,
            loader_version: None,
            side: Side::Both,
        }
    }

    fn make_prefs() -> Preferences {
        Preferences::default()
    }

    #[tokio::test]
    async fn test_resolve_simple_mod() {
        let mut reg = MockRegistry::new();
        let sodium = make_project("sodium-id", "sodium", "Sodium");
        let fabric_api = make_project("P7dR8mSH", "fabric-api", "Fabric API");
        reg.add_project(sodium);
        reg.add_project(fabric_api);
        reg.add_version(
            "sodium-id",
            make_version("sodium-id", "0.5.8", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("latest".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let prefs = make_prefs();
        let plan = resolve(&reg, &mods, &env, &prefs).await.unwrap();

        assert!(plan.success);
        assert_eq!(plan.mods.len(), 2);
        let sodium_mod = plan
            .mods
            .iter()
            .find(|m| m.project_id == "sodium-id")
            .unwrap();
        assert_eq!(sodium_mod.reason, LockReason::Requested);
    }

    #[tokio::test]
    async fn test_resolve_with_dependency() {
        let mut reg = MockRegistry::new();
        let sodium = make_project("sodium-id", "sodium", "Sodium");
        let fabric_api = make_project("fabric-api-id", "fabric-api", "Fabric API");
        let platform_fa = make_project("P7dR8mSH", "fabric-api", "Fabric API");
        reg.add_project(sodium.clone());
        reg.add_project(fabric_api);
        reg.add_project(platform_fa);

        let mut sodium_ver = make_version("sodium-id", "0.5.8", &["1.20.1"], &[LoaderKind::Fabric]);
        sodium_ver.dependencies.push(Dependency {
            project_id: Some("fabric-api-id".to_string()),
            version_id: None,
            kind: DependencyKind::Required,
        });
        reg.add_version("sodium-id", sodium_ver);

        reg.add_version(
            "fabric-api-id",
            make_version(
                "fabric-api-id",
                "0.90.0",
                &["1.20.1"],
                &[LoaderKind::Fabric],
            ),
        );
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("latest".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let prefs = make_prefs();
        let plan = resolve(&reg, &mods, &env, &prefs).await.unwrap();

        assert!(plan.success);
        assert_eq!(plan.mods.len(), 3);

        let project_ids: Vec<&str> = plan.mods.iter().map(|m| m.project_id.as_str()).collect();
        assert!(project_ids.contains(&"sodium-id"));
        assert!(project_ids.contains(&"fabric-api-id"));
        assert!(project_ids.contains(&"P7dR8mSH"));
    }

    #[tokio::test]
    async fn test_resolve_no_compatible_version() {
        let mut reg = MockRegistry::new();
        let sodium = make_project("sodium-id", "sodium", "Sodium");
        reg.add_project(sodium);
        reg.add_version(
            "sodium-id",
            make_version("sodium-id", "0.5.8", &["1.19.4"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("latest".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let prefs = make_prefs();
        let result = resolve(&reg, &mods, &env, &prefs).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_optional_dependency_warning() {
        let mut reg = MockRegistry::new();
        let sodium = make_project("sodium-id", "sodium", "Sodium");
        let fabric_api = make_project("P7dR8mSH", "fabric-api", "Fabric API");
        reg.add_project(sodium.clone());
        reg.add_project(fabric_api);

        let mut sodium_ver = make_version("sodium-id", "0.5.8", &["1.20.1"], &[LoaderKind::Fabric]);
        sodium_ver.dependencies.push(Dependency {
            project_id: Some("iris-id".to_string()),
            version_id: None,
            kind: DependencyKind::Optional,
        });
        reg.add_version("sodium-id", sodium_ver);
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("latest".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let prefs = make_prefs();
        let plan = resolve(&reg, &mods, &env, &prefs).await.unwrap();

        assert!(plan.success);
        assert_eq!(plan.mods.len(), 2);
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].message.contains("optional"));
        let fa_mod = plan
            .mods
            .iter()
            .find(|m| m.project_id == "P7dR8mSH")
            .unwrap();
        assert_eq!(fa_mod.reason, LockReason::Dependency);
    }

    #[tokio::test]
    async fn test_resolve_platform_dependency() {
        let mut reg = MockRegistry::new();
        let sodium = make_project("sodium-id", "sodium", "Sodium");
        let fabric_api = make_project("P7dR8mSH", "fabric-api", "Fabric API");
        reg.add_project(sodium);
        reg.add_project(fabric_api);
        reg.add_version(
            "sodium-id",
            make_version("sodium-id", "0.5.8", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("latest".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let prefs = make_prefs();
        let plan = resolve(&reg, &mods, &env, &prefs).await.unwrap();

        assert!(plan.success);
        assert_eq!(plan.mods.len(), 2);

        let sodium_mod = plan
            .mods
            .iter()
            .find(|m| m.project_id == "sodium-id")
            .unwrap();
        assert_eq!(sodium_mod.reason, LockReason::Requested);

        let fa_mod = plan
            .mods
            .iter()
            .find(|m| m.project_id == "P7dR8mSH")
            .unwrap();
        assert_eq!(fa_mod.reason, LockReason::Dependency);
    }

    #[tokio::test]
    async fn test_platform_dep_already_requested_not_duplicated() {
        let mut reg = MockRegistry::new();
        let fabric_api = make_project("P7dR8mSH", "fabric-api", "Fabric API");
        reg.add_project(fabric_api);
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert(
            "fabric-api".to_string(),
            ModEntry::Detailed(mcpm_core::DetailedModEntry {
                version: Some("latest".to_string()),
                source: Some(mcpm_core::SourceKind::Modrinth),
                side: None,
                project_id: Some("P7dR8mSH".to_string()),
            }),
        );

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let prefs = make_prefs();
        let plan = resolve(&reg, &mods, &env, &prefs).await.unwrap();

        assert!(plan.success);
        assert_eq!(plan.mods.len(), 1); // Not duplicated
        let fa_mod = plan
            .mods
            .iter()
            .find(|m| m.project_id == "P7dR8mSH")
            .unwrap();
        assert_eq!(fa_mod.reason, LockReason::Requested); // Respects user intent
    }

    #[tokio::test]
    async fn test_prefers_release_over_newer_beta() {
        let mut reg = MockRegistry::new();
        reg.add_project(make_project("sodium-id", "sodium", "Sodium"));
        reg.add_project(make_project("P7dR8mSH", "fabric-api", "Fabric API"));
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        reg.add_version(
            "sodium-id",
            make_version("sodium-id", "0.6.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        let mut beta = make_version("sodium-id", "0.7.0", &["1.20.1"], &[LoaderKind::Fabric]);
        beta.version_type = ReleaseType::Beta;
        reg.add_version("sodium-id", beta);

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("latest".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let plan = resolve(&reg, &mods, &env, &make_prefs()).await.unwrap();

        let sodium = plan
            .mods
            .iter()
            .find(|m| m.project_id == "sodium-id")
            .unwrap();
        assert_eq!(sodium.version_number, "0.6.0");
    }

    #[tokio::test]
    async fn test_requested_version_is_pinned() {
        let mut reg = MockRegistry::new();
        reg.add_project(make_project("sodium-id", "sodium", "Sodium"));
        reg.add_project(make_project("P7dR8mSH", "fabric-api", "Fabric API"));
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", "0.100.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        reg.add_version(
            "sodium-id",
            make_version("sodium-id", "0.5.8", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        reg.add_version(
            "sodium-id",
            make_version("sodium-id", "0.6.0", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("sodium".to_string(), ModEntry::Simple("0.5.8".to_string()));

        let env = make_env("1.20.1", LoaderKind::Fabric);
        let plan = resolve(&reg, &mods, &env, &make_prefs()).await.unwrap();

        let sodium = plan
            .mods
            .iter()
            .find(|m| m.project_id == "sodium-id")
            .unwrap();
        assert_eq!(sodium.version_number, "0.5.8");
    }
}
