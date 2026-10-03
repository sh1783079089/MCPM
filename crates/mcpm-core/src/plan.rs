use serde::{Deserialize, Serialize};

use crate::{
    Environment, LockEnvironment, LockedMod, Lockfile, McpmError, ResolveConflict, ResolveWarning,
    ResolvedMod,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum PlanAction {
    Install(ResolvedMod),
    Update { from: LockedMod, to: ResolvedMod },
    Remove(LockedMod),
    Keep(LockedMod),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePlan {
    pub environment: Environment,
    pub actions: Vec<PlanAction>,
    pub conflicts: Vec<ResolveConflict>,
    pub warnings: Vec<ResolveWarning>,
    pub success: bool,
}

impl UpdatePlan {
    pub fn has_changes(&self) -> bool {
        self.actions
            .iter()
            .any(|action| !matches!(action, PlanAction::Keep(_)))
    }

    pub fn removals(&self) -> Vec<&LockedMod> {
        self.actions
            .iter()
            .filter_map(|action| match action {
                PlanAction::Remove(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    /// Builds the target lockfile described by this plan.
    pub fn to_lockfile(&self, mcpm_version: &str) -> Result<Lockfile, McpmError> {
        let loader = self.environment.loader.as_fixed().ok_or_else(|| {
            McpmError::Config(
                "environment loader must be fixed for lockfile generation".to_string(),
            )
        })?;

        let lock_env = LockEnvironment {
            minecraft: self.environment.minecraft.clone(),
            loader,
            loader_version: self.environment.loader_version.clone(),
            side: self.environment.side,
        };

        let mut lockfile = Lockfile::new(mcpm_version, lock_env);
        for action in &self.actions {
            match action {
                PlanAction::Install(m) => lockfile.mods.push(resolved_to_locked(m)),
                PlanAction::Update { to, .. } => lockfile.mods.push(resolved_to_locked(to)),
                PlanAction::Keep(m) => lockfile.mods.push(m.clone()),
                PlanAction::Remove(_) => {}
            }
        }
        Ok(lockfile)
    }
}

pub fn resolved_to_locked(r: &ResolvedMod) -> LockedMod {
    LockedMod {
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
        manual_download: r.manual_download,
        website_url: r.website_url.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LoaderKind, LoaderSpec, LockReason, ReleaseType, Side, SourceKind};

    fn locked(name: &str, id: &str, version: &str) -> LockedMod {
        LockedMod {
            name: name.to_string(),
            source: SourceKind::Modrinth,
            project_id: id.to_string(),
            slug: Some(id.to_string()),
            version_id: format!("{}-{}", id, version),
            version_number: version.to_string(),
            version_type: Some(ReleaseType::Release),
            filename: format!("{}.jar", id),
            url: "https://example.com/x.jar".to_string(),
            sha512: None,
            sha1: None,
            size: None,
            side: Side::Both,
            reason: LockReason::Requested,
            dependencies: vec![],
            manual_download: false,
            website_url: None,
        }
    }

    fn resolved(id: &str, version: &str) -> ResolvedMod {
        ResolvedMod {
            name: id.to_string(),
            source: SourceKind::Modrinth,
            project_id: id.to_string(),
            slug: Some(id.to_string()),
            version_id: format!("{}-{}", id, version),
            version_number: version.to_string(),
            version_type: ReleaseType::Release,
            filename: format!("{}.jar", id),
            url: "https://example.com/x.jar".to_string(),
            sha512: None,
            sha1: None,
            size: None,
            side: Side::Both,
            reason: LockReason::Requested,
            dependencies: vec![],
            manual_download: false,
            website_url: None,
        }
    }

    fn env() -> Environment {
        Environment {
            minecraft: "1.20.1".to_string(),
            minecraft_range: None,
            loader: LoaderSpec::Fixed(LoaderKind::Fabric),
            loader_candidates: None,
            loader_version: Some("0.15.11".to_string()),
            side: Side::Both,
        }
    }

    #[test]
    fn test_plan_has_changes_and_removals() {
        let plan = UpdatePlan {
            environment: env(),
            actions: vec![
                PlanAction::Keep(locked("A", "a", "1.0.0")),
                PlanAction::Update {
                    from: locked("B", "b", "1.0.0"),
                    to: resolved("b", "2.0.0"),
                },
                PlanAction::Remove(locked("C", "c", "1.0.0")),
            ],
            conflicts: vec![],
            warnings: vec![],
            success: true,
        };

        assert!(plan.has_changes());
        assert_eq!(plan.removals().len(), 1);
    }

    #[test]
    fn test_plan_to_lockfile() {
        let plan = UpdatePlan {
            environment: env(),
            actions: vec![
                PlanAction::Keep(locked("A", "a", "1.0.0")),
                PlanAction::Update {
                    from: locked("B", "b", "1.0.0"),
                    to: resolved("b", "2.0.0"),
                },
                PlanAction::Remove(locked("C", "c", "1.0.0")),
            ],
            conflicts: vec![],
            warnings: vec![],
            success: true,
        };

        let lockfile = plan.to_lockfile("0.1.0").unwrap();
        assert_eq!(lockfile.mods.len(), 2);
        assert!(lockfile.find_mod_by_project_id("b").is_some());
        assert_eq!(
            lockfile.find_mod_by_project_id("b").unwrap().version_number,
            "2.0.0"
        );
        assert!(lockfile.find_mod_by_project_id("c").is_none());
    }
}
