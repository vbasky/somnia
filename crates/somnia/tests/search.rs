//! Full-text (`Search`) and vector (`VectorSearch`) query-helper tests:
//! builder-output assertions plus live roundtrips against the in-memory engine.

#[cfg(test)]
mod tests {
    use somnia::{ident, SurrealRecord, Thing};

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("doc", schemaless)]
    struct Doc {
        #[field(thing)]
        id: Thing<Doc>,
        body: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("pt", schemaless)]
    struct Pt {
        #[field(thing)]
        id: Thing<Pt>,
        embedding: Vec<f32>,
    }

    // ───────────────────────────── full-text builder ─────────────────────────────

    #[test]
    fn search_renders_plain_match() {
        assert_eq!(
            Doc::table().search("body", "rust database").to_surrealql(),
            "SELECT * FROM doc WHERE body @@ 'rust database'"
        );
    }

    #[test]
    fn search_renders_score_and_order() {
        assert_eq!(
            Doc::table()
                .search("body", "rust database")
                .score_as("score")
                .order_by_score()
                .limit(10)
                .to_surrealql(),
            "SELECT *, search::score(0) AS score FROM doc \
             WHERE body @0@ 'rust database' ORDER BY score DESC LIMIT 10"
        );
    }

    #[test]
    fn search_explicit_reference_and_filter() {
        assert_eq!(
            Doc::table()
                .search("body", "rust")
                .reference(2)
                .filter(ident("body").contains("db".to_string()))
                .to_surrealql(),
            "SELECT * FROM doc WHERE body @2@ 'rust' AND body CONTAINS 'db'"
        );
    }

    #[test]
    fn search_with_params_binds_query() {
        let (sql, params) = Doc::table()
            .search("body", "rust database")
            .to_surrealql_with_params();
        assert_eq!(sql, "SELECT * FROM doc WHERE body @@ $p0");
        assert_eq!(
            params.get("p0").unwrap(),
            &serde_json::json!("rust database")
        );
    }

    // ───────────────────────────── vector builder ─────────────────────────────

    #[test]
    fn nearest_renders_hnsw_knn() {
        assert_eq!(
            Doc::table()
                .nearest("embedding", vec![0.1, 0.2, 0.3])
                .k(5)
                .distance_as("dist")
                .order_by_distance()
                .to_surrealql(),
            "SELECT *, vector::distance::knn() AS dist FROM doc \
             WHERE embedding <|5,5|> [0.1, 0.2, 0.3] ORDER BY dist"
        );
    }

    #[test]
    fn nearest_brute_force_with_metric() {
        assert_eq!(
            Doc::table()
                .nearest("embedding", vec![1.0, 2.0])
                .k(3)
                .distance("EUCLIDEAN")
                .to_surrealql(),
            "SELECT * FROM doc WHERE embedding <|3,EUCLIDEAN|> [1, 2]"
        );
    }

    #[test]
    fn nearest_approx_ef() {
        assert_eq!(
            Doc::table()
                .nearest("embedding", vec![1.0])
                .k(4)
                .ef(40)
                .to_surrealql(),
            "SELECT * FROM doc WHERE embedding <|4,40|> [1]"
        );
    }

    // ───────────────────────────── live engine ─────────────────────────────

    #[tokio::test]
    async fn full_text_search_runs_on_live_engine() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("t").use_db("t").await.unwrap();
        db.query(
            "DEFINE ANALYZER simple TOKENIZERS blank FILTERS lowercase;
             DEFINE TABLE doc SCHEMALESS;
             DEFINE INDEX body_idx ON doc FIELDS body FULLTEXT ANALYZER simple BM25;
             CREATE doc SET body = 'the rust database is fast';
             CREATE doc SET body = 'python data tools';
             CREATE doc SET body = 'rust systems programming';",
        )
        .await
        .unwrap()
        .check()
        .unwrap();

        let sql = Doc::table()
            .search("body", "rust")
            .score_as("score")
            .order_by_score()
            .to_surrealql();
        let mut res = db.query(&sql).await.unwrap().check().unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        // Both rust docs match; the python one does not.
        assert_eq!(rows.len(), 2, "expected 2 full-text matches, got {rows:?}");
        for row in &rows {
            let body = row["body"].as_str().unwrap();
            assert!(body.contains("rust"), "unexpected match: {body}");
            assert!(row.get("score").is_some(), "score not projected: {row:?}");
        }
    }

    #[tokio::test]
    async fn vector_search_runs_on_live_engine() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("t").use_db("t").await.unwrap();
        db.query(
            "DEFINE TABLE pt SCHEMALESS;
             DEFINE INDEX emb_idx ON pt FIELDS embedding HNSW DIMENSION 2 DIST EUCLIDEAN TYPE F32;
             CREATE pt:near SET embedding = [1.0, 1.0];
             CREATE pt:mid  SET embedding = [3.0, 3.0];
             CREATE pt:far  SET embedding = [9.0, 9.0];",
        )
        .await
        .unwrap()
        .check()
        .unwrap();

        // Query nearest to [1,1]; expect pt:near first, then mid, then far.
        let sql = Pt::table()
            .nearest("embedding", vec![1.0, 1.0])
            .k(3)
            .distance_as("dist")
            .order_by_distance()
            .to_surrealql();
        let mut res = db.query(&sql).await.unwrap().check().unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 3, "expected 3 ranked points, got {rows:?}");
        let ids: Vec<String> = rows
            .iter()
            .map(|r| r["id"].as_str().map(str::to_string).unwrap_or_default())
            .collect();
        assert!(
            ids[0].contains("near"),
            "nearest point should rank first, got order {ids:?}"
        );
    }
}
