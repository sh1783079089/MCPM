use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "mcpm", version, about = "Minecraft pack manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new modpack
    Init {
        /// Minecraft version
        #[arg(long)]
        minecraft: Option<String>,

        /// Mod loader kind
        #[arg(long)]
        loader: Option<String>,
    },

    /// Add a mod to the modpack
    Add {
        /// Mod slug or project id
        slug: String,
    },

    /// Install mods from the lockfile
    Install,

    /// List installed mods
    List,

    /// Check for issues in the resolved modpack
    Check,

    /// Resolve dependencies and update the lockfile
    Solve,

    /// Explore compatible environment candidates
    SolveEnv,

    /// Update mods to latest compatible versions
    Update,
}

fn setup_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mcpm=info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

fn main() -> anyhow::Result<()> {
    setup_tracing();

    let cli = Cli::parse();

    match cli.command {
        None => {
            Cli::parse_from(["mcpm", "--help"]);
        }
        Some(cmd) => match cmd {
            Commands::Init { minecraft, loader } => {
                tracing::info!("init: minecraft={:?}, loader={:?}", minecraft, loader);
                println!("init command — not yet implemented");
            }
            Commands::Add { slug } => {
                tracing::info!("add: slug={}", slug);
                println!("add command — not yet implemented");
            }
            Commands::Install => {
                tracing::info!("install");
                println!("install command — not yet implemented");
            }
            Commands::List => {
                tracing::info!("list");
                println!("list command — not yet implemented");
            }
            Commands::Check => {
                tracing::info!("check");
                println!("check command — not yet implemented");
            }
            Commands::Solve => {
                tracing::info!("solve");
                println!("solve command — not yet implemented");
            }
            Commands::SolveEnv => {
                tracing::info!("solve-env");
                println!("solve-env command — not yet implemented");
            }
            Commands::Update => {
                tracing::info!("update");
                println!("update command — not yet implemented");
            }
        },
    }

    Ok(())
}
