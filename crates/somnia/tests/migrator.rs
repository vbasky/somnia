//! Exercises the `Migrator` against a self-contained fixture migration set on an
//! in-memory SurrealDB: apply all ups, verify tracking + ordering + idempotency,
//! then revert all downs.

#[cfg(test)]
mod tests {
    use somnia::Migrator;

    fn fixtures() -> std::path::PathBuf {
        // tests run with CWD = crates/somnia
        std::path::PathBuf::from("tests/fixtures/migrations")
    }

    #[tokio::test]
    async fn run_status_and_revert() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("t").use_db("t").await.unwrap();
        let migrator = Migrator::new(db.clone(), fixtures());

        let total = migrator.status().await.unwrap().len();
        assert_eq!(total, 3, "expected 3 fixture migrations");

        // Apply everything.
        let applied = migrator.run().await.unwrap();
        assert_eq!(applied.len(), 3, "all migrations should apply on a fresh DB");

        // The seed ran, so the row exists.
        let mut res = db.query("SELECT name FROM widget:one;").await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.first().and_then(|r| r["name"].as_str()), Some("first"));

        // Status now shows everything applied, in ascending (timestamp) order.
        let status = migrator.status().await.unwrap();
        assert!(status.iter().all(|m| m.applied));
        let ids: Vec<&str> = status.iter().map(|m| m.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "migrations must be timestamp-ordered");

        // Re-running is a no-op (nothing pending).
        assert!(migrator.run().await.unwrap().is_empty());

        // Revert the latest, then the rest.
        let last = migrator.revert_last().await.unwrap();
        assert_eq!(last.as_deref(), ids.last().copied());
        let rest = migrator.revert_all().await.unwrap();
        assert_eq!(rest.len(), 2);

        // Nothing applied after reverting all.
        let after = migrator.status().await.unwrap();
        assert!(after.iter().all(|m| !m.applied));
    }
}
