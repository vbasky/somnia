//! Regression for https://github.com/vbasky/somnia/issues/1:
//! creating a record with a native UUID id must select back as `Key::Uuid`,
//! not `Key::String("u'…'")`.

use somnia::{Key, Returning, SomniaClient, SurrealRecord, Thing};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealRecord)]
#[table("uuid_probe", schemaless)]
struct UuidProbe {
    #[field(thing)]
    id: Thing<UuidProbe>,
}

#[tokio::test]
async fn uuid_record_id_round_trips_as_key_uuid() {
    let client = SomniaClient::connect_anonymous("mem://", "t", "t")
        .await
        .expect("connect");
    client
        .raw("DEFINE TABLE uuid_probe SCHEMALESS;")
        .await
        .expect("define table");

    let id = Uuid::now_v7();
    let create = UuidProbe::table()
        .create()
        .record(id)
        .returning(Returning::None);
    client
        .query::<UuidProbe>(&create)
        .await
        .expect("create with uuid id");

    let rows: Vec<UuidProbe> = client
        .query(&UuidProbe::table().select(UuidProbe::all()))
        .await
        .expect("select");

    assert_eq!(rows.len(), 1, "expected one row, got {rows:?}");
    match &rows[0].id.key {
        Key::Uuid(got) => assert_eq!(*got, id, "selected uuid must match created id"),
        other => panic!("expected Key::Uuid, got {other:?} (issue #1 regression)"),
    }
}

#[tokio::test]
async fn uuid_record_id_from_raw_create_selects_as_key_uuid() {
    // Same wire form as the SDK's `into_json` path for native UUID keys —
    // creates via raw SurrealQL so we don't depend on the builder path alone.
    let client = SomniaClient::connect_anonymous("mem://", "t", "t")
        .await
        .expect("connect");
    client
        .raw("DEFINE TABLE uuid_probe SCHEMALESS;")
        .await
        .expect("define table");

    let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    client
        .raw(&format!("CREATE type::record('uuid_probe', u'{id}');"))
        .await
        .expect("raw create");

    let rows: Vec<UuidProbe> = client
        .query(&UuidProbe::table().select(UuidProbe::all()))
        .await
        .expect("select");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id.key, Key::Uuid(id));
}
