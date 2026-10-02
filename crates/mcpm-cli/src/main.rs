mod i18n;

rust_i18n::i18n!("locales", fallback = "en");

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Command, CommandFactory, FromArgMatches, Parser, Subcommand};
use i18n::t;
use mcpm_core::{
    version_matches_environment, version_matches_preferences, DetailedModEntry, Environment,
    EnvironmentStrategy, LoaderKind, LoaderSpec, LockReason, ModEntry, PackConfig, PackMeta,
    Preferences, Registry, Side, VersionRange,
};
use mcpm_installer::Installer;
use mcpm_registry::ModrinthClient;

#[derive(Parser)]
#[command(name = "mcpm", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new modpack
    Init {
        /// Minecraft version (e.g. 1.20.1)
        #[arg(long)]
        minecraft: String,

        /// Mod loader (fabric, quilt, forge, neoforge)
        #[arg(long)]
        loader: String,

        /// Pack name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,

        /// Side (client, server, both)
        #[arg(long, default_value = "both")]
        side: String,
    },

    /// Add one or more mods to the modpack
    Add {
        /// Mod slugs or project IDs
        #[arg(required = true)]
        slugs: Vec<String>,
    },

    /// Remove a mod from the modpack
    Remove {
        /// Mod slug
        slug: String,
    },

    /// Remove orphaned dependencies
    Autoremove,

    /// Resolve dependencies and install mods
    Install,

    /// List mods in the lockfile
    List,

    /// Check for issues in the resolved modpack
    Check,

    /// Explore and rank candidate environments
    SolveEnv,

    /// Apply a candidate environment from solve-env
    Use {
        /// Candidate index (1-based)
        candidate: usize,
    },

    /// Lock the resolved environment into the config
    LockEnv {
        /// Candidate index to lock when multiple candidates exist
        #[arg(long)]
        choose: Option<usize>,
    },

    /// Restore the environment to explore mode
    UnLockEnv,
}

const CONFIG_FILE: &str = "mcpm.toml";
const LOCK_FILE: &str = "mcpm.lock.json";

fn setup_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("mcpm=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

fn parse_loader(s: &str) -> Result<LoaderKind> {
    match s.to_lowercase().as_str() {
        "fabric" => Ok(LoaderKind::Fabric),
        "quilt" => Ok(LoaderKind::Quilt),
        "forge" => Ok(LoaderKind::Forge),
        "neoforge" => Ok(LoaderKind::NeoForge),
        _ => anyhow::bail!("{}", t!("cli.error.unknown_loader", loader = s)),
    }
}

fn parse_side(s: &str) -> Result<Side> {
    match s.to_lowercase().as_str() {
        "client" => Ok(Side::Client),
        "server" => Ok(Side::Server),
        "both" => Ok(Side::Both),
        _ => anyhow::bail!("{}", t!("cli.error.unknown_side", side = s)),
    }
}

fn load_config() -> Result<PackConfig> {
    let content = std::fs::read_to_string(CONFIG_FILE)
        .with_context(|| t!("cli.error.load_config", file = CONFIG_FILE))?;
    toml::from_str(&content).with_context(|| t!("cli.error.parse_config", file = CONFIG_FILE))
}

fn save_config(config: &PackConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context(t!("cli.error.serialize_config"))?;
    std::fs::write(CONFIG_FILE, content)
        .with_context(|| t!("cli.error.save_config", file = CONFIG_FILE))
}

fn load_lockfile() -> Result<Option<mcpm_core::Lockfile>> {
    if !PathBuf::from(LOCK_FILE).exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(LOCK_FILE)
        .with_context(|| t!("cli.error.load_config", file = LOCK_FILE))?;
    let lockfile: mcpm_core::Lockfile = serde_json::from_str(&content)
        .with_context(|| t!("cli.error.parse_config", file = LOCK_FILE))?;
    Ok(Some(lockfile))
}

fn save_lockfile(lockfile: &mcpm_core::Lockfile) -> Result<()> {
    let content =
        serde_json::to_string_pretty(lockfile).context(t!("cli.error.serialize_config"))?;
    std::fs::write(LOCK_FILE, content)
        .with_context(|| t!("cli.error.save_config", file = LOCK_FILE))
}

async fn resolve_and_install(config: &PackConfig) -> Result<mcpm_core::Lockfile> {
    let registry = ModrinthClient::new();
    let game_version = config.environment.minecraft.clone();
    let loader = config
        .environment
        .loader
        .as_fixed()
        .ok_or_else(|| anyhow::anyhow!("{}", t!("cli.error.loader_not_fixed")))?;

    println!(
        "{}",
        t!(
            "cli.install.resolving",
            mc = &game_version,
            loader = loader.to_string()
        )
    );

    let plan = mcpm_resolver::resolve_with_compat(
        &registry,
        &config.mods,
        &config.environment,
        &config.preferences,
        config.compatibility.as_ref(),
    )
    .await?;

    for conflict in &plan.conflicts {
        eprintln!(
            "{}",
            t!("cli.resolve.conflict", message = &conflict.message)
        );
    }
    for warning in &plan.warnings {
        println!("{}", t!("cli.resolve.warning", message = &warning.message));
    }
    for workaround in &plan.workarounds {
        println!("{}", t!("cli.resolve.workaround", message = workaround));
    }

    let mcpm_version = env!("CARGO_PKG_VERSION");
    let lockfile = mcpm_resolver::plan_to_lockfile(&plan, &config.environment, mcpm_version)?;

    println!(
        "\n{}",
        t!(
            "cli.install.resolved",
            count = lockfile.mods.len().to_string()
        )
    );
    for m in &lockfile.mods {
        let reason = match m.reason {
            LockReason::Requested => t!("cli.resolve.reason_requested"),
            LockReason::Dependency => t!("cli.resolve.reason_dependency"),
        };
        println!(
            "  {}",
            t!(
                "cli.install.mod_line",
                name = &m.name,
                version = &m.version_number,
                reason = &reason
            )
        );
    }

    save_lockfile(&lockfile)?;
    println!("\n{}", t!("cli.install.lockfile_written", path = LOCK_FILE));

    run_installer(&lockfile).await?;

    Ok(lockfile)
}

async fn run_installer(lockfile: &mcpm_core::Lockfile) -> Result<()> {
    let installer = Installer::new(std::path::Path::new("."))?;
    let result = installer.install(lockfile).await?;

    for f in &result.installed {
        println!("{}", t!("cli.install.installed", filename = f));
    }
    for (f, e) in &result.errors {
        eprintln!(
            "{}",
            t!(
                "cli.install.install_error",
                filename = f,
                error = e.to_string()
            )
        );
    }

    if result.errors.is_empty() {
        println!(
            "{}",
            t!(
                "cli.install.complete",
                installed = result.installed.len().to_string(),
                skipped = result.skipped.len().to_string()
            )
        );
        Ok(())
    } else {
        anyhow::bail!(
            "{}",
            t!(
                "cli.install.with_errors",
                count = result.errors.len().to_string()
            )
        )
    }
}

async fn cmd_init(
    minecraft: String,
    loader: String,
    name: Option<String>,
    side: String,
) -> Result<()> {
    let loader_kind = parse_loader(&loader)?;
    let side_kind = parse_side(&side)?;

    let pack_name = name.unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "my-pack".to_string())
    });

    let config = PackConfig {
        pack: PackMeta {
            name: pack_name.clone(),
            author: None,
            description: None,
        },
        environment: Environment {
            minecraft: minecraft.clone(),
            minecraft_range: None,
            loader: LoaderSpec::Fixed(loader_kind),
            loader_candidates: None,
            loader_version: None,
            side: side_kind,
        },
        preferences: Preferences::default(),
        mods: indexmap::IndexMap::new(),
        compatibility: None,
    };

    save_config(&config)?;

    if !PathBuf::from("mods").exists() {
        std::fs::create_dir("mods").context(t!("cli.error.create_mods_dir"))?;
    }
    if !PathBuf::from(".mcpm").exists() {
        std::fs::create_dir(".mcpm").context(t!("cli.error.create_mcpm_dir"))?;
    }

    println!(
        "{}",
        t!(
            "cli.init.success",
            name = &pack_name,
            mc = &minecraft,
            loader = loader_kind.to_string()
        )
    );
    Ok(())
}

async fn cmd_add(slugs: Vec<String>) -> Result<()> {
    let mut config = load_config()?;

    let loader = config
        .environment
        .loader
        .as_fixed()
        .ok_or_else(|| anyhow::anyhow!("{}", t!("cli.error.loader_not_fixed")))?;
    let game_version = config.environment.minecraft.clone();
    let registry = ModrinthClient::new();

    let mut added: Vec<(String, String)> = Vec::new();

    for slug in &slugs {
        if config.mods.contains_key(slug) {
            anyhow::bail!("{}", t!("cli.add.already_exists", slug = slug));
        }

        println!("{}", t!("cli.add.searching", slug = slug));
        let results = registry.search_projects(slug).await?;

        if results.is_empty() {
            anyhow::bail!("{}", t!("cli.add.no_results", slug = slug));
        }

        let best = &results[0];
        let project = registry.get_project(&best.id).await?;

        println!(
            "{}",
            t!("cli.add.found", title = &project.title, id = &project.id)
        );

        let versions = registry.get_versions(&project.id).await?;
        let compatible: Vec<_> = versions
            .iter()
            .filter(|v| {
                version_matches_environment(v, &game_version, loader)
                    && version_matches_preferences(v, &config.preferences)
            })
            .collect();

        if compatible.is_empty() {
            let all_versions = versions.len();
            anyhow::bail!(
                "{}",
                t!(
                    "cli.add.no_compatible",
                    title = &project.title,
                    mc = game_version,
                    loader = loader.to_string(),
                    count = all_versions.to_string()
                )
            );
        }

        let best_version = mcpm_resolver::select_best_version(
            &versions,
            &game_version,
            loader,
            &config.preferences,
            None,
        )
        .unwrap_or(compatible[0]);

        println!(
            "{}",
            t!(
                "cli.add.compatible",
                version = &best_version.version_number,
                release_type = best_version.version_type.to_string()
            )
        );

        let entry = ModEntry::Detailed(DetailedModEntry {
            version: Some("latest".to_string()),
            source: Some(mcpm_core::SourceKind::Modrinth),
            side: None,
            project_id: Some(project.id.clone()),
        });

        let display_name = project.slug.as_deref().unwrap_or(slug).to_string();
        config.mods.insert(display_name.clone(), entry);
        println!("{}", t!("cli.add.added", title = &project.title));
        added.push((display_name, project.id.clone()));
    }

    save_config(&config)?;

    let existing_lockfile = load_lockfile()?;

    if let Some(mut lockfile) = existing_lockfile {
        let mcpm_version = env!("CARGO_PKG_VERSION");
        let new_entries = {
            let mut temp_config = config.clone();
            temp_config.mods.clear();
            for (display_name, project_id) in &added {
                temp_config.mods.insert(
                    display_name.clone(),
                    ModEntry::Detailed(DetailedModEntry {
                        version: Some("latest".to_string()),
                        source: Some(mcpm_core::SourceKind::Modrinth),
                        side: None,
                        project_id: Some(project_id.clone()),
                    }),
                );
            }

            let plan = mcpm_resolver::resolve_with_compat(
                &registry,
                &temp_config.mods,
                &temp_config.environment,
                &temp_config.preferences,
                temp_config.compatibility.as_ref(),
            )
            .await?;

            for conflict in &plan.conflicts {
                eprintln!(
                    "{}",
                    t!("cli.resolve.conflict", message = &conflict.message)
                );
            }
            for warning in &plan.warnings {
                println!("{}", t!("cli.resolve.warning", message = &warning.message));
            }
            for workaround in &plan.workarounds {
                println!("{}", t!("cli.resolve.workaround", message = workaround));
            }

            let temp_lock =
                mcpm_resolver::plan_to_lockfile(&plan, &config.environment, mcpm_version)?;

            let existing_ids: HashSet<String> =
                lockfile.mods.iter().map(|m| m.project_id.clone()).collect();
            let mut new = Vec::new();
            for m in &temp_lock.mods {
                if !existing_ids.contains(&m.project_id) {
                    new.push(m.clone());
                    lockfile.mods.push(m.clone());
                }
            }
            new
        };

        save_lockfile(&lockfile)?;

        if !new_entries.is_empty() {
            for m in &new_entries {
                let reason = match m.reason {
                    LockReason::Requested => t!("cli.resolve.reason_requested"),
                    LockReason::Dependency => t!("cli.resolve.reason_dependency"),
                };
                println!(
                    "  {}",
                    t!(
                        "cli.install.mod_line",
                        name = &m.name,
                        version = &m.version_number,
                        reason = &reason
                    )
                );
            }

            let install_lock = mcpm_core::Lockfile {
                lockfile_version: lockfile.lockfile_version,
                mcpm_version: lockfile.mcpm_version.clone(),
                environment: lockfile.environment.clone(),
                mods: new_entries,
            };

            run_installer(&install_lock).await?;
        } else {
            println!("{}", t!("cli.add.all_in_lockfile"));
        }
    } else {
        resolve_and_install(&config).await?;
    }

    Ok(())
}

async fn cmd_remove(slug: String) -> Result<()> {
    let mut config = load_config()?;
    let old_lockfile = load_lockfile()?;

    if !config.mods.contains_key(&slug) {
        anyhow::bail!("{}", t!("cli.remove.not_found", slug = &slug));
    }

    config.mods.swap_remove(&slug);
    save_config(&config)?;
    println!("{}", t!("cli.remove.removed_config", slug = &slug));

    let new_lockfile = resolve_and_install(&config).await?;

    if let Some(old) = old_lockfile {
        let old_filenames: Vec<&str> = old.mods.iter().map(|m| m.filename.as_str()).collect();
        let new_filenames: Vec<&str> = new_lockfile
            .mods
            .iter()
            .map(|m| m.filename.as_str())
            .collect();

        for old_f in &old_filenames {
            if !new_filenames.contains(old_f) {
                let path = PathBuf::from("mods").join(old_f);
                if path.exists() {
                    std::fs::remove_file(&path)?;
                    println!("{}", t!("cli.remove.removed_file", filename = old_f));
                }
            }
        }
    }

    Ok(())
}

async fn cmd_autoremove() -> Result<()> {
    let mut config = load_config()?;
    let old_lockfile =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.check.no_lockfile")))?;

    let mut needed: HashSet<&str> = HashSet::new();
    for m in &old_lockfile.mods {
        if m.reason == LockReason::Requested {
            needed.insert(m.project_id.as_str());
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for m in &old_lockfile.mods {
            if needed.contains(m.project_id.as_str()) {
                for dep_id in &m.dependencies {
                    if needed.insert(dep_id.as_str()) {
                        changed = true;
                    }
                }
            }
        }
    }

    let orphaned: Vec<&mcpm_core::LockedMod> = old_lockfile
        .mods
        .iter()
        .filter(|m| m.reason == LockReason::Dependency && !needed.contains(m.project_id.as_str()))
        .collect();

    if orphaned.is_empty() {
        println!("{}", t!("cli.autoremove.none"));
        return Ok(());
    }

    println!(
        "{}",
        t!("cli.autoremove.found", count = orphaned.len().to_string())
    );
    for m in &orphaned {
        println!(
            "{}",
            t!(
                "cli.autoremove.dep_line",
                name = &m.name,
                version = &m.version_number
            )
        );
    }

    for m in &orphaned {
        let mut removed = false;
        if let Some(slug) = &m.slug {
            if config.mods.contains_key(slug) {
                config.mods.swap_remove(slug);
                println!("{}", t!("cli.autoremove.removed_config", slug = slug));
                removed = true;
            }
        }
        if !removed {
            let keys: Vec<String> = config
                .mods
                .iter()
                .filter(|(_, entry)| entry.project_id() == Some(&m.project_id))
                .map(|(k, _)| k.clone())
                .collect();
            for key in keys {
                config.mods.swap_remove(&key);
                println!("{}", t!("cli.autoremove.removed_config", slug = &key));
            }
        }
    }

    save_config(&config)?;

    let new_lockfile = resolve_and_install(&config).await?;

    let old_filenames: Vec<&str> = old_lockfile
        .mods
        .iter()
        .map(|m| m.filename.as_str())
        .collect();
    let new_filenames: Vec<&str> = new_lockfile
        .mods
        .iter()
        .map(|m| m.filename.as_str())
        .collect();

    for old_f in &old_filenames {
        if !new_filenames.contains(old_f) {
            let path = PathBuf::from("mods").join(old_f);
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("{}", t!("cli.remove.removed_file", filename = old_f));
            }
        }
    }

    Ok(())
}

async fn cmd_install() -> Result<()> {
    let lockfile =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.install.no_lockfile")))?;
    run_installer(&lockfile).await?;
    Ok(())
}

async fn cmd_list() -> Result<()> {
    let lockfile =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.list.no_lockfile")))?;

    println!(
        "{:<30} {:<15} {:<10} {:<10}",
        t!("cli.list.header_name"),
        t!("cli.list.header_version"),
        t!("cli.list.header_source"),
        t!("cli.list.header_reason"),
    );
    println!("{}", "-".repeat(65));
    for m in &lockfile.mods {
        let reason = match m.reason {
            LockReason::Requested => t!("cli.resolve.reason_requested"),
            LockReason::Dependency => t!("cli.resolve.reason_dependency"),
        };
        let source_str = match m.source {
            mcpm_core::SourceKind::Modrinth => t!("cli.common.source_modrinth"),
            _ => format!("{}", m.source).into(),
        };
        println!(
            "{:<30} {:<15} {:<10} {:<10}",
            m.name, m.version_number, source_str, reason
        );
    }
    println!(
        "\n{}",
        t!("cli.list.total", count = lockfile.mods.len().to_string())
    );

    Ok(())
}

async fn cmd_check() -> Result<()> {
    let _config = load_config()?;
    let lockfile =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.check.no_lockfile")))?;

    let game_version = &lockfile.environment.minecraft;
    let loader = lockfile.environment.loader;

    println!(
        "{}",
        t!(
            "cli.check.env_line",
            mc = game_version,
            loader = loader.to_string()
        )
    );
    println!(
        "{}",
        t!(
            "cli.check.mod_count",
            count = lockfile.mods.len().to_string()
        )
    );

    for m in &lockfile.mods {
        println!(
            "{}",
            t!(
                "cli.check.mod_line",
                name = &m.name,
                version = &m.version_number
            )
        );
    }

    check_installed_jars(&lockfile);

    println!("{}", t!("cli.check.complete"));
    Ok(())
}

fn check_installed_jars(lockfile: &mcpm_core::Lockfile) {
    let report = mcpm_metadata::verify_dir(
        std::path::Path::new("mods"),
        lockfile.environment.side,
        lockfile.environment.loader,
    );

    println!(
        "{}",
        t!(
            "cli.check.jar_summary",
            mods = report.mods.len().to_string()
        )
    );

    for (filename, error) in &report.parse_errors {
        eprintln!(
            "{}",
            t!("cli.check.parse_error", filename = filename, error = error)
        );
    }

    for id in &report.duplicate_ids {
        eprintln!("{}", t!("cli.check.duplicate_id", id = id));
    }

    for conflict in &report.side_conflicts {
        eprintln!(
            "{}",
            t!(
                "cli.check.side_conflict",
                id = &conflict.id,
                mod_side = conflict.mod_side.to_string(),
                expected = conflict.expected.to_string(),
                filename = &conflict.filename
            )
        );
    }

    for mismatch in &report.loader_mismatches {
        eprintln!(
            "{}",
            t!(
                "cli.check.loader_mismatch",
                id = &mismatch.id,
                actual = mismatch.actual.to_string(),
                expected = mismatch.expected.to_string(),
                filename = &mismatch.filename
            )
        );
    }

    for dep in &report.missing_dependencies {
        eprintln!(
            "{}",
            t!(
                "cli.check.missing_dependency",
                id = &dep.id,
                missing = &dep.missing,
                filename = &dep.filename
            )
        );
    }

    for requirement in &report.loader_version_requirements {
        println!(
            "{}",
            t!("cli.check.loader_requirement", requirement = requirement)
        );
    }
}

fn explore_loaders(config: &PackConfig) -> Vec<LoaderKind> {
    match &config.environment.loader {
        LoaderSpec::Candidates(loaders) => loaders.clone(),
        LoaderSpec::Fixed(loader) => vec![*loader],
    }
}

async fn compute_candidates(
    config: &PackConfig,
) -> Result<Vec<mcpm_resolver::CandidateEvaluation>> {
    let registry = ModrinthClient::new();
    let range = VersionRange::parse(&config.environment.minecraft).ok();
    let loaders = explore_loaders(config);
    let evaluations = mcpm_resolver::solve_env(
        &registry,
        &config.mods,
        range.as_ref(),
        &loaders,
        config.environment.side,
        &config.preferences,
        config.compatibility.as_ref(),
    )
    .await?;
    Ok(evaluations)
}

async fn apply_candidate(
    config: &mut PackConfig,
    index: usize,
    evaluations: &[mcpm_resolver::CandidateEvaluation],
) -> Result<()> {
    if index == 0 || index > evaluations.len() {
        anyhow::bail!(
            "{}",
            t!(
                "cli.use.invalid_index",
                index = index.to_string(),
                max = evaluations.len().to_string()
            )
        );
    }

    let chosen = &evaluations[index - 1];
    let explore_range = config.environment.minecraft.clone();
    let explore_loaders = explore_loaders(config);

    config.environment.minecraft = chosen.candidate.minecraft.clone();
    config.environment.minecraft_range = Some(explore_range);
    config.environment.loader = LoaderSpec::Fixed(chosen.candidate.loader);
    config.environment.loader_candidates = Some(explore_loaders);
    config.environment.loader_version = chosen.candidate.loader_version.clone();
    config.preferences.environment_strategy = None;
    save_config(config)?;

    resolve_and_install(config).await?;

    println!(
        "{}",
        t!(
            "cli.use.selected",
            mc = &chosen.candidate.minecraft,
            loader = chosen.candidate.loader.to_string()
        )
    );
    Ok(())
}

async fn cmd_solve_env() -> Result<()> {
    let config = load_config()?;
    let evaluations = compute_candidates(&config).await?;

    if evaluations.is_empty() {
        println!("{}", t!("cli.solve_env.none"));
        return Ok(());
    }

    println!("{}", t!("cli.solve_env.title"));
    for (i, e) in evaluations.iter().enumerate() {
        println!(
            "{}",
            t!(
                "cli.solve_env.candidate",
                index = (i + 1).to_string(),
                mc = &e.candidate.minecraft,
                loader = e.candidate.loader.to_string()
            )
        );
        println!(
            "{}",
            t!(
                "cli.solve_env.requested",
                available = e.requested_available.to_string(),
                total = e.requested_total.to_string()
            )
        );
        if !e.missing.is_empty() {
            println!(
                "{}",
                t!("cli.solve_env.missing", mods = e.missing.join(", "))
            );
        }
        println!(
            "{}",
            t!(
                "cli.solve_env.dependencies",
                count = e.dependency_count.to_string()
            )
        );
        println!(
            "{}",
            t!(
                "cli.solve_env.risks",
                warnings = e.warnings.len().to_string(),
                conflicts = e.conflicts.len().to_string()
            )
        );
        println!("{}", t!("cli.solve_env.score", score = e.score.to_string()));
        println!();
    }

    Ok(())
}

async fn cmd_use(candidate: usize) -> Result<()> {
    let mut config = load_config()?;
    let evaluations = compute_candidates(&config).await?;
    if evaluations.is_empty() {
        anyhow::bail!("{}", t!("cli.solve_env.none"));
    }
    apply_candidate(&mut config, candidate, &evaluations).await?;
    Ok(())
}

async fn cmd_lock_env(choose: Option<usize>) -> Result<()> {
    let mut config = load_config()?;
    let evaluations = compute_candidates(&config).await?;
    if evaluations.is_empty() {
        anyhow::bail!("{}", t!("cli.solve_env.none"));
    }

    let index = match choose {
        Some(index) => index,
        None if evaluations.len() == 1 => 1,
        None => anyhow::bail!("{}", t!("cli.lock_env.multiple")),
    };

    apply_candidate(&mut config, index, &evaluations).await?;
    Ok(())
}

async fn cmd_unlock_env() -> Result<()> {
    let mut config = load_config()?;

    let range = config
        .environment
        .minecraft_range
        .take()
        .ok_or_else(|| anyhow::anyhow!("{}", t!("cli.un_lock_env.not_locked")))?;
    let loaders = config.environment.loader_candidates.take();

    config.environment.minecraft = range;
    if let Some(loaders) = loaders {
        config.environment.loader = LoaderSpec::Candidates(loaders);
    }
    config.environment.loader_version = None;
    config.preferences.environment_strategy = Some(EnvironmentStrategy::MostCompatible);
    save_config(&config)?;

    if PathBuf::from(LOCK_FILE).exists() {
        std::fs::remove_file(LOCK_FILE)
            .with_context(|| t!("cli.error.save_config", file = LOCK_FILE))?;
    }

    println!(
        "{}",
        t!(
            "cli.un_lock_env.unlocked",
            minecraft = &config.environment.minecraft
        )
    );
    Ok(())
}

fn build_cli() -> Command {
    Cli::command()
        .about(t!("cli.about").to_string())
        .mut_subcommand("init", |c| c.about(t!("cli.init.help").to_string()))
        .mut_subcommand("add", |c| c.about(t!("cli.add.help").to_string()))
        .mut_subcommand("remove", |c| c.about(t!("cli.remove.help").to_string()))
        .mut_subcommand("autoremove", |c| {
            c.about(t!("cli.autoremove.help").to_string())
        })
        .mut_subcommand("install", |c| c.about(t!("cli.install.help").to_string()))
        .mut_subcommand("list", |c| c.about(t!("cli.list.help").to_string()))
        .mut_subcommand("check", |c| c.about(t!("cli.check.help").to_string()))
        .mut_subcommand("solve-env", |c| {
            c.about(t!("cli.solve_env.help").to_string())
        })
        .mut_subcommand("use", |c| c.about(t!("cli.use.help").to_string()))
        .mut_subcommand("lock-env", |c| c.about(t!("cli.lock_env.help").to_string()))
        .mut_subcommand("un-lock-env", |c| {
            c.about(t!("cli.un_lock_env.help").to_string())
        })
}

#[tokio::main]
async fn main() -> Result<()> {
    i18n::init_locale();
    setup_tracing();

    let matches = build_cli().try_get_matches().unwrap_or_else(|e| e.exit());
    let cli = Cli::from_arg_matches(&matches).map_err(|e| anyhow::anyhow!("{}", e))?;

    let result = match cli.command {
        None => {
            let mut cmd = build_cli();
            let _ = cmd.print_help();
            Ok(())
        }
        Some(cmd) => match cmd {
            Commands::Init {
                minecraft,
                loader,
                name,
                side,
            } => cmd_init(minecraft, loader, name, side).await,
            Commands::Add { slugs } => cmd_add(slugs).await,
            Commands::Remove { slug } => cmd_remove(slug).await,
            Commands::Autoremove => cmd_autoremove().await,
            Commands::Install => cmd_install().await,
            Commands::List => cmd_list().await,
            Commands::Check => cmd_check().await,
            Commands::SolveEnv => cmd_solve_env().await,
            Commands::Use { candidate } => cmd_use(candidate).await,
            Commands::LockEnv { choose } => cmd_lock_env(choose).await,
            Commands::UnLockEnv => cmd_unlock_env().await,
        },
    };

    if let Err(ref e) = result {
        eprintln!("{}: {:#}", t!("cli.common.error_prefix"), e);
        std::process::exit(1);
    }

    result
}
