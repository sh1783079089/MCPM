use mcpm_core::{
    CompatibilityConfig, Environment, LocalConflictRule, ResolveConflict, ResolvePlan,
    ResolveWarning, RuleSeverity, VersionRange, Workaround,
};

/// Applies pack-local compatibility rules to a resolved plan.
///
/// Hard conflicts mark the plan as unsuccessful, soft conflicts are
/// recorded as warnings, and matching workarounds are collected as hints.
/// Both hard and soft conflicts therefore affect environment scoring.
pub fn apply_rules(
    plan: &mut ResolvePlan,
    environment: &Environment,
    compatibility: &CompatibilityConfig,
) {
    let mut hard_conflict = false;

    for rule in &compatibility.conflicts {
        if !rule_applies(rule, environment) {
            continue;
        }
        let matched = match match_rule(rule, plan) {
            Some(matched) => matched,
            None => continue,
        };

        match rule.severity {
            RuleSeverity::Hard => {
                hard_conflict = true;
                plan.conflicts.push(ResolveConflict {
                    project_id: matched.first().cloned().unwrap_or_default(),
                    name: matched.join(", "),
                    conflicting_project_id: matched.get(1).cloned().unwrap_or_default(),
                    conflicting_name: matched.get(1).cloned().unwrap_or_default(),
                    message: format!("local hard conflict: {}", rule.reason),
                });
            }
            RuleSeverity::Soft => {
                plan.warnings.push(ResolveWarning {
                    project_id: matched.first().cloned().unwrap_or_default(),
                    name: matched.join(", "),
                    message: format!("local soft conflict: {}", rule.reason),
                });
            }
        }
    }

    if hard_conflict {
        plan.success = false;
    }

    for workaround in &compatibility.workarounds {
        if workaround_applies(workaround, plan) {
            plan.workarounds.push(workaround.reason.clone());
        }
    }
}

fn rule_applies(rule: &LocalConflictRule, environment: &Environment) -> bool {
    if let Some(ref minecraft) = rule.minecraft {
        let matches = VersionRange::parse(minecraft)
            .map(|range| range.matches(&environment.minecraft))
            .unwrap_or_else(|_| minecraft == &environment.minecraft);
        if !matches {
            return false;
        }
    }

    if let Some(loader) = rule.loader {
        if environment.loader.as_fixed() != Some(loader) {
            return false;
        }
    }

    true
}

fn match_rule(rule: &LocalConflictRule, plan: &ResolvePlan) -> Option<Vec<String>> {
    let mut matched = Vec::new();

    for constraint in &rule.mods {
        let found = plan.mods.iter().find(|m| {
            m.project_id == constraint.id || m.slug.as_deref() == Some(constraint.id.as_str())
        })?;

        if let Some(ref version) = constraint.version {
            let ok = VersionRange::parse(version)
                .map(|range| range.matches(&found.version_number))
                .unwrap_or(false);
            if !ok {
                return None;
            }
        }

        matched.push(found.name.clone());
    }

    Some(matched)
}

fn workaround_applies(workaround: &Workaround, plan: &ResolvePlan) -> bool {
    workaround.mods.iter().all(|id| {
        plan.mods
            .iter()
            .any(|m| m.project_id == *id || m.slug.as_deref() == Some(id.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpm_core::{
        LoaderKind, LoaderSpec, LockReason, ModConstraint, ResolvedMod, Side, SourceKind,
    };

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

    fn make_mod(name: &str, id: &str, version: &str) -> ResolvedMod {
        ResolvedMod {
            name: name.to_string(),
            source: SourceKind::Modrinth,
            project_id: id.to_string(),
            slug: Some(id.to_string()),
            version_id: format!("{}-{}", id, version),
            version_number: version.to_string(),
            version_type: mcpm_core::ReleaseType::Release,
            filename: format!("{}.jar", id),
            url: "https://example.com/x.jar".to_string(),
            sha512: None,
            sha1: None,
            size: None,
            side: Side::Both,
            reason: LockReason::Requested,
            dependencies: vec![],
        }
    }

    fn empty_plan(mods: Vec<ResolvedMod>) -> ResolvePlan {
        ResolvePlan {
            mods,
            warnings: vec![],
            conflicts: vec![],
            missing: vec![],
            workarounds: vec![],
            success: true,
        }
    }

    #[test]
    fn test_hard_conflict_blocks_plan() {
        let mut plan = empty_plan(vec![
            make_mod("Mod A", "mod_a", "1.4.2"),
            make_mod("Mod B", "mod_b", "3.2.1"),
        ]);
        let compat = CompatibilityConfig {
            conflicts: vec![LocalConflictRule {
                mods: vec![
                    ModConstraint {
                        id: "mod_a".to_string(),
                        version: Some(">=1.4 <1.5".to_string()),
                    },
                    ModConstraint {
                        id: "mod_b".to_string(),
                        version: Some(">=3.2 <3.3".to_string()),
                    },
                ],
                minecraft: Some("1.20.1".to_string()),
                loader: Some(LoaderKind::Fabric),
                severity: RuleSeverity::Hard,
                reason: "Crashes on startup".to_string(),
            }],
            workarounds: vec![],
        };

        apply_rules(&mut plan, &make_env("1.20.1", LoaderKind::Fabric), &compat);

        assert!(!plan.success);
        assert_eq!(plan.conflicts.len(), 1);
    }

    #[test]
    fn test_rule_not_applied_on_other_environment() {
        let mut plan = empty_plan(vec![
            make_mod("Mod A", "mod_a", "1.4.2"),
            make_mod("Mod B", "mod_b", "3.2.1"),
        ]);
        let compat = CompatibilityConfig {
            conflicts: vec![LocalConflictRule {
                mods: vec![
                    ModConstraint {
                        id: "mod_a".to_string(),
                        version: None,
                    },
                    ModConstraint {
                        id: "mod_b".to_string(),
                        version: None,
                    },
                ],
                minecraft: Some("1.20.1".to_string()),
                loader: Some(LoaderKind::Forge),
                severity: RuleSeverity::Hard,
                reason: "Forge only".to_string(),
            }],
            workarounds: vec![],
        };

        apply_rules(&mut plan, &make_env("1.20.1", LoaderKind::Fabric), &compat);

        assert!(plan.success);
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn test_soft_conflict_warns_and_workaround_hint() {
        let mut plan = empty_plan(vec![
            make_mod("Mod A", "mod_a", "1.4.2"),
            make_mod("Mod B", "mod_b", "3.2.1"),
        ]);
        let compat = CompatibilityConfig {
            conflicts: vec![LocalConflictRule {
                mods: vec![
                    ModConstraint {
                        id: "mod_a".to_string(),
                        version: None,
                    },
                    ModConstraint {
                        id: "mod_b".to_string(),
                        version: None,
                    },
                ],
                minecraft: None,
                loader: None,
                severity: RuleSeverity::Soft,
                reason: "Visual glitch".to_string(),
            }],
            workarounds: vec![Workaround {
                mods: vec!["mod_a".to_string(), "mod_b".to_string()],
                reason: "Disable feature X".to_string(),
                actions: vec![],
            }],
        };

        apply_rules(&mut plan, &make_env("1.20.1", LoaderKind::Fabric), &compat);

        assert!(plan.success);
        assert_eq!(plan.warnings.len(), 1);
        assert_eq!(plan.workarounds, vec!["Disable feature X".to_string()]);
    }
}
