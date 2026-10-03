use indexmap::IndexMap;
use mcpm_core::{
    CompatibilityConfig, Environment, LoaderKind, LoaderSpec, LockReason, McpmError, ModEntry,
    Preferences, Registry, ReleaseType, ResolveConflict, ResolvePlan, ResolveWarning, Side,
    VersionRange,
};

use crate::resolve_partial_with_compat;

/// Mainstream Minecraft release versions used to generate candidate
/// environments, newest first. Eventually this should come from the
/// Mojang version manifest.
pub const DEFAULT_MINECRAFT_VERSIONS: &[&str] =
    &["1.21.1", "1.20.1", "1.19.2", "1.18.2", "1.16.5", "1.12.2"];

#[derive(Debug, Clone)]
pub struct EnvironmentCandidate {
    pub minecraft: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub side: Side,
}

#[derive(Debug, Clone)]
pub struct CandidateEvaluation {
    pub candidate: EnvironmentCandidate,
    pub requested_available: usize,
    pub requested_total: usize,
    pub dependency_count: usize,
    pub missing: Vec<String>,
    pub conflicts: Vec<ResolveConflict>,
    pub warnings: Vec<ResolveWarning>,
    pub score: i32,
    pub notes: Vec<String>,
}

pub fn default_minecraft_candidates(range: Option<&VersionRange>) -> Vec<String> {
    DEFAULT_MINECRAFT_VERSIONS
        .iter()
        .filter(|v| range.map(|r| r.matches(v)).unwrap_or(true))
        .map(|v| v.to_string())
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub async fn solve_env(
    registry: &dyn Registry,
    mods: &IndexMap<String, ModEntry>,
    minecraft_range: Option<&VersionRange>,
    loader_candidates: &[LoaderKind],
    side: Side,
    preferences: &Preferences,
    compatibility: Option<&CompatibilityConfig>,
) -> Result<Vec<CandidateEvaluation>, McpmError> {
    let minecraft_versions = default_minecraft_candidates(minecraft_range);
    let loaders: Vec<LoaderKind> = if loader_candidates.is_empty() {
        vec![LoaderKind::Fabric]
    } else {
        loader_candidates.to_vec()
    };

    let mut evaluations = Vec::new();

    for mc in &minecraft_versions {
        for loader in &loaders {
            let environment = Environment {
                minecraft: mc.clone(),
                minecraft_range: None,
                loader: LoaderSpec::Fixed(*loader),
                loader_candidates: None,
                loader_version: None,
                side,
            };
            let plan = resolve_partial_with_compat(
                registry,
                mods,
                &environment,
                preferences,
                compatibility,
            )
            .await?;

            let candidate = EnvironmentCandidate {
                minecraft: mc.clone(),
                loader: *loader,
                loader_version: environment.loader_version.clone(),
                side,
            };
            evaluations.push(evaluate(candidate, plan));
        }
    }

    evaluations.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.candidate.minecraft.cmp(&b.candidate.minecraft))
    });

    Ok(evaluations)
}

fn evaluate(candidate: EnvironmentCandidate, plan: ResolvePlan) -> CandidateEvaluation {
    let requested_available = plan
        .mods
        .iter()
        .filter(|m| m.reason == LockReason::Requested)
        .count();
    let requested_total = requested_available + plan.missing.len();
    let dependency_count = plan
        .mods
        .iter()
        .filter(|m| m.reason == LockReason::Dependency)
        .count();
    let missing: Vec<String> = plan.missing.iter().map(|m| m.name.clone()).collect();

    let mut score = 0;
    let mut notes = Vec::new();

    if plan.missing.is_empty() {
        score += 50;
    } else {
        score -= 30 * plan.missing.len() as i32;
        notes.push(format!(
            "{} requested mod(s) unavailable",
            plan.missing.len()
        ));
    }

    if plan.conflicts.is_empty() {
        score += 10;
    } else {
        score -= 25 * plan.conflicts.len() as i32;
        notes.push(format!("{} dependency conflict(s)", plan.conflicts.len()));
    }

    let prerelease_count = plan
        .mods
        .iter()
        .filter(|m| m.version_type != ReleaseType::Release)
        .count();
    if prerelease_count == 0 {
        score += 10;
    } else {
        score -= 15;
        notes.push(format!("{} pre-release mod(s) required", prerelease_count));
    }

    score -= 5 * plan.warnings.len() as i32;
    if !plan.warnings.is_empty() {
        notes.push(format!("{} warning(s)", plan.warnings.len()));
    }

    score += minecraft_bonus(&candidate.minecraft);

    CandidateEvaluation {
        candidate,
        requested_available,
        requested_total,
        dependency_count,
        missing,
        conflicts: plan.conflicts,
        warnings: plan.warnings,
        score,
        notes,
    }
}

fn minecraft_bonus(minecraft: &str) -> i32 {
    match DEFAULT_MINECRAFT_VERSIONS
        .iter()
        .position(|v| *v == minecraft)
    {
        Some(index) => (10 - index as i32 * 2).max(0),
        None => 0,
    }
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
            Ok(self
                .search_results
                .get(&query.to_lowercase())
                .cloned()
                .unwrap_or_default())
        }

        async fn get_project(&self, _source: SourceKind, id: &str) -> Result<Project, McpmError> {
            self.projects
                .get(id)
                .cloned()
                .ok_or_else(|| McpmError::ProjectNotFound(id.to_string()))
        }

        async fn get_versions(
            &self,
            _source: SourceKind,
            project_id: &str,
        ) -> Result<Vec<ModVersion>, McpmError> {
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
            website_url: None,
        }
    }

    fn make_version(
        project_id: &str,
        game_versions: &[&str],
        loaders: &[LoaderKind],
    ) -> ModVersion {
        ModVersion {
            id: format!("{}-v", project_id),
            project_id: project_id.to_string(),
            name: None,
            version_number: "1.0.0".to_string(),
            version_type: ReleaseType::Release,
            game_versions: game_versions.iter().map(|s| s.to_string()).collect(),
            loaders: loaders.to_vec(),
            files: vec![FileArtifact {
                filename: format!("{}.jar", project_id),
                url: "https://example.com/x.jar".to_string(),
                hashes: Hashes {
                    sha512: Some("hash".to_string()),
                    sha1: None,
                },
                size: Some(1),
                primary: true,
            }],
            dependencies: vec![],
        }
    }

    #[tokio::test]
    async fn test_solve_env_ranks_most_compatible_first() {
        let mut reg = MockRegistry::new();
        let create = make_project("create", "create", "Create");
        let fabric_api = make_project("P7dR8mSH", "fabric-api", "Fabric API");
        reg.add_project(create.clone());
        reg.add_project(fabric_api.clone());
        // Only available for 1.20.1 + fabric
        reg.add_version(
            "create",
            make_version("create", &["1.20.1"], &[LoaderKind::Fabric]),
        );
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", &["1.20.1", "1.21.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("create".to_string(), ModEntry::Simple("latest".to_string()));

        let evals = solve_env(
            &reg,
            &mods,
            Some(&VersionRange::parse(">=1.20 <1.22").unwrap()),
            &[LoaderKind::Fabric],
            Side::Both,
            &Preferences::default(),
            None,
        )
        .await
        .unwrap();

        assert!(!evals.is_empty());
        assert_eq!(evals[0].candidate.minecraft, "1.20.1");
        assert_eq!(evals[0].requested_available, 1);
        assert_eq!(evals[0].requested_total, 1);
        assert!(evals[0].missing.is_empty());
        assert!(evals[0].score > 50);
    }

    #[tokio::test]
    async fn test_solve_env_reports_missing_mods() {
        let mut reg = MockRegistry::new();
        reg.add_project(make_project("create", "create", "Create"));
        reg.add_project(make_project("P7dR8mSH", "fabric-api", "Fabric API"));
        reg.add_version(
            "P7dR8mSH",
            make_version("P7dR8mSH", &["1.20.1"], &[LoaderKind::Fabric]),
        );

        let mut mods = indexmap::IndexMap::new();
        mods.insert("create".to_string(), ModEntry::Simple("latest".to_string()));

        let evals = solve_env(
            &reg,
            &mods,
            Some(&VersionRange::parse("1.20.1").unwrap()),
            &[LoaderKind::Fabric],
            Side::Both,
            &Preferences::default(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(evals[0].requested_total, 1);
        assert_eq!(evals[0].requested_available, 0);
        assert_eq!(evals[0].missing, vec!["create".to_string()]);
    }
}
