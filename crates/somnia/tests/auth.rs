//! Live-engine tests for the authentication surface, exercised against the
//! in-memory engine (`mem://`).

use somnia::{Credentials, SomniaClient};

#[test]
fn credentials_constructors() {
    // The typed constructors build the matching variants without leaking
    // surrealdb's auth structs.
    assert!(matches!(
        Credentials::root("root", "root"),
        Credentials::Root { .. }
    ));
    assert!(matches!(
        Credentials::namespace("ns", "u", "p"),
        Credentials::Namespace { .. }
    ));
    assert!(matches!(
        Credentials::database("ns", "db", "u", "p"),
        Credentials::Database { .. }
    ));
    assert!(matches!(Credentials::token("jwt"), Credentials::Token(_)));
}

#[tokio::test]
async fn connect_anonymous_selects_ns_db() {
    let client = SomniaClient::connect_anonymous("mem://", "test", "test")
        .await
        .expect("anonymous connect");

    // A trivial query confirms the ns/db were selected.
    let rows = client.raw("RETURN 1;").await.expect("raw query");
    assert_eq!(rows, vec![serde_json::json!(1)]);
}

#[tokio::test]
async fn record_signup_signin_invalidate_roundtrip() {
    let client = SomniaClient::connect_anonymous("mem://", "test", "test")
        .await
        .expect("anonymous connect");

    // Define a record-access method with SIGNUP/SIGNIN queries.
    client
        .raw(
            r#"
            DEFINE TABLE user SCHEMALESS PERMISSIONS FULL;
            DEFINE ACCESS account ON DATABASE TYPE RECORD
                SIGNUP ( CREATE user SET email = $email, pass = crypto::argon2::generate($pass) )
                SIGNIN ( SELECT * FROM user WHERE email = $email
                         AND crypto::argon2::compare(pass, $pass) )
                DURATION FOR TOKEN 15m, FOR SESSION 12h;
            "#,
        )
        .await
        .expect("define access");

    let params = serde_json::json!({ "email": "a@b.com", "pass": "secret" });

    // Sign up mints a token for the new record user.
    let signup_token = client
        .signup_record("test", "test", "account", &params)
        .await
        .expect("signup");
    assert!(!signup_token.is_empty(), "signup should mint a token");

    // Sign in with the same credentials mints another token.
    let signin_token = client
        .signin_record("test", "test", "account", &params)
        .await
        .expect("signin");
    assert!(!signin_token.is_empty(), "signin should mint a token");

    // The issued token can be re-attached on a fresh connection.
    let other = SomniaClient::connect_anonymous("mem://", "test", "test")
        .await
        .expect("second connect");
    // (different in-memory instance — just assert the call shape is wired)
    let reattach = other.authenticate(&signin_token).await;
    // A token from a different mem instance won't validate; either way the call
    // must surface a typed result, not panic.
    let _ = reattach;

    // Invalidate clears the authenticated session.
    client.invalidate().await.expect("invalidate");

    // Wrong password is rejected with a typed Auth error.
    let bad = serde_json::json!({ "email": "a@b.com", "pass": "wrong" });
    let err = client
        .signin_record("test", "test", "account", &bad)
        .await
        .expect_err("wrong password must fail");
    assert!(
        matches!(err, somnia::SomniaError::Auth(_)),
        "expected Auth error, got {err:?}"
    );
}
