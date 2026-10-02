mod i18n;

rust_i18n::i18n!("locales", fallback = "en");

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Command, CommandFactory, FromArgMatches, Parser, Subcommand};
use i18n::t;
use mcpm_core::{
    version_matches_environment, version_matches_preferences, DetailedModEntry, Environment,
    LoaderKind, LoaderSpec, LockReason, ModEntry, PackConfig, PackMeta, Preferences, Registry,
    Side,
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

    /// Add a mod to the modpack
    Add {
        /// Mod slug or project ID
        slug: String,
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

    let plan = mcpm_resolver::resolve(
        &registry,
        &config.mods,
        &config.environment,
        &config.preferences,
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

    let installer = Installer::new(std::path::Path::new("."))?;
    let result = installer.install(&lockfile).await?;

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
    } else {
        anyhow::bail!(
            "{}",
            t!(
                "cli.install.with_errors",
                count = result.errors.len().to_string()
            )
        );
    }

    Ok(lockfile)
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
            loader: LoaderSpec::Fixed(loader_kind),
            loader_version: None,
            side: side_kind,
        },
        preferences: Preferences::default(),
        mods: indexmap::IndexMap::new(),
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

async fn cmd_add(slug: String) -> Result<()> {
    let mut config = load_config()?;

    if config.mods.contains_key(&slug) {
        anyhow::bail!("{}", t!("cli.add.already_exists", slug = &slug));
    }

    let loader = config
        .environment
        .loader
        .as_fixed()
        .ok_or_else(|| anyhow::anyhow!("{}", t!("cli.error.loader_not_fixed")))?;
    let game_version = &config.environment.minecraft;

    println!("{}", t!("cli.add.searching", slug = &slug));
    let registry = ModrinthClient::new();
    let results = registry.search_projects(&slug).await?;

    if results.is_empty() {
        anyhow::bail!("{}", t!("cli.add.no_results", slug = &slug));
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
            version_matches_environment(v, game_version, loader)
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

    println!(
        "{}",
        t!(
            "cli.add.compatible",
            version = &compatible[0].version_number,
            release_type = compatible[0].version_type.to_string()
        )
    );

    let entry = ModEntry::Detailed(DetailedModEntry {
        version: Some("latest".to_string()),
        source: Some(mcpm_core::SourceKind::Modrinth),
        side: None,
        project_id: Some(project.id.clone()),
    });

    let display_name = project.slug.as_deref().unwrap_or(&slug).to_string();
    config.mods.insert(display_name.clone(), entry);

    save_config(&config)?;
    println!("{}", t!("cli.add.added", title = &project.title));

    let existing_lockfile = load_lockfile()?;

    if let Some(mut lockfile) = existing_lockfile {
        let mcpm_version = env!("CARGO_PKG_VERSION");
        let new_entries = {
            let mut temp_config = config.clone();
            temp_config.mods.clear();
            temp_config.mods.insert(
                display_name.clone(),
                ModEntry::Detailed(DetailedModEntry {
                    version: Some("latest".to_string()),
                    source: Some(mcpm_core::SourceKind::Modrinth),
                    side: None,
                    project_id: Some(project.id.clone()),
                }),
            );
            let plan = mcpm_resolver::resolve(
                &registry,
                &temp_config.mods,
                &temp_config.environment,
                &temp_config.preferences,
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

            let installer = Installer::new(std::path::Path::new("."))?;
            let result = installer.install(&install_lock).await?;

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
            } else {
                anyhow::bail!(
                    "{}",
                    t!(
                        "cli.install.with_errors",
                        count = result.errors.len().to_string()
                    )
                );
            }
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
    let config = load_config()?;
    resolve_and_install(&config).await?;
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

    println!("{}", t!("cli.check.complete"));
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
            Commands::Add { slug } => cmd_add(slug).await,
            Commands::Remove { slug } => cmd_remove(slug).await,
            Commands::Autoremove => cmd_autoremove().await,
            Commands::Install => cmd_install().await,
            Commands::List => cmd_list().await,
            Commands::Check => cmd_check().await,
        },
    };

    if let Err(ref e) = result {
        eprintln!("{}: {:#}", t!("cli.common.error_prefix"), e);
        std::process::exit(1);
    }

    result
}
