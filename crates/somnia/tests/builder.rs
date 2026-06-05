//! Asserts the somnia query builder reproduces the exact SurrealQL the aeris
//! SurrealDB store layer hand-wrote. Each test mirrors a real store query so
//! conversions can be made with confidence that the emitted SQL is unchanged.

#[cfg(test)]
mod tests {
    use somnia::{col, field, ident, Batch, Grouped, NoneLit, Raw, RecordLink, Returning, Thing};
    use somnia_derive::SurrealRecord;

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
}
