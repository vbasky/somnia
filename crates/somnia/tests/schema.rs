//! Proves `#[derive(SurrealRecord)]` makes the Rust type the source of truth for
//! the SurrealDB schema: `up()` emits the same `DEFINE TABLE`/`DEFINE FIELD` DDL
//! as the hand-written `migrations/027_create_asset_enrichment_schema.surql`, and
//! `down()` is its inverse.

#[cfg(test)]
mod tests {
    use somnia::{SurrealSchema, Thing};
    use somnia_derive::SurrealRecord;

    // Mirrors the `asset_version` table from migration 027 — the Rust type is now
    // the single source of truth for that schema.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("asset_version")]
    #[allow(dead_code)]
    struct AssetVersion {
        #[field(thing)]
        id: Thing<AssetVersion>,
        #[field(record = "tenant")]
        tenant: Option<serde_json::Value>,
        #[field(record = "asset")]
        asset: serde_json::Value,
        #[field(default = "1")]
        version_number: i64,
        label: Option<String>,
        storage_account: String,
        container: String,
        name: String,
        url: String,
        #[field(default = "0")]
        size: i64,
        hash: Option<String>,
        content_type: Option<String>,
        #[field(record = "user")]
        created_by: Option<serde_json::Value>,
        change_notes: Option<String>,
        #[field(default = "false")]
        is_current: bool,
        #[field(ty = "datetime", default = "time::now()")]
        created_at: String,
    }

    #[test]
    fn up_matches_migration_027() {
        let expected = "\
DEFINE TABLE IF NOT EXISTS asset_version SCHEMAFULL PERMISSIONS FULL;
DEFINE FIELD IF NOT EXISTS tenant ON TABLE asset_version TYPE option<record<tenant>>;
DEFINE FIELD IF NOT EXISTS asset ON TABLE asset_version TYPE record<asset>;
DEFINE FIELD IF NOT EXISTS version_number ON TABLE asset_version TYPE int DEFAULT 1;
DEFINE FIELD IF NOT EXISTS label ON TABLE asset_version TYPE option<string>;
DEFINE FIELD IF NOT EXISTS storage_account ON TABLE asset_version TYPE string;
DEFINE FIELD IF NOT EXISTS container ON TABLE asset_version TYPE string;
DEFINE FIELD IF NOT EXISTS name ON TABLE asset_version TYPE string;
DEFINE FIELD IF NOT EXISTS url ON TABLE asset_version TYPE string;
DEFINE FIELD IF NOT EXISTS size ON TABLE asset_version TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS hash ON TABLE asset_version TYPE option<string>;
DEFINE FIELD IF NOT EXISTS content_type ON TABLE asset_version TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_by ON TABLE asset_version TYPE option<record<user>>;
DEFINE FIELD IF NOT EXISTS change_notes ON TABLE asset_version TYPE option<string>;
DEFINE FIELD IF NOT EXISTS is_current ON TABLE asset_version TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE asset_version TYPE datetime DEFAULT time::now();";

        assert_eq!(AssetVersion::up(), expected);
    }

    #[test]
    fn down_drops_the_table() {
        assert_eq!(AssetVersion::down(), "REMOVE TABLE IF EXISTS asset_version;");
    }

    #[test]
    fn schemaless_and_custom_permissions() {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
        #[table("scratch", schemaless, permissions = "NONE")]
        #[allow(dead_code)]
        struct Scratch {
            #[field(thing)]
            id: Thing<Scratch>,
            blob: serde_json::Value,
        }
        assert_eq!(
            Scratch::define_table(),
            "DEFINE TABLE IF NOT EXISTS scratch SCHEMALESS PERMISSIONS NONE;"
        );
        // Fields are still defined even on a schemaless table.
        assert_eq!(
            Scratch::define_fields(),
            &["DEFINE FIELD IF NOT EXISTS blob ON TABLE scratch TYPE object;"]
        );
    }

    #[test]
    fn up_down_round_trips_against_live_surreal() {
        // The generated DDL must actually apply (and reverse) on SurrealDB 3.x.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();

            // up: define schema, then a write that satisfies SCHEMAFULL must succeed.
            db.query(AssetVersion::up()).await.unwrap().check().unwrap();
            db.query(
                "CREATE asset_version SET asset = asset:a1, version_number = 2, \
                 storage_account = 'sa', container = 'c', name = 'n', url = 'u', \
                 is_current = true;",
            )
            .await
            .unwrap()
            .check()
            .unwrap();

            // down: drop the table. Selecting from the now-removed SCHEMAFULL
            // table errors with "table does not exist" — proving the reversal.
            db.query(AssetVersion::down()).await.unwrap().check().unwrap();
            let res = db.query("SELECT * FROM asset_version;").await.unwrap();
            assert!(res.check().is_err(), "table should not exist after down()");

            // up() is idempotent / reversible: re-applying it succeeds again.
            db.query(AssetVersion::up()).await.unwrap().check().unwrap();
        });
    }
}
