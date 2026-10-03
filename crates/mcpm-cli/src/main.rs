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
use mcpm_registry::{CompositeRegistry, CurseForgeClient, ModrinthClient};

fn build_registry() -> CompositeRegistry {
    let curseforge = mcpm_core::userconfig::curseforge_api_key().map(CurseForgeClient::new);
    CompositeRegistry::new(ModrinthClient::new(), curseforge)
}

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

    /// Update mods safely within the current environment
    Update {
        /// Also refresh the loader version
        #[arg(long)]
        allow_loader: bool,
    },

    /// Upgrade the Minecraft version (explicit, plan-based)
    UpgradeEnv {
        /// Target Minecraft version (e.g. 1.21.1)
        #[arg(long)]
        target_minecraft: String,
    },

    /// Migrate to another loader type (explicit, plan-based)
    MigrateLoader {
        /// Target loader (fabric, quilt, forge, neoforge)
        #[arg(long)]
        to: String,
    },

    /// Apply the last generated update plan
    ApplyPlan,

    /// Import existing content into the modpack
    Import {
        #[command(subcommand)]
        target: ImportTarget,
    },
}

#[derive(Subcommand)]
pub enum ImportTarget {
    /// Import mods from the existing mods/ directory
    Mods {
        /// Preview changes without writing to mcpm.toml
        #[arg(long)]
        dry_run: bool,

        /// Overwrite entries that already exist in mcpm.toml
        #[arg(long)]
        overwrite: bool,
    },
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
    let registry = build_registry();
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
    let registry = build_registry();

    let mut added: Vec<(String, String, mcpm_core::SourceKind, Option<String>)> = Vec::new();

    for slug in &slugs {
        if config.mods.contains_key(slug) {
            anyhow::bail!("{}", t!("cli.add.already_exists", slug = slug));
        }

        println!("{}", t!("cli.add.searching", slug = slug));
        let results = registry.search_projects(slug).await?;

        let best = results
            .iter()
            .find(|p| config.preferences.source.contains(&p.source));

        let best = match best {
            Some(best) => best,
            None => anyhow::bail!("{}", t!("cli.add.no_results", slug = slug)),
        };
        let source = best.source;
        let project = registry.get_project(source, &best.id).await?;

        println!(
            "{}",
            t!("cli.add.found", title = &project.title, id = &project.id)
        );

        let versions = registry.get_versions(source, &project.id).await?;
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

        let file_id = if source == mcpm_core::SourceKind::CurseForge {
            Some(best_version.id.clone())
        } else {
            None
        };

        let entry = ModEntry::Detailed(DetailedModEntry {
            version: Some(if source == mcpm_core::SourceKind::CurseForge {
                best_version.version_number.clone()
            } else {
                "latest".to_string()
            }),
            file_id: file_id.clone(),
            source: Some(source),
            side: None,
            project_id: Some(project.id.clone()),
        });

        let display_name = project.slug.as_deref().unwrap_or(slug).to_string();
        config.mods.insert(display_name.clone(), entry);
        println!("{}", t!("cli.add.added", title = &project.title));
        added.push((display_name, project.id.clone(), source, file_id));
    }

    save_config(&config)?;

    let existing_lockfile = load_lockfile()?;

    if let Some(mut lockfile) = existing_lockfile {
        let mcpm_version = env!("CARGO_PKG_VERSION");
        let new_entries = {
            let mut temp_config = config.clone();
            temp_config.mods.clear();
            for (display_name, project_id, source, file_id) in &added {
                temp_config.mods.insert(
                    display_name.clone(),
                    ModEntry::Detailed(DetailedModEntry {
                        version: Some("latest".to_string()),
                        file_id: file_id.clone(),
                        source: Some(*source),
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
    let registry = build_registry();
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

const PLAN_FILE: &str = ".mcpm/plans/plan.json";

fn save_plan(plan: &mcpm_core::UpdatePlan) -> Result<()> {
    let path = PathBuf::from(PLAN_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| t!("cli.error.save_config", file = PLAN_FILE))?;
    }
    let json = serde_json::to_string_pretty(plan).context(t!("cli.error.serialize_config"))?;
    std::fs::write(&path, json).with_context(|| t!("cli.error.save_config", file = PLAN_FILE))?;
    Ok(())
}

fn load_plan() -> Result<mcpm_core::UpdatePlan> {
    let content = std::fs::read_to_string(PLAN_FILE)
        .with_context(|| t!("cli.error.load_config", file = PLAN_FILE))?;
    serde_json::from_str(&content).with_context(|| t!("cli.error.parse_config", file = PLAN_FILE))
}

fn print_plan(plan: &mcpm_core::UpdatePlan) {
    println!("{}", t!("cli.plan.header"));
    for action in &plan.actions {
        match action {
            mcpm_core::PlanAction::Install(m) => println!(
                "{}",
                t!(
                    "cli.plan.install",
                    name = &m.name,
                    version = &m.version_number
                )
            ),
            mcpm_core::PlanAction::Update { from, to } => println!(
                "{}",
                t!(
                    "cli.plan.update",
                    name = &to.name,
                    from = &from.version_number,
                    to = &to.version_number
                )
            ),
            mcpm_core::PlanAction::Remove(m) => {
                println!("{}", t!("cli.plan.remove", name = &m.name))
            }
            mcpm_core::PlanAction::Keep(_) => {}
        }
    }

    for conflict in &plan.conflicts {
        eprintln!(
            "{}",
            t!("cli.resolve.conflict", message = &conflict.message)
        );
    }
    for warning in &plan.warnings {
        println!("{}", t!("cli.resolve.warning", message = &warning.message));
    }
}

async fn apply_update_plan(plan: &mcpm_core::UpdatePlan) -> Result<()> {
    if !plan.success {
        anyhow::bail!("{}", t!("cli.plan.conflicts_block"));
    }

    let lockfile = plan.to_lockfile(env!("CARGO_PKG_VERSION"))?;
    save_lockfile(&lockfile)?;
    run_installer(&lockfile).await?;

    for m in plan.removals() {
        let path = PathBuf::from("mods").join(&m.filename);
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("{}", t!("cli.remove.removed_file", filename = &m.filename));
        }
    }

    println!("{}", t!("cli.plan.applied"));
    Ok(())
}

async fn cmd_update(allow_loader: bool) -> Result<()> {
    let config = load_config()?;
    let old =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.update.no_lockfile")))?;
    let registry = build_registry();
    let mode = if allow_loader {
        mcpm_resolver::UpdateMode::AllowLoader
    } else {
        mcpm_resolver::UpdateMode::Safe
    };

    let plan = mcpm_resolver::plan_update(
        &registry,
        &config.mods,
        &config.preferences,
        config.compatibility.as_ref(),
        &old,
        config.environment.clone(),
        mode,
        Some(registry.modrinth() as &dyn mcpm_core::LoaderVersionRegistry),
        std::path::Path::new("mods"),
    )
    .await?;

    print_plan(&plan);

    if !plan.has_changes() {
        println!("{}", t!("cli.plan.no_changes"));
        return Ok(());
    }

    save_plan(&plan)?;
    apply_update_plan(&plan).await
}

async fn cmd_upgrade_env(target_minecraft: String) -> Result<()> {
    let mut config = load_config()?;
    let old =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.update.no_lockfile")))?;
    let registry = build_registry();

    let mut target = config.environment.clone();
    target.minecraft = target_minecraft.clone();

    let plan = mcpm_resolver::plan_update(
        &registry,
        &config.mods,
        &config.preferences,
        config.compatibility.as_ref(),
        &old,
        target,
        mcpm_resolver::UpdateMode::Explicit,
        Some(registry.modrinth() as &dyn mcpm_core::LoaderVersionRegistry),
        std::path::Path::new("mods"),
    )
    .await?;

    print_plan(&plan);
    save_plan(&plan)?;
    apply_update_plan(&plan).await?;

    config.environment.minecraft = target_minecraft.clone();
    config.environment.loader_version = plan.environment.loader_version.clone();
    save_config(&config)?;
    println!("{}", t!("cli.upgrade_env.upgraded", mc = &target_minecraft));
    Ok(())
}

async fn cmd_migrate_loader(to: String) -> Result<()> {
    let target_loader = parse_loader(&to)?;
    let mut config = load_config()?;
    let old =
        load_lockfile()?.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.update.no_lockfile")))?;
    let registry = build_registry();

    let mut target = config.environment.clone();
    target.loader = LoaderSpec::Fixed(target_loader);
    target.loader_version = None;

    let plan = mcpm_resolver::plan_update(
        &registry,
        &config.mods,
        &config.preferences,
        config.compatibility.as_ref(),
        &old,
        target,
        mcpm_resolver::UpdateMode::Explicit,
        Some(registry.modrinth() as &dyn mcpm_core::LoaderVersionRegistry),
        std::path::Path::new("mods"),
    )
    .await?;

    print_plan(&plan);
    save_plan(&plan)?;
    apply_update_plan(&plan).await?;

    config.environment.loader = LoaderSpec::Fixed(target_loader);
    config.environment.loader_version = plan.environment.loader_version.clone();
    save_config(&config)?;
    println!(
        "{}",
        t!(
            "cli.migrate_loader.migrated",
            loader = target_loader.to_string()
        )
    );
    Ok(())
}

async fn cmd_apply_plan() -> Result<()> {
    let plan = load_plan()?;
    print_plan(&plan);
    apply_update_plan(&plan).await
}

struct ImportCandidate {
    key: String,
    entry: ModEntry,
    is_local: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum MergeOutcome {
    Added,
    Overwritten,
    Skipped,
}

fn merge_entry(
    config: &mut PackConfig,
    candidate: &ImportCandidate,
    overwrite: bool,
) -> MergeOutcome {
    let exists = config.mods.contains_key(&candidate.key);
    if exists && !overwrite {
        MergeOutcome::Skipped
    } else {
        config
            .mods
            .insert(candidate.key.clone(), candidate.entry.clone());
        if exists {
            MergeOutcome::Overwritten
        } else {
            MergeOutcome::Added
        }
    }
}

async fn lookup_mod(
    registry: &ModrinthClient,
    path: &std::path::Path,
    metadata: &mcpm_metadata::ModMetadata,
) -> Result<ImportCandidate, anyhow::Error> {
    let sha512 = mcpm_core::hash::sha512_hex(path).ok();
    let sha1 = mcpm_core::hash::sha1_hex(path).ok();

    let mut found: Option<(String, String, String)> = None;
    if let Some(ref hash) = sha512 {
        if let Some(version) = registry.get_version_by_hash(hash, "sha512").await? {
            if let Ok(project) = registry
                .get_project(mcpm_core::SourceKind::Modrinth, &version.project_id)
                .await
            {
                found = Some((
                    project.id,
                    project.slug.unwrap_or(metadata.id.clone()),
                    version.version_number,
                ));
            }
        }
    }
    if found.is_none() {
        if let Some(ref hash) = sha1 {
            if let Some(version) = registry.get_version_by_hash(hash, "sha1").await? {
                if let Ok(project) = registry
                    .get_project(mcpm_core::SourceKind::Modrinth, &version.project_id)
                    .await
                {
                    found = Some((
                        project.id,
                        project.slug.unwrap_or(metadata.id.clone()),
                        version.version_number,
                    ));
                }
            }
        }
    }

    if found.is_none() {
        if let Ok(project) = registry
            .get_project(mcpm_core::SourceKind::Modrinth, &metadata.id)
            .await
        {
            found = Some((
                project.id,
                project.slug.unwrap_or(metadata.id.clone()),
                metadata
                    .version
                    .clone()
                    .unwrap_or_else(|| "latest".to_string()),
            ));
        }
    }

    if found.is_none() {
        let query = metadata.name.as_deref().unwrap_or(&metadata.id);
        let results = registry.search_projects(query).await.unwrap_or_default();
        let matched = results
            .iter()
            .find(|p| p.slug.as_deref() == Some(metadata.id.as_str()))
            .or_else(|| results.first());
        if let Some(project) = matched {
            found = Some((
                project.id.clone(),
                project.slug.clone().unwrap_or(metadata.id.clone()),
                metadata
                    .version
                    .clone()
                    .unwrap_or_else(|| "latest".to_string()),
            ));
        }
    }

    let side = match metadata.environment {
        Side::Client => Some(Side::Client),
        Side::Server => Some(Side::Server),
        Side::Both => None,
    };

    match found {
        Some((project_id, key, version)) => Ok(ImportCandidate {
            key,
            entry: ModEntry::Detailed(DetailedModEntry {
                version: Some(version),
                file_id: None,
                source: Some(mcpm_core::SourceKind::Modrinth),
                side,
                project_id: Some(project_id),
            }),
            is_local: false,
        }),
        None => Ok(ImportCandidate {
            key: metadata.id.clone(),
            entry: ModEntry::Detailed(DetailedModEntry {
                version: metadata.version.clone(),
                file_id: None,
                source: Some(mcpm_core::SourceKind::Local),
                side,
                project_id: None,
            }),
            is_local: true,
        }),
    }
}

async fn cmd_import_mods(dry_run: bool, overwrite: bool) -> Result<()> {
    let mut config = load_config()?;
    let registry = ModrinthClient::new();

    let entries = std::fs::read_dir("mods")
        .with_context(|| t!("cli.import.no_mods_dir"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jar"))
        .collect::<Vec<_>>();

    if entries.is_empty() {
        println!("{}", t!("cli.import.empty"));
        return Ok(());
    }

    println!(
        "{}",
        t!("cli.import.scanning", count = entries.len().to_string())
    );

    let mut added = 0;
    let mut skipped = 0;
    let mut local_count = 0;
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut working = config.clone();

    for path in &entries {
        let metadatas = match mcpm_metadata::parse_jar(path) {
            Ok(metadatas) => metadatas,
            Err(e) => {
                let filename = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                eprintln!(
                    "{}",
                    t!(
                        "cli.import.parse_error",
                        filename = filename,
                        error = e.to_string()
                    )
                );
                continue;
            }
        };

        for metadata in metadatas {
            if !seen_ids.insert(metadata.id.clone()) {
                eprintln!("{}", t!("cli.import.duplicate_id", id = &metadata.id));
                continue;
            }

            let candidate = lookup_mod(&registry, path, &metadata).await?;

            if candidate.is_local {
                local_count += 1;
                eprintln!("{}", t!("cli.import.local_mod", id = &candidate.key));
            }

            match merge_entry(&mut working, &candidate, overwrite) {
                MergeOutcome::Skipped => {
                    skipped += 1;
                    println!("{}", t!("cli.import.skip_existing", key = &candidate.key));
                }
                MergeOutcome::Added => {
                    added += 1;
                    println!("{}", t!("cli.import.add_entry", key = &candidate.key));
                }
                MergeOutcome::Overwritten => {
                    added += 1;
                    println!("{}", t!("cli.import.overwrite_entry", key = &candidate.key));
                }
            }
        }
    }

    if !dry_run {
        config = working;
        save_config(&config)?;
    }

    println!(
        "{}",
        t!(
            "cli.import.summary",
            added = added.to_string(),
            skipped = skipped.to_string(),
            local = local_count.to_string()
        )
    );
    if dry_run {
        println!("{}", t!("cli.import.dry_run"));
    } else {
        println!("{}", t!("cli.import.written", path = CONFIG_FILE));
    }

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
        .mut_subcommand("update", |c| c.about(t!("cli.update.help").to_string()))
        .mut_subcommand("upgrade-env", |c| {
            c.about(t!("cli.upgrade_env.help").to_string())
        })
        .mut_subcommand("migrate-loader", |c| {
            c.about(t!("cli.migrate_loader.help").to_string())
        })
        .mut_subcommand("apply-plan", |c| {
            c.about(t!("cli.apply_plan.help").to_string())
        })
        .mut_subcommand("import", |c| c.about(t!("cli.import.help").to_string()))
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
            Commands::Update { allow_loader } => cmd_update(allow_loader).await,
            Commands::UpgradeEnv { target_minecraft } => cmd_upgrade_env(target_minecraft).await,
            Commands::MigrateLoader { to } => cmd_migrate_loader(to).await,
            Commands::ApplyPlan => cmd_apply_plan().await,
            Commands::Import { target } => match target {
                ImportTarget::Mods { dry_run, overwrite } => {
                    cmd_import_mods(dry_run, overwrite).await
                }
            },
        },
    };

    if let Err(ref e) = result {
        eprintln!("{}: {:#}", t!("cli.common.error_prefix"), e);
        std::process::exit(1);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(keys: &[&str]) -> PackConfig {
        let mut mods = indexmap::IndexMap::new();
        for key in keys {
            mods.insert(key.to_string(), ModEntry::Simple("latest".to_string()));
        }
        PackConfig {
            pack: PackMeta {
                name: "test".to_string(),
                author: None,
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
            mods,
            compatibility: None,
        }
    }

    fn candidate(key: &str) -> ImportCandidate {
        ImportCandidate {
            key: key.to_string(),
            entry: ModEntry::Detailed(DetailedModEntry {
                version: Some("1.0.0".to_string()),
                file_id: None,
                source: Some(mcpm_core::SourceKind::Modrinth),
                side: None,
                project_id: Some("abc".to_string()),
            }),
            is_local: false,
        }
    }

    #[test]
    fn test_merge_entry_added() {
        let mut config = config_with(&[]);
        assert_eq!(
            merge_entry(&mut config, &candidate("sodium"), false),
            MergeOutcome::Added
        );
        assert!(config.mods.contains_key("sodium"));
    }

    #[test]
    fn test_merge_entry_skip_existing() {
        let mut config = config_with(&["sodium"]);
        let outcome = merge_entry(&mut config, &candidate("sodium"), false);
        assert_eq!(outcome, MergeOutcome::Skipped);
        assert!(matches!(
            config.mods.get("sodium"),
            Some(ModEntry::Simple(v)) if v == "latest"
        ));
    }

    #[test]
    fn test_merge_entry_overwrite() {
        let mut config = config_with(&["sodium"]);
        let outcome = merge_entry(&mut config, &candidate("sodium"), true);
        assert_eq!(outcome, MergeOutcome::Overwritten);
        assert!(matches!(
            config.mods.get("sodium"),
            Some(ModEntry::Detailed(_))
        ));
    }
}
