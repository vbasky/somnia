//! Live-query tests against the in-memory engine: a `LIVE SELECT` stream should
//! surface create/update/delete notifications with the deserialized record.

use futures::StreamExt;
use somnia::{Action, SomniaClient, SurrealRecord, Thing};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
#[table("widget", schemaless)]
struct Widget {
    #[field(thing)]
    id: Thing<Widget>,
    name: String,
}

#[tokio::test]
async fn live_select_streams_create_update_delete() {
    let client = SomniaClient::connect_anonymous("mem://", "t", "t")
        .await
        .expect("connect");
    client
        .raw("DEFINE TABLE widget SCHEMALESS;")
        .await
        .expect("define table");

    let mut stream = client
        .live_select::<Widget>()
        .await
        .expect("start live query");

    // Drive a create, an update, and a delete on the watched table.
    client
        .raw("CREATE widget:one SET name = 'first';")
        .await
        .expect("create");
    client
        .raw("UPDATE widget:one SET name = 'renamed';")
        .await
        .expect("update");
    client.raw("DELETE widget:one;").await.expect("delete");

    // Collect the three notifications (with a timeout so a hang fails fast).
    let mut actions = Vec::new();
    let mut names = Vec::new();
    for _ in 0..3 {
        let item = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .expect("notification within timeout")
            .expect("stream not ended")
            .expect("notification ok");
        actions.push(item.action);
        names.push(item.data.name.clone());
        assert!(!item.query_id.is_nil(), "query_id should be set");
    }

    assert_eq!(
        actions,
        vec![Action::Create, Action::Update, Action::Delete],
        "expected create/update/delete in order, got {actions:?}"
    );
    // Create + update carry the live value; delete carries the last-known record.
    assert_eq!(names[0], "first");
    assert_eq!(names[1], "renamed");

    // Dropping the stream issues KILL; nothing should panic.
    drop(stream);
}
