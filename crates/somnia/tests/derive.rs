//! Exercises the `#[derive(SurrealRecord)]` macro end to end: the generated
//! column accessors, `all()`, and `table()` must produce SurrealQL that both
//! matches the hand-written form and actually executes against SurrealDB.

#[cfg(test)]
mod tests {
    use somnia::SurrealRecord;
    use somnia::{Relate, SurrealEdge, Thing};
    use surrealdb::engine::any::connect;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("asset")]
    struct Asset {
        #[field(thing)]
        id: Thing<Asset>,
        name: String,
        #[field(name = "content_type")]
        content_type: Option<String>,
        file_size: Option<i64>,
    }

    #[test]
    fn derive_sets_table_and_primary_key() {
        assert_eq!(Asset::table_name(), "asset");
        assert_eq!(Asset::primary_key(), "id");
    }

    #[test]
    fn derive_generates_typed_column_accessors() {
        // `#[field(name = "content_type")]` must rename the db column even though
        // the Rust field is also `content_type`; the accessor stays field-named.
        assert_eq!(Asset::content_type().name, "content_type");
        assert_eq!(Asset::name().name, "name");
        assert_eq!(Asset::file_size().surreal_type, "option<int>");
    }

    #[test]
    fn derive_query_matches_handwritten_surrealql() {
        let sql = Asset::table()
            .select(Asset::all())
            .filter(Asset::content_type().eq(Some("video/mp4".to_string())))
            .order_asc(Asset::name())
            .limit(10)
            .to_surrealql();

        assert_eq!(
            sql,
            "SELECT * FROM asset WHERE content_type = 'video/mp4' ORDER BY name ASC LIMIT 10"
        );
    }

    #[tokio::test]
    async fn derive_query_executes() {
        let db = connect("mem://").await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();
        db.query("DEFINE TABLE asset SCHEMALESS;").await.unwrap();
        db.query(
            "
            INSERT INTO asset { name: 'a.mp4', content_type: 'video/mp4', file_size: 10 };
            INSERT INTO asset { name: 'b.jpg', content_type: 'image/jpeg', file_size: 20 };
        ",
        )
        .await
        .unwrap();

        let sql = Asset::table()
            .select(Asset::all())
            .filter(Asset::content_type().eq(Some("video/mp4".to_string())))
            .to_surrealql();
        let mut res = db.query(&sql).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "a.mp4");
    }

    // ─── RELATE / edges ─────────────────────────────────────────────────────────

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("user")]
    struct User {
        #[field(thing)]
        id: Thing<User>,
        name: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("follows")]
    struct Follows {
        #[field(thing)]
        id: Thing<Follows>,
    }

    impl SurrealEdge for Follows {
        fn edge_name() -> &'static str {
            "follows"
        }
    }

    #[test]
    fn relate_uses_record_tables_and_edge_name() {
        let alice: Thing<User> = Thing::new("alice");
        let bob: Thing<User> = Thing::new("bob");

        // Regression: this used to render the Rust type path via type_name::<E>()
        // for both record prefixes instead of the SurrealDB table names.
        let sql = Relate::<Follows>::to_surrealql(&alice, &bob);
        assert_eq!(sql, "RELATE user:alice -> follows -> user:bob");
    }
}
