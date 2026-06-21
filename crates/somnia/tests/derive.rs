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

    #[test]
    fn relate_escapes_complex_record_keys() {
        // A UUID key must be quoted so RELATE produces a valid record id.
        let a: Thing<User> = Thing::new("550e8400-e29b-41d4-a716-446655440000");
        let b: Thing<User> = Thing::new("bob");
        let sql = Relate::<Follows>::to_surrealql(&a, &b);
        assert_eq!(
            sql,
            "RELATE user:`550e8400-e29b-41d4-a716-446655440000` -> follows -> user:bob"
        );
    }

    #[test]
    fn relate_return_projection() {
        use somnia::{RelateEdge, Returning};
        let alice: Thing<User> = Thing::new("alice");
        let bob: Thing<User> = Thing::new("bob");
        assert_eq!(
            RelateEdge::<Follows>::from(&alice)
                .to(&bob)
                .returning(Returning::After)
                .build(),
            "RELATE user:alice -> follows -> user:bob RETURN AFTER"
        );
        assert_eq!(
            RelateEdge::<Follows>::from(&alice)
                .to(&bob)
                .return_field("id")
                .build(),
            "RELATE user:alice -> follows -> user:bob RETURN id"
        );
    }

    #[test]
    fn recursive_graph_path_executes_on_live_surreal() {
        use somnia::{DynExpr, Path};
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = connect("mem://").await.unwrap();
            db.use_ns("t").use_db("t").await.unwrap();
            // a follows b follows c — a two-hop chain.
            db.query(
                "CREATE user:a SET name='A'; CREATE user:b SET name='B'; CREATE user:c SET name='C'; \
                 RELATE user:a->follows->user:b; RELATE user:b->follows->user:c;",
            )
            .await
            .unwrap()
            .check()
            .unwrap();

            // A recursive path built by somnia must be valid SurrealQL and run.
            let mut path = String::new();
            Path::out::<Follows>()
                .to::<User>()
                .recurse_up_to(3)
                .render_dyn(&mut path);
            assert_eq!(path, "@.{..3}->follows->user");

            let sql = format!("SELECT {path} AS reach FROM user:a");
            let mut res = db.query(&sql).await.unwrap().check().unwrap();
            // The query is accepted by the engine and returns the anchor row.
            let rows: Vec<serde_json::Value> = res.take(0).unwrap();
            assert_eq!(rows.len(), 1, "recursive select should return one row for user:a");
        });
    }
}
