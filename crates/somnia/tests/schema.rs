//! Proves `#[derive(SurrealRecord)]` makes the Rust type the source of truth for
//! the SurrealDB schema: `up()` emits the same `DEFINE TABLE`/`DEFINE FIELD` DDL
//! as the hand-written `migrations/027_create_asset_enrichment_schema.surql`, and
//! `down()` is its inverse.

#[cfg(test)]
mod tests {
    use somnia::{
        DefineAnalyzer, DefineEvent, DefineFunction, DefineParam, For, IfExpr, SurrealRecord,
        SurrealSchema, Thing, Transaction,
    };

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
        assert_eq!(
            AssetVersion::down(),
            "REMOVE TABLE IF EXISTS asset_version;"
        );
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

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("member")]
    #[index(name = "member_email_unique", fields = "email", unique)]
    #[index(name = "member_name_idx", fields = "first_name, last_name")]
    #[allow(dead_code)]
    struct Member {
        #[field(thing)]
        id: Thing<Member>,
        email: String,
        first_name: String,
        last_name: String,
    }

    #[test]
    fn index_attrs_emit_define_index_ddl() {
        assert_eq!(
            Member::define_indexes(),
            &[
                "DEFINE INDEX IF NOT EXISTS member_email_unique ON TABLE member FIELDS email UNIQUE;",
                "DEFINE INDEX IF NOT EXISTS member_name_idx ON TABLE member FIELDS first_name, last_name;",
            ]
        );
        // up() appends indexes after the field definitions.
        let up = Member::up();
        let fields_end = up.find("DEFINE INDEX").unwrap();
        assert!(up[..fields_end]
            .contains("DEFINE FIELD IF NOT EXISTS email ON TABLE member TYPE string;"));
        assert!(up.ends_with("FIELDS first_name, last_name;"));
    }

    #[test]
    fn indexes_apply_and_enforce_on_live_surreal() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();
            db.query(Member::up()).await.unwrap().check().unwrap();

            // First insert with a given email succeeds.
            db.query("CREATE member SET email = 'a@x.com', first_name = 'A', last_name = 'B';")
                .await
                .unwrap()
                .check()
                .unwrap();
            // A second insert with the same email must violate the UNIQUE index.
            let dup = db
                .query("CREATE member SET email = 'a@x.com', first_name = 'C', last_name = 'D';")
                .await
                .unwrap()
                .check();
            assert!(
                dup.is_err(),
                "UNIQUE index should reject the duplicate email"
            );
        });
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("account")]
    #[allow(dead_code)]
    struct Account {
        #[field(thing)]
        id: Thing<Account>,
        #[field(assert = "$value >= 0")]
        balance: i64,
        #[field(readonly)]
        created_by: String,
        #[field(permissions = "FOR select NONE")]
        secret: String,
    }

    #[test]
    fn field_assert_readonly_permissions_ddl() {
        assert_eq!(
            Account::define_fields(),
            &[
                "DEFINE FIELD IF NOT EXISTS balance ON TABLE account TYPE int ASSERT $value >= 0;",
                "DEFINE FIELD IF NOT EXISTS created_by ON TABLE account TYPE string READONLY;",
                "DEFINE FIELD IF NOT EXISTS secret ON TABLE account TYPE string PERMISSIONS FOR select NONE;",
            ]
        );
    }

    #[test]
    fn field_attrs_apply_on_live_surreal() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();
            db.query(Account::up()).await.unwrap().check().unwrap();
            // A valid row applies.
            db.query("CREATE account SET balance = 10, created_by = 'me', secret = 's';")
                .await
                .unwrap()
                .check()
                .unwrap();
            // ASSERT rejects a negative balance.
            let bad = db
                .query("CREATE account SET balance = -1, created_by = 'me', secret = 's';")
                .await
                .unwrap()
                .check();
            assert!(bad.is_err(), "ASSERT $value >= 0 should reject -1");
        });
    }

    #[test]
    fn standalone_ddl_builders_apply_on_live_surreal() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();
            db.query("DEFINE TABLE post SCHEMALESS;")
                .await
                .unwrap()
                .check()
                .unwrap();

            for ddl in [
                DefineAnalyzer::new("ascii")
                    .tokenizers(["class"])
                    .filters(["lowercase", "ascii"])
                    .to_surrealql(),
                DefineParam::new("rate", "0.5").to_surrealql(),
                DefineFunction::new("greet")
                    .arg("name", "string")
                    .returns("string")
                    .body("RETURN 'hi ' + $name;")
                    .to_surrealql(),
                DefineEvent::new("on_post", "post")
                    .when("$event = 'CREATE'")
                    .then("{ CREATE log SET at = time::now() }")
                    .to_surrealql(),
            ] {
                db.query(&ddl).await.unwrap().check().unwrap();
            }

            // The function is callable.
            let mut res = db
                .query("RETURN fn::greet('world');")
                .await
                .unwrap()
                .check()
                .unwrap();
            let out: Option<String> = res.take(0).unwrap();
            assert_eq!(out.as_deref(), Some("hi world"));

            // The param resolves to its defined value.
            let mut res = db.query("RETURN $rate;").await.unwrap().check().unwrap();
            let rate: Option<f64> = res.take(0).unwrap();
            assert_eq!(rate, Some(0.5));
        });
    }

    #[test]
    fn control_flow_runs_on_live_surreal() {
        use somnia::{DynExpr, Raw};
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();

            // FOR loop creates three rows.
            let for_sql = For::new("n", Raw("[1, 2, 3]".into()))
                .push("CREATE counter SET v = $n")
                .to_surrealql();
            db.query(&for_sql).await.unwrap().check().unwrap();
            let mut res = db
                .query("SELECT count() FROM counter GROUP ALL;")
                .await
                .unwrap()
                .check()
                .unwrap();
            let rows: Vec<serde_json::Value> = res.take(0).unwrap();
            assert_eq!(rows[0]["count"].as_i64(), Some(3));

            // IF expression evaluates the matching branch.
            let mut if_buf = String::from("RETURN ");
            IfExpr::new(Raw("5 > 3".into()), Raw("'yes'".into()))
                .else_(Raw("'no'".into()))
                .render_dyn(&mut if_buf);
            let mut res = db.query(&if_buf).await.unwrap().check().unwrap();
            let out: Option<String> = res.take(0).unwrap();
            assert_eq!(out.as_deref(), Some("yes"));
        });
    }

    #[test]
    fn transaction_is_atomic_on_live_surreal() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();
            db.query(Member::up()).await.unwrap().check().unwrap();

            // Two inserts in one transaction; the second reuses the first's email,
            // violating the UNIQUE index. The whole transaction must roll back.
            let tx = Transaction::new()
                .push("CREATE member SET email = 'dup@x.com', first_name = 'A', last_name = 'B'")
                .push("CREATE member SET email = 'dup@x.com', first_name = 'C', last_name = 'D'");
            let res = db.query(tx.to_surrealql()).await.unwrap().check();
            assert!(res.is_err(), "transaction should fail on the duplicate");

            // Atomicity: the first insert must NOT have persisted.
            let mut count = db
                .query("SELECT count() FROM member GROUP ALL;")
                .await
                .unwrap()
                .check()
                .unwrap();
            let rows: Vec<serde_json::Value> = count.take(0).unwrap();
            let n = rows
                .first()
                .and_then(|r| r.get("count"))
                .and_then(|c| c.as_i64())
                .unwrap_or(0);
            assert_eq!(n, 0, "rolled-back transaction must leave no rows");
        });
    }

    // Exercises the recursive type mapper: typed arrays, arrays of records,
    // nested Option, and duration.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("catalog")]
    #[allow(dead_code)]
    struct Catalog {
        #[field(thing)]
        id: Thing<Catalog>,
        tags: Vec<String>,
        scores: Vec<i64>,
        owners: Vec<Thing<Member>>,
        nicknames: Option<Vec<String>>,
        ttl: std::time::Duration,
    }

    #[test]
    fn richer_type_mapping_emits_precise_field_types() {
        assert_eq!(
            Catalog::define_fields(),
            &[
                "DEFINE FIELD IF NOT EXISTS tags ON TABLE catalog TYPE array<string>;",
                "DEFINE FIELD IF NOT EXISTS scores ON TABLE catalog TYPE array<int>;",
                "DEFINE FIELD IF NOT EXISTS owners ON TABLE catalog TYPE array<record>;",
                "DEFINE FIELD IF NOT EXISTS nicknames ON TABLE catalog TYPE option<array<string>>;",
                "DEFINE FIELD IF NOT EXISTS ttl ON TABLE catalog TYPE duration;",
            ]
        );
    }

    #[test]
    fn richer_types_apply_on_live_surreal() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = surrealdb::engine::any::connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();
            db.query(Catalog::up()).await.unwrap().check().unwrap();
            // A write satisfying every typed field must succeed under SCHEMAFULL.
            db.query(
                "CREATE catalog SET tags = ['a', 'b'], scores = [1, 2], \
                 owners = [member:x], nicknames = NONE, ttl = 1s500ms;",
            )
            .await
            .unwrap()
            .check()
            .unwrap();
        });
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
            db.query(AssetVersion::down())
                .await
                .unwrap()
                .check()
                .unwrap();
            let res = db.query("SELECT * FROM asset_version;").await.unwrap();
            assert!(res.check().is_err(), "table should not exist after down()");

            // up() is idempotent / reversible: re-applying it succeeds again.
            db.query(AssetVersion::up()).await.unwrap().check().unwrap();
        });
    }
}
