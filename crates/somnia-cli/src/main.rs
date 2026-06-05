//! `somnia` — a diesel-cli-style migration runner for the somnia SurrealDB ORM.
//!
//! ```text
//! somnia migration run        # apply all pending up.surql
//! somnia migration revert     # revert the most recent (--all for everything)
//! somnia migration redo       # revert the most recent, then re-apply it
//! somnia migration list       # show applied / pending
//! somnia migration generate <name>   # scaffold a timestamped up/down folder
//! ```
//!
//! Connection is configured via flags or environment variables
//! (`SOMNIA_ENDPOINT`, `SOMNIA_USER`, `SOMNIA_PASS`, `SOMNIA_NS`, `SOMNIA_DB`).
//! `generate` works offline (no connection needed).

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use surrealdb::engine::any::{connect, Any};
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

#[derive(Parser)]
#[command(
    name = "somnia",
    version,
    about = "Migration runner for the somnia SurrealDB ORM"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage database migrations
    #[command(subcommand)]
    Migration(MigrationCmd),
}

#[derive(Subcommand)]
enum MigrationCmd {
    /// Apply all pending migrations
    Run(WithConn),
    /// Revert migrations (the most recent by default, or all with --all)
    Revert {
        #[command(flatten)]
        conn: WithConn,
        /// Revert every applied migration
        #[arg(long)]
        all: bool,
    },
    /// Revert the most recent migration, then re-apply it
    Redo(WithConn),
    /// List applied and pending migrations
    List(WithConn),
    /// Scaffold a new timestamped migration folder (offline)
    Generate {
        /// Name appended after the timestamp
        name: String,
        /// Migrations directory
        #[arg(long, env = "SOMNIA_MIGRATIONS", default_value = "migrations")]
        dir: std::path::PathBuf,
    },
}

/// Connection + migrations-dir options shared by the DB-touching subcommands.
#[derive(Args)]
struct WithConn {
    /// SurrealDB endpoint (e.g. ws://localhost:8000)
    #[arg(long, env = "SOMNIA_ENDPOINT", default_value = "ws://localhost:8000")]
    endpoint: String,
    #[arg(long, env = "SOMNIA_USER", default_value = "root")]
    user: String,
    #[arg(long, env = "SOMNIA_PASS", default_value = "root")]
    pass: String,
    #[arg(long, env = "SOMNIA_NS", default_value = "test")]
    ns: String,
    #[arg(long, env = "SOMNIA_DB", default_value = "test")]
    db: String,
    /// Migrations directory
    #[arg(long, env = "SOMNIA_MIGRATIONS", default_value = "migrations")]
    dir: std::path::PathBuf,
}

async fn connect_db(c: &WithConn) -> Result<Surreal<Any>> {
    let db = connect(&c.endpoint)
        .await
        .with_context(|| format!("connecting to {}", c.endpoint))?;
    db.signin(Root {
        username: c.user.clone(),
        password: c.pass.clone(),
    })
    .await
    .context("signin")?;
    db.use_ns(c.ns.clone())
        .use_db(c.db.clone())
        .await
        .context("use ns/db")?;
    Ok(db)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Migration(cmd) => run_migration(cmd).await,
    }
}

async fn run_migration(cmd: MigrationCmd) -> Result<()> {
    match cmd {
        MigrationCmd::Run(c) => {
            let m = somnia::Migrator::new(connect_db(&c).await?, c.dir);
            let applied = m.run().await?;
            if applied.is_empty() {
                println!("Already up to date — nothing to apply.");
            } else {
                for id in &applied {
                    println!("applied  {id}");
                }
                println!("✅ {} migration(s) applied.", applied.len());
            }
        }
        MigrationCmd::Revert { conn, all } => {
            let m = somnia::Migrator::new(connect_db(&conn).await?, conn.dir);
            if all {
                let reverted = m.revert_all().await?;
                for id in &reverted {
                    println!("reverted {id}");
                }
                println!("✅ {} migration(s) reverted.", reverted.len());
            } else {
                match m.revert_last().await? {
                    Some(id) => println!("reverted {id}"),
                    None => println!("Nothing to revert."),
                }
            }
        }
        MigrationCmd::Redo(c) => {
            let m = somnia::Migrator::new(connect_db(&c).await?, c.dir);
            match m.revert_last().await? {
                Some(id) => {
                    println!("reverted {id}");
                    let applied = m.run().await?;
                    for id in &applied {
                        println!("applied  {id}");
                    }
                }
                None => println!("Nothing to redo."),
            }
        }
        MigrationCmd::List(c) => {
            let m = somnia::Migrator::new(connect_db(&c).await?, c.dir);
            for s in m.status().await? {
                println!("{}  {}", if s.applied { "[X]" } else { "[ ]" }, s.id);
            }
        }
        MigrationCmd::Generate { name, dir } => {
            let ts = chrono::Utc::now().format("%Y-%m-%d-%H%M%S");
            let slug = name.trim().replace([' ', '-'], "_").to_lowercase();
            let folder = dir.join(format!("{ts}_{slug}"));
            std::fs::create_dir_all(&folder)?;
            std::fs::write(
                folder.join("up.surql"),
                format!(
                    "-- up: {name}\n-- Write idempotent SurrealQL (IF NOT EXISTS / UPSERT).\n\n"
                ),
            )?;
            std::fs::write(
                folder.join("down.surql"),
                format!("-- down: revert {name}\n-- e.g. REMOVE TABLE IF EXISTS ...;\n\n"),
            )?;
            println!("Created {}", folder.display());
        }
    }
    Ok(())
}
