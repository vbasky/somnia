//! Diesel-style migration runner for SurrealDB.
//!
//! A migration is a directory named `<timestamp>_<name>/` containing an
//! `up.surql` (apply) and a `down.surql` (revert). The runner discovers them in
//! lexical (= timestamp) order, applies pending `up.surql` files, and can revert
//! by running `down.surql` in reverse. Applied migrations are tracked in the
//! `_somnia_migrations` table so re-running only applies what's pending.
//!
//! ```ignore
//! let client = SomniaClient::connect("ws://localhost:8000", "root", "root", "ns", "db").await?;
//! let migrator = client.migrator("somnia-migrations");
//! let applied = migrator.run().await?;        // apply all pending ups
//! migrator.revert_last().await?;              // roll back the most recent
//! for m in migrator.status().await? { println!("{} {}", if m.applied {"✓"} else {" "}, m.id); }
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use somnia_core::error::SomniaError;

/// Tracking table for applied migrations.
const TRACKING_TABLE: &str = "_somnia_migrations";

/// One discovered migration on disk.
#[derive(Debug, Clone)]
struct Migration {
    /// Folder name, e.g. `2025-01-01-000000_create_multi_tenant_schema`.
    id: String,
    dir: PathBuf,
}

/// Apply/pending state of a migration, for `status()`.
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub id: String,
    pub applied: bool,
}

/// Runs `up.surql`/`down.surql` migrations from a directory against SurrealDB.
pub struct Migrator {
    db: Surreal<Any>,
    dir: PathBuf,
}

impl Migrator {
    pub fn new(db: Surreal<Any>, dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            dir: dir.into(),
        }
    }

    /// Apply every pending `up.surql` in timestamp order. Returns the ids applied.
    pub async fn run(&self) -> Result<Vec<String>, SomniaError> {
        self.ensure_tracking_table().await?;
        let applied = self.applied_ids().await?;
        let migrations = self.discover().await?;

        let mut newly = Vec::new();
        for m in &migrations {
            if applied.contains(&m.id) {
                continue;
            }
            let up = read_step(&m.dir, "up.surql").await?;
            self.exec(&up)
                .await
                .map_err(|e| SomniaError::migration(format!("up {}: {e}", m.id)))?;
            self.record_applied(&m.id).await?;
            tracing::info!(migration = %m.id, "applied");
            newly.push(m.id.clone());
        }
        Ok(newly)
    }

    /// Revert the most-recently-applied migration by running its `down.surql`.
    /// Returns the reverted id, or `None` if nothing was applied.
    pub async fn revert_last(&self) -> Result<Option<String>, SomniaError> {
        self.ensure_tracking_table().await?;
        let applied = self.applied_ids().await?;
        let migrations = self.discover().await?;
        // Highest (latest) applied migration in timestamp order.
        let Some(target) = migrations.iter().rev().find(|m| applied.contains(&m.id)) else {
            return Ok(None);
        };
        let down = read_step(&target.dir, "down.surql").await?;
        self.exec(&down)
            .await
            .map_err(|e| SomniaError::migration(format!("down {}: {e}", target.id)))?;
        self.record_reverted(&target.id).await?;
        tracing::info!(migration = %target.id, "reverted");
        Ok(Some(target.id.clone()))
    }

    /// Revert all applied migrations in reverse order. Returns reverted ids.
    pub async fn revert_all(&self) -> Result<Vec<String>, SomniaError> {
        let mut reverted = Vec::new();
        while let Some(id) = self.revert_last().await? {
            reverted.push(id);
        }
        Ok(reverted)
    }

    /// Applied/pending state for every migration on disk, in order.
    pub async fn status(&self) -> Result<Vec<MigrationStatus>, SomniaError> {
        self.ensure_tracking_table().await?;
        let applied = self.applied_ids().await?;
        Ok(self
            .discover()
            .await?
            .into_iter()
            .map(|m| MigrationStatus {
                applied: applied.contains(&m.id),
                id: m.id,
            })
            .collect())
    }

    // ─── internals ──────────────────────────────────────────────────────────

    async fn exec(&self, surql: &str) -> Result<(), surrealdb::Error> {
        // Skip comment-only / empty steps (valid no-op downs).
        let meaningful = surql.lines().any(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("--")
        });
        if !meaningful {
            return Ok(());
        }
        self.db.query(surql).await?.check()?;
        Ok(())
    }

    async fn ensure_tracking_table(&self) -> Result<(), SomniaError> {
        let ddl = format!(
            "DEFINE TABLE IF NOT EXISTS {t} SCHEMAFULL;\n\
             DEFINE FIELD IF NOT EXISTS applied_at ON TABLE {t} TYPE datetime DEFAULT time::now();",
            t = TRACKING_TABLE
        );
        self.db
            .query(&ddl)
            .await
            .map_err(|e| SomniaError::migration(e.to_string()))?
            .check()
            .map_err(|e| SomniaError::migration(e.to_string()))?;
        Ok(())
    }

    async fn applied_ids(&self) -> Result<HashSet<String>, SomniaError> {
        let q = format!("SELECT record::id(id) AS id FROM {TRACKING_TABLE};");
        let mut resp = self
            .db
            .query(&q)
            .await
            .map_err(|e| SomniaError::migration(e.to_string()))?;
        let rows: Vec<serde_json::Value> = resp
            .take(0)
            .map_err(|e| SomniaError::migration(e.to_string()))?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect())
    }

    async fn record_applied(&self, id: &str) -> Result<(), SomniaError> {
        let q =
            format!("CREATE type::record('{TRACKING_TABLE}', $id) SET applied_at = time::now();");
        self.db
            .query(&q)
            .bind(("id", id.to_string()))
            .await
            .map_err(|e| SomniaError::migration(e.to_string()))?
            .check()
            .map_err(|e| SomniaError::migration(e.to_string()))?;
        Ok(())
    }

    async fn record_reverted(&self, id: &str) -> Result<(), SomniaError> {
        let q = format!("DELETE type::record('{TRACKING_TABLE}', $id);");
        self.db
            .query(&q)
            .bind(("id", id.to_string()))
            .await
            .map_err(|e| SomniaError::migration(e.to_string()))?
            .check()
            .map_err(|e| SomniaError::migration(e.to_string()))?;
        Ok(())
    }

    /// Discover migration folders (those containing `up.surql`), sorted by name.
    async fn discover(&self) -> Result<Vec<Migration>, SomniaError> {
        let mut entries = tokio::fs::read_dir(&self.dir)
            .await
            .map_err(|e| SomniaError::migration(format!("read {}: {e}", self.dir.display())))?;
        let mut found = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| SomniaError::migration(e.to_string()))?
        {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            if !dir.join("up.surql").exists() {
                continue;
            }
            let id = dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| SomniaError::migration(String::from("invalid migration dir name")))?
                .to_string();
            found.push(Migration { id, dir });
        }
        found.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(found)
    }
}

async fn read_step(dir: &Path, file: &str) -> Result<String, SomniaError> {
    let path = dir.join(file);
    if !path.exists() {
        // A missing down.surql is treated as an empty (no-op) step.
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| SomniaError::migration(format!("read {}: {e}", path.display())))
}
