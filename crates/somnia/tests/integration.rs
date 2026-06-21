#[cfg(test)]
mod tests {
    use somnia_core::{col, Column, ColumnMeta, ColumnSet, Table, Thing};
    use surrealdb::engine::any::connect;
    use surrealdb::Surreal;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct Asset {
        id: Thing<Asset>,
        name: String,
        file_size: Option<i64>,
        content_type: Option<String>,
    }

    impl somnia_core::SurrealRecord for Asset {
        fn table_name() -> &'static str {
            "asset"
        }
        fn primary_key() -> &'static str {
            "id"
        }
    }

    impl Asset {
        #[allow(dead_code)]
        pub fn id() -> Column<Asset, Thing<Asset>> {
            Column {
                name: "id",
                surreal_type: "record",
                _marker: std::marker::PhantomData,
            }
        }
        pub fn name() -> Column<Asset, String> {
            Column {
                name: "name",
                surreal_type: "string",
                _marker: std::marker::PhantomData,
            }
        }
        pub fn file_size() -> Column<Asset, Option<i64>> {
            Column {
                name: "file_size",
                surreal_type: "option<int>",
                _marker: std::marker::PhantomData,
            }
        }
        pub fn content_type() -> Column<Asset, Option<String>> {
            Column {
                name: "content_type",
                surreal_type: "option<string>",
                _marker: std::marker::PhantomData,
            }
        }
        pub fn all() -> ColumnSet<Self> {
            static COLS: &[ColumnMeta] = &[
                ColumnMeta {
                    name: "id",
                    surreal_type: "record",
                },
                ColumnMeta {
                    name: "name",
                    surreal_type: "string",
                },
                ColumnMeta {
                    name: "file_size",
                    surreal_type: "option<int>",
                },
                ColumnMeta {
                    name: "content_type",
                    surreal_type: "option<string>",
                },
            ];
            ColumnSet {
                cols: COLS,
                _marker: std::marker::PhantomData,
            }
        }
        pub fn table() -> Table<Self> {
            Table::new()
        }
    }

    async fn setup() -> Surreal<surrealdb::engine::any::Any> {
        let db = connect("mem://").await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        // mem:// doesn't require auth — signin is a no-op or skipped
        db.query("DEFINE TABLE asset SCHEMAFULL;").await.unwrap();
        db.query("DEFINE FIELD name ON asset TYPE string;")
            .await
            .unwrap();
        db.query("DEFINE FIELD file_size ON asset TYPE option<int>;")
            .await
            .unwrap();
        db.query("DEFINE FIELD content_type ON asset TYPE option<string>;")
            .await
            .unwrap();
        db
    }

    #[tokio::test]
    async fn test_insert_and_query() {
        let db = setup().await;

        // Insert via SurrealQL
        let sql = "
            INSERT INTO asset { name: 'video.mp4', file_size: 1048576, content_type: 'video/mp4' };
            INSERT INTO asset { name: 'photo.jpg', file_size: 524288, content_type: 'image/jpeg' };
            INSERT INTO asset { name: 'doc.pdf', file_size: 102400, content_type: 'application/pdf' };
        ";
        db.query(sql).await.unwrap();

        // Query with somnia builder
        let sel = Asset::table()
            .select(Asset::all())
            .filter(Asset::content_type().eq(Some("video/mp4".to_string())))
            .limit(1)
            .to_surrealql();

        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "video.mp4");
    }

    #[tokio::test]
    async fn select_extras_run_on_live_surreal() {
        let db = setup().await;
        db.query("INSERT INTO asset { name: 'a.mp4', file_size: 100, content_type: 'video/mp4' };")
            .await
            .unwrap();
        db.query("INSERT INTO asset { name: 'b.mp4', file_size: 200, content_type: 'video/mp4' };")
            .await
            .unwrap();

        // VALUE mode returns bare scalars, not field-wrapping objects.
        let value_sql = Asset::table()
            .project(vec![col("name")])
            .value()
            .to_surrealql();
        let mut res = db.query(&value_sql).await.unwrap().check().unwrap();
        let names: Vec<String> = res.take(0).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a.mp4".to_string()));

        // OMIT + TIMEOUT both parse and run; the omitted field is gone.
        let omit_sql = Asset::table()
            .select(Asset::all())
            .omit("file_size")
            .timeout("5s")
            .to_surrealql();
        let mut res = db.query(&omit_sql).await.unwrap().check().unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(
            rows[0].get("file_size").is_none(),
            "file_size should be omitted"
        );
        assert!(rows[0].get("name").is_some());

        // EXPLAIN returns a plan rather than rows — it just needs to parse + run.
        let explain_sql = Asset::table().select(Asset::all()).explain().to_surrealql();
        db.query(&explain_sql).await.unwrap().check().unwrap();
    }

    #[tokio::test]
    async fn subquery_in_where_runs_on_live_surreal() {
        use somnia_core::ident;
        let db = setup().await;
        db.query("INSERT INTO asset { name: 'big.mp4', file_size: 500 };")
            .await
            .unwrap();
        db.query("INSERT INTO asset { name: 'small.mp4', file_size: 10 };")
            .await
            .unwrap();

        // WHERE name IN (SELECT VALUE name FROM asset WHERE file_size > 100)
        let sub = Asset::table()
            .project(vec![col("name")])
            .value()
            .filter(Asset::file_size().gt(Some(100)));
        let sql = Asset::table()
            .select(Asset::all())
            .filter(ident("name").in_expr(sub))
            .to_surrealql();

        let mut res = db.query(&sql).await.unwrap().check().unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "big.mp4");

        // FROM (<subquery>) also parses and runs.
        let inner = Asset::table()
            .select(Asset::all())
            .filter(Asset::file_size().gt(Some(100)));
        let from_sub = Asset::table()
            .select(Asset::all())
            .from_subquery(inner)
            .to_surrealql();
        db.query(&from_sub).await.unwrap().check().unwrap();
    }

    #[tokio::test]
    async fn test_filter_and_order() {
        let db = setup().await;
        db.query("INSERT INTO asset { name: 'c.mp3', file_size: 100 };")
            .await
            .unwrap();
        db.query("INSERT INTO asset { name: 'a.mp3', file_size: 300 };")
            .await
            .unwrap();
        db.query("INSERT INTO asset { name: 'b.mp3', file_size: 200 };")
            .await
            .unwrap();

        let sel = Asset::table()
            .select(Asset::all())
            .filter(Asset::file_size().gt(Some(50)))
            .order_asc(Asset::name())
            .to_surrealql();

        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["name"], "a.mp3");
        assert_eq!(rows[1]["name"], "b.mp3");
        assert_eq!(rows[2]["name"], "c.mp3");
    }

    #[tokio::test]
    async fn test_count_and_group() {
        let db = setup().await;
        db.query(
            "
            INSERT INTO asset { name: 'a.mp4', content_type: 'video/mp4' };
            INSERT INTO asset { name: 'b.mp4', content_type: 'video/mp4' };
            INSERT INTO asset { name: 'c.jpg', content_type: 'image/jpeg' };
        ",
        )
        .await
        .unwrap();

        // Count all
        let sel = Asset::table().count().to_surrealql();

        assert!(sel.contains("SELECT count()"));

        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        // count() returns a single value
        assert!(!rows.is_empty());
    }

    #[tokio::test]
    async fn test_column_eq_ne_gt_lt() {
        let db = setup().await;
        db.query(
            "
            INSERT INTO asset { name: 'small', file_size: 10 };
            INSERT INTO asset { name: 'medium', file_size: 100 };
            INSERT INTO asset { name: 'large', file_size: 1000 };
        ",
        )
        .await
        .unwrap();

        // Test eq
        let sel = Asset::table()
            .select(Asset::all())
            .filter(Asset::name().eq("medium".to_string()))
            .to_surrealql();
        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "medium");

        // Test gt
        let sel = Asset::table()
            .select(Asset::all())
            .filter(Asset::file_size().gt(Some(50)))
            .to_surrealql();
        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 2);

        // Test ne
        let sel = Asset::table()
            .select(Asset::all())
            .filter(Asset::name().ne("small".to_string()))
            .to_surrealql();
        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn test_combinators_and_or() {
        let db = setup().await;
        db.query(
            "
            INSERT INTO asset { name: 'a', file_size: 10, content_type: 'video/mp4' };
            INSERT INTO asset { name: 'b', file_size: 100, content_type: 'video/mp4' };
            INSERT INTO asset { name: 'c', file_size: 1000, content_type: 'image/jpeg' };
        ",
        )
        .await
        .unwrap();

        // (content_type = 'video/mp4') AND (file_size > 50)
        let sel = Asset::table()
            .select(Asset::all())
            .filter(
                Asset::content_type()
                    .eq(Some("video/mp4".to_string()))
                    .and(Asset::file_size().gt(Some(50))),
            )
            .to_surrealql();

        let mut res = db.query(&sel).await.unwrap();
        let rows: Vec<serde_json::Value> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "b");
    }
}
