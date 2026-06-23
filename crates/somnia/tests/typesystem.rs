//! SurrealDB 3.x type-system features: closures, record `REFERENCE`s, and
//! union / literal field types. Builder/schema assertions plus live roundtrips.
//!
//! (`future<T>` is intentionally not modelled — the keyword was removed from
//! SurrealDB 3.x; use a computed `#[field(value = "…")]` instead.)

#[cfg(test)]
mod tests {
    use somnia::{Closure, Func, Raw, SurrealRecord, SurrealSchema, Thing};

    // ───────────────────────────── closures ─────────────────────────────

    #[test]
    fn closure_renders_args_and_return() {
        let c = Closure::new(Raw("$x * 2".to_string()))
            .arg("x", "int")
            .returns("int");
        let mut buf = String::new();
        somnia::DynExpr::render_dyn(&c, &mut buf);
        assert_eq!(buf, "|$x: int| -> int $x * 2");
    }

    #[test]
    fn closure_no_args() {
        let c = Closure::new(Raw("42".to_string()));
        let mut buf = String::new();
        somnia::DynExpr::render_dyn(&c, &mut buf);
        assert_eq!(buf, "|| 42");
    }

    #[tokio::test]
    async fn closure_runs_in_higher_order_fn() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("t").use_db("t").await.unwrap();

        let mapper = Closure::new(Raw("$v * 2".to_string())).arg("v", "int");
        let call = Func::new(
            "array::map",
            vec![Box::new(Raw("[1, 2, 3]".to_string())), Box::new(mapper)],
        );
        let mut sql = String::new();
        somnia::DynExpr::render_dyn(&call, &mut sql);
        assert_eq!(sql, "array::map([1, 2, 3], |$v: int| $v * 2)");

        let mut res = db
            .query(format!("RETURN {sql};"))
            .await
            .unwrap()
            .check()
            .unwrap();
        let out: Vec<i64> = res.take(0).unwrap();
        assert_eq!(out, vec![2, 4, 6]);
    }

    // ───────────────────────────── references ─────────────────────────────

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("ts_user", schemaless)]
    struct TsUser {
        #[field(thing)]
        id: Thing<TsUser>,
        name: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("ts_comment", schemaless)]
    struct TsComment {
        #[field(thing)]
        id: Thing<TsComment>,
        #[field(record = "ts_user", reference = "cascade")]
        author: Thing<TsUser>,
        body: String,
    }

    #[test]
    fn reference_clause_in_define_field() {
        let fields = TsComment::define_fields();
        assert!(
            fields.iter().any(|f| f
                == &"DEFINE FIELD IF NOT EXISTS author ON TABLE ts_comment \
                     TYPE record<ts_user> REFERENCE ON DELETE CASCADE;"),
            "missing REFERENCE clause, got {fields:?}"
        );
    }

    #[tokio::test]
    async fn reference_cascade_applies_on_live_engine() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("t").use_db("t").await.unwrap();

        // Apply both schemas; the comment carries a cascading reference.
        db.query(TsUser::up()).await.unwrap().check().unwrap();
        db.query(TsComment::up()).await.unwrap().check().unwrap();

        db.query(
            "CREATE ts_user:alice SET name = 'Alice';
             CREATE ts_comment:c1 SET author = ts_user:alice, body = 'hi';",
        )
        .await
        .unwrap()
        .check()
        .unwrap();

        // Deleting the referenced user cascades to the comment.
        db.query("DELETE ts_user:alice;")
            .await
            .unwrap()
            .check()
            .unwrap();
        let mut res = db.query("SELECT * FROM ts_comment;").await.unwrap();
        let remaining: Vec<serde_json::Value> = res.take(0).unwrap();
        assert!(
            remaining.is_empty(),
            "ON DELETE CASCADE should have removed the comment, got {remaining:?}"
        );
    }

    // ─────────────────────────── union / literal types ───────────────────────────

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("ts_variant", schemaless)]
    struct TsVariant {
        #[field(thing)]
        id: Thing<TsVariant>,
        #[field(ty = "int | string")]
        mixed: serde_json::Value,
        #[field(ty = "'draft' | 'published' | 'archived'")]
        status: String,
    }

    #[test]
    fn union_and_literal_types_in_define_field() {
        let fields = TsVariant::define_fields();
        assert!(fields.iter().any(|f| f.contains("TYPE int | string;")));
        assert!(fields
            .iter()
            .any(|f| f.contains("TYPE 'draft' | 'published' | 'archived';")));
    }

    #[tokio::test]
    async fn union_and_literal_types_enforced_on_live_engine() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("t").use_db("t").await.unwrap();
        db.query(TsVariant::up()).await.unwrap().check().unwrap();

        // Valid: int|string union and an allowed literal.
        db.query("CREATE ts_variant:ok SET mixed = 7, status = 'draft';")
            .await
            .unwrap()
            .check()
            .unwrap();
        db.query("CREATE ts_variant:ok2 SET mixed = 'hello', status = 'published';")
            .await
            .unwrap()
            .check()
            .unwrap();

        // Invalid literal is rejected.
        let bad = db
            .query("CREATE ts_variant:bad SET mixed = 1, status = 'bogus';")
            .await
            .unwrap()
            .check();
        assert!(bad.is_err(), "literal type should reject 'bogus'");
    }
}
