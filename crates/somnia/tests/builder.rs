//! Asserts the somnia query builder reproduces the exact SurrealQL the aeris
//! SurrealDB store layer hand-wrote. Each test mirrors a real store query so
//! conversions can be made with confidence that the emitted SQL is unchanged.

#[cfg(test)]
mod tests {
    use somnia::{
        col, field, ident, Batch, Grouped, NoneLit, Path, Raw, RecordLink, Returning, SurrealEdge,
        SurrealRecord, Thing,
    };

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("asset_comment")]
    struct AssetComment {
        #[field(thing)]
        id: Thing<AssetComment>,
        body: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("system_settings")]
    struct SystemSetting {
        #[field(thing)]
        id: Thing<SystemSetting>,
        key: String,
        value: Option<serde_json::Value>,
    }

    // Comment projection list, reused across queries.
    fn comment_fields() -> Vec<somnia::Projection> {
        vec![
            field("record::id(id)", "id"),
            field(
                "IF tenant != NONE THEN record::id(tenant) ELSE NONE END",
                "tenant_id",
            ),
            field("record::id(asset)", "asset_id"),
            field("record::id(user)", "user_id"),
            field(
                "IF parent != NONE THEN record::id(parent) ELSE NONE END",
                "parent_id",
            ),
            col("body"),
            col("timecode_seconds"),
            col("is_resolved"),
            field("type::string(created_at)", "created_at"),
            field("type::string(updated_at)", "updated_at"),
        ]
    }

    #[test]
    fn select_with_function_projections_and_record_link_filter() {
        // CommentStore::list_replies
        let sql = AssetComment::table()
            .project(comment_fields())
            .filter(ident("parent").eq_expr(RecordLink::new("asset_comment", "abc".to_string())))
            .order_asc(Raw("created_at".into()))
            .to_surrealql();

        assert_eq!(
            sql,
            "SELECT record::id(id) AS id, IF tenant != NONE THEN record::id(tenant) ELSE NONE END AS tenant_id, record::id(asset) AS asset_id, record::id(user) AS user_id, IF parent != NONE THEN record::id(parent) ELSE NONE END AS parent_id, body, timecode_seconds, is_resolved, type::string(created_at) AS created_at, type::string(updated_at) AS updated_at FROM asset_comment WHERE parent = type::record('asset_comment', 'abc') ORDER BY created_at ASC"
        );
    }

    #[test]
    fn select_count_group_all() {
        // SurrealCommentStore::reply_count
        let sql = AssetComment::table()
            .count("")
            .count_as("c")
            .filter(Raw("parent".into()).and(Raw("1=1".into())))
            .to_surrealql();
        // count_as overrides default count() alias; ensure count() + GROUP ALL render.
        assert!(sql.starts_with("SELECT count() AS c FROM asset_comment WHERE"));
        assert!(sql.ends_with("GROUP ALL"));
    }

    #[test]
    fn delete_record_return_before() {
        // SavedSearchStore::delete style
        let sql = AssetComment::table()
            .delete()
            .filter(ident("id").eq_expr(RecordLink::new("asset_comment", "xyz".to_string())))
            .returning(Returning::Before)
            .to_surrealql();
        assert_eq!(
            sql,
            "DELETE asset_comment WHERE id = type::record('asset_comment', 'xyz') RETURN BEFORE"
        );
    }

    #[test]
    fn update_record_set_then_where() {
        // SET must come before WHERE (SurrealQL ordering).
        let sql = AssetComment::table()
            .update()
            .set_lit("body", "hello".to_string())
            .set_expr("updated_at", Raw("time::now()".into()))
            .filter(ident("id").eq_expr(RecordLink::new("asset_comment", "id1".to_string())))
            .to_surrealql();
        assert_eq!(
            sql,
            "UPDATE asset_comment SET body = 'hello', updated_at = time::now() WHERE id = type::record('asset_comment', 'id1')"
        );
    }

    #[test]
    fn update_merge() {
        // TaxonomyStore::update_fields — UPDATE type::record('taxonomy', $tid) MERGE $content
        let content = serde_json::json!({"name": "x"});
        let sql = SystemSetting::table()
            .update()
            .record("k1".to_string())
            .merge(somnia_core::expr::Literal(content))
            .to_surrealql();
        assert_eq!(
            sql,
            "UPDATE type::record('system_settings', 'k1') MERGE {\"name\":\"x\"}"
        );
    }

    #[test]
    fn create_record_content() {
        // CommentStore::create — CREATE type::record(...) CONTENT {...}
        let content = Raw("{ body: 'hi', is_resolved: false, created_at: time::now() }".into());
        let sql = AssetComment::table()
            .create()
            .record("new1".to_string())
            .content(content)
            .to_surrealql();
        assert_eq!(
            sql,
            "CREATE type::record('asset_comment', 'new1') CONTENT { body: 'hi', is_resolved: false, created_at: time::now() }"
        );
    }

    #[test]
    fn create_set_return_after() {
        // system_setting style: CREATE table SET ... RETURN AFTER
        let sql = SystemSetting::table()
            .create()
            .set_lit("key", "k".to_string())
            .set_expr("value", Raw("{ enabled: true }".into()))
            .returning(Returning::After)
            .to_surrealql();
        assert_eq!(
            sql,
            "CREATE system_settings SET key = 'k', value = { enabled: true } RETURN AFTER"
        );
    }

    #[test]
    fn select_grouped_or_filter() {
        // SavedSearchStore::list_for_user — (user = type::record('user', $id) OR is_shared = true)
        let inner = ident("user")
            .eq_expr(RecordLink::new("user", "u1".to_string()))
            .or(Raw("is_shared = true".into()));
        let sql = SystemSetting::table()
            .project(vec![col("key")])
            .filter(Grouped::new(inner))
            .order_desc(Raw("updated_at".into()))
            .to_surrealql();
        assert_eq!(
            sql,
            "SELECT key FROM system_settings WHERE (user = type::record('user', 'u1') OR is_shared = true) ORDER BY updated_at DESC"
        );
    }

    #[test]
    fn batch_mutate_then_reselect() {
        let create = AssetComment::table()
            .create()
            .record("n".to_string())
            .content(Raw("{ body: 'x' }".into()))
            .to_surrealql();
        let select = AssetComment::table()
            .project(vec![col("body")])
            .filter(ident("id").eq_expr(RecordLink::new("asset_comment", "n".to_string())))
            .limit(1)
            .to_surrealql();
        let batch = Batch::new().push(create).push(select);
        assert_eq!(batch.len(), 2);
        assert_eq!(
            batch.to_surrealql(),
            "CREATE type::record('asset_comment', 'n') CONTENT { body: 'x' };\nSELECT body FROM asset_comment WHERE id = type::record('asset_comment', 'n') LIMIT 1"
        );
    }

    #[test]
    fn create_then_select_joins_with_semicolon() {
        let sql = SystemSetting::table()
            .create()
            .set_lit("key", "k1".to_string())
            .then_select(SystemSetting::table().project(vec![col("key")]).limit(1));
        assert_eq!(
            sql,
            "CREATE system_settings SET key = 'k1';\nSELECT key FROM system_settings LIMIT 1"
        );
    }

    #[test]
    fn update_then_select_joins_with_semicolon() {
        let sql = SystemSetting::table()
            .update()
            .set_lit("key", "k2".to_string())
            .then_select(SystemSetting::table().project(vec![col("key")]).limit(1));
        assert_eq!(
            sql,
            "UPDATE system_settings SET key = 'k2';\nSELECT key FROM system_settings LIMIT 1"
        );
    }

    #[test]
    fn delete_then_select_joins_with_semicolon() {
        let sql = SystemSetting::table()
            .delete()
            .then_select(SystemSetting::table().project(vec![col("key")]).limit(1));
        assert_eq!(
            sql,
            "DELETE system_settings;\nSELECT key FROM system_settings LIMIT 1"
        );
    }

    #[test]
    fn then_select_preserves_mutation_clauses() {
        let sql = SystemSetting::table()
            .create()
            .set_lit("key", "k3".to_string())
            .returning(Returning::After)
            .then_select(SystemSetting::table().project(vec![col("key")]).limit(1));
        assert!(sql.starts_with("CREATE system_settings SET key = 'k3' RETURN AFTER;\nSELECT"));
    }

    #[test]
    fn update_content_upsert() {
        // stats_provider upsert — UPDATE type::record(...) CONTENT {...} RETURN AFTER
        let cfg = serde_json::json!({"enabled": true});
        let sql = SystemSetting::table()
            .update()
            .record("opta".to_string())
            .content(somnia_core::expr::Literal(cfg))
            .returning(Returning::After)
            .to_surrealql();
        assert_eq!(
            sql,
            "UPDATE type::record('system_settings', 'opta') CONTENT {\"enabled\":true} RETURN AFTER"
        );
    }

    #[test]
    fn none_literal_and_is_none() {
        let sql = AssetComment::table()
            .update()
            .set_expr("image_url", NoneLit)
            .filter(Raw("tenant".into()).and(Raw("x = 1".into())))
            .to_surrealql();
        assert!(sql.contains("SET image_url = NONE WHERE"));
    }

    #[test]
    fn datetime_literal_uses_d_prefix() {
        // SurrealDB 2.0+ datetimes require the `d` prefix — a bare quoted string
        // is a `string`, not a `datetime`, and won't compare against the field.
        let dt = chrono::DateTime::parse_from_rfc3339("2023-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let sql = SystemSetting::table()
            .project(vec![col("key")])
            .filter(ident("created_at").gt(dt))
            .to_surrealql();
        assert_eq!(
            sql,
            "SELECT key FROM system_settings WHERE created_at > d'2023-06-01T12:00:00+00:00'"
        );
    }

    #[test]
    fn uuid_literal_uses_u_prefix() {
        // SurrealDB 2.0+ uuids require the `u` prefix.
        let sql = SystemSetting::table()
            .project(vec![col("key")])
            .filter(ident("ext_id").eq(uuid::Uuid::nil()))
            .to_surrealql();
        assert_eq!(
            sql,
            "SELECT key FROM system_settings WHERE ext_id = u'00000000-0000-0000-0000-000000000000'"
        );
    }

    #[test]
    fn thing_literal_escapes_uuid_key() {
        // A UUID record-id key has dashes; it must be backtick-quoted or it parses
        // as an arithmetic expression rather than a record id.
        let t: Thing<SystemSetting> = Thing::new("550e8400-e29b-41d4-a716-446655440000");
        let sql = SystemSetting::table()
            .project(vec![col("ref")])
            .filter(ident("ref").eq(t))
            .to_surrealql();
        assert_eq!(
            sql,
            "SELECT ref FROM system_settings WHERE ref = system_settings:`550e8400-e29b-41d4-a716-446655440000`"
        );
    }

    #[test]
    fn insert_renders_record_inline() {
        // INSERT serializes the record inline as an object literal — no unbound $data.
        let row = SystemSetting {
            id: Thing::new("k1"),
            key: "theme".to_string(),
            value: None,
        };
        let sql = SystemSetting::table().insert().content(row).to_surrealql();
        assert_eq!(
            sql,
            r#"INSERT INTO system_settings {"id":"system_settings:k1","key":"theme","value":null}"#
        );
    }

    #[test]
    fn upsert_renders_upsert_keyword() {
        // UPSERT: update the record, or create it if absent. Same builder as UPDATE.
        let sql = SystemSetting::table()
            .upsert()
            .record("k1".to_string())
            .set_lit("key", "theme".to_string())
            .returning(Returning::After)
            .to_surrealql();
        assert_eq!(
            sql,
            "UPSERT type::record('system_settings', 'k1') SET key = 'theme' RETURN AFTER"
        );
    }

    #[test]
    fn upsert_table_with_filter_and_merge() {
        let sql = SystemSetting::table()
            .upsert()
            .merge(Raw("{ enabled: true }".into()))
            .filter(ident("key").eq("theme".to_string()))
            .to_surrealql();
        assert_eq!(
            sql,
            "UPSERT system_settings MERGE { enabled: true } WHERE key = 'theme'"
        );
    }

    // ─── Graph traversal (Path) ─────────────────────────────────────────────────

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("user")]
    struct User {
        #[field(thing)]
        id: Thing<User>,
        name: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("post")]
    struct Post {
        #[field(thing)]
        id: Thing<Post>,
        title: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
    #[table("wrote")]
    struct Wrote {
        #[field(thing)]
        id: Thing<Wrote>,
    }
    impl SurrealEdge for Wrote {
        fn edge_name() -> &'static str {
            "wrote"
        }
    }

    fn render(p: Path) -> String {
        use somnia::DynExpr;
        let mut buf = String::new();
        p.render_dyn(&mut buf);
        buf
    }

    #[test]
    fn path_out_to_table() {
        assert_eq!(render(Path::out::<Wrote>().to::<Post>()), "->wrote->post");
    }

    #[test]
    fn path_in_and_both() {
        assert_eq!(render(Path::inn::<Wrote>().to::<User>()), "<-wrote<-user");
        assert_eq!(
            render(Path::both::<Wrote>().to::<Post>()),
            "<->wrote<->post"
        );
    }

    #[test]
    fn path_field_and_all_accessors() {
        assert_eq!(
            render(Path::out::<Wrote>().to::<Post>().field("title")),
            "->wrote->post.title"
        );
        assert_eq!(
            render(Path::out::<Wrote>().to::<Post>().all()),
            "->wrote->post.*"
        );
    }

    #[test]
    fn path_edge_only_no_dest() {
        assert_eq!(render(Path::out::<Wrote>()), "->wrote");
    }

    #[test]
    fn path_multi_hop() {
        // ->wrote->post<-wrote<-user : a post's co-authors
        let p = Path::out::<Wrote>()
            .to::<Post>()
            .then_in::<Wrote>()
            .to::<User>();
        assert_eq!(render(p), "->wrote->post<-wrote<-user");
    }

    #[test]
    fn path_edge_where_filter() {
        let p = Path::out_edge("reacted_to")
            .where_(ident("kind").eq("celebrate".to_string()))
            .to_table("post");
        assert_eq!(render(p), "->(reacted_to WHERE kind = 'celebrate')->post");
    }

    #[test]
    fn path_from_record_anchor() {
        let tobie: Thing<User> = Thing::new("tobie");
        let p = Path::out::<Wrote>().to::<Post>().from_record(tobie);
        assert_eq!(render(p), "user:tobie->wrote->post");
    }

    #[test]
    fn path_as_select_projection() {
        // SELECT ->wrote->post.title AS titles FROM user
        let sql = User::table()
            .project_path(Path::out::<Wrote>().to::<Post>().field("title"), "titles")
            .to_surrealql();
        assert_eq!(sql, "SELECT ->wrote->post.title AS titles FROM user");
    }

    #[test]
    fn path_with_star_projection() {
        // SELECT *, ->wrote->post AS posts FROM user
        let sql = User::table()
            .select(User::all())
            .with_path(Path::out::<Wrote>().to::<Post>(), "posts")
            .to_surrealql();
        assert_eq!(sql, "SELECT *, ->wrote->post AS posts FROM user");
    }

    #[test]
    fn path_explicit_projection_then_path() {
        // project([...]) then with_path appends without injecting a `*`.
        let sql = User::table()
            .project(vec![col("name")])
            .with_path(Path::out::<Wrote>().to::<Post>(), "posts")
            .to_surrealql();
        assert_eq!(sql, "SELECT name, ->wrote->post AS posts FROM user");
    }

    #[test]
    fn path_in_where_filter() {
        // SELECT name FROM user WHERE ->wrote->post CONTAINS post:p1
        let p1: Thing<Post> = Thing::new("p1");
        let sql = User::table()
            .project(vec![col("name")])
            .filter(Path::out::<Wrote>().to::<Post>().contains(p1))
            .to_surrealql();
        assert_eq!(
            sql,
            "SELECT name FROM user WHERE ->wrote->post CONTAINS post:p1"
        );
    }

    #[test]
    fn path_combinators_and() {
        // Path supports .and()/.or() via the shared combinator macro.
        let p = Path::out::<Wrote>()
            .to::<Post>()
            .field("title")
            .eq_expr(Raw("'hi'".into()))
            .and(Raw("1 = 1".into()));
        let mut buf = String::new();
        use somnia::DynExpr;
        p.render_dyn(&mut buf);
        assert_eq!(buf, "->wrote->post.title = 'hi' AND 1 = 1");
    }
}
