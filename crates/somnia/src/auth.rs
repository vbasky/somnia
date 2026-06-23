//! Authentication credentials for [`SomniaClient`](crate::SomniaClient).
//!
//! [`Credentials`] models the signin-capable auth levels SurrealDB exposes —
//! root, namespace, and database users, plus a pre-issued JWT/token. It is
//! consumed by [`SomniaClient::connect_with`](crate::SomniaClient::connect_with)
//! and [`SomniaClient::signin`](crate::SomniaClient::signin).
//!
//! Record (scope) auth is handled separately by
//! [`signin_record`](crate::SomniaClient::signin_record) /
//! [`signup_record`](crate::SomniaClient::signup_record), because it carries a
//! free-form `params` payload and is the only level that supports `SIGNUP`.

/// Credentials for signing in to a SurrealDB connection.
///
/// Mirrors SurrealDB's signin-capable auth levels. Each variant maps to the
/// matching `surrealdb::opt::auth` credential; [`Token`](Credentials::Token)
/// attaches an existing JWT via `authenticate` instead of signing in.
#[derive(Debug, Clone)]
pub enum Credentials {
    /// Root user (`DEFINE USER … ON ROOT`).
    Root { username: String, password: String },
    /// Namespace user (`DEFINE USER … ON NAMESPACE`).
    Namespace {
        namespace: String,
        username: String,
        password: String,
    },
    /// Database user (`DEFINE USER … ON DATABASE`).
    Database {
        namespace: String,
        database: String,
        username: String,
        password: String,
    },
    /// A previously issued JWT/token, attached with `authenticate`.
    Token(String),
}

impl Credentials {
    /// Root credentials.
    pub fn root(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Root {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Namespace-user credentials.
    pub fn namespace(
        namespace: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self::Namespace {
            namespace: namespace.into(),
            username: username.into(),
            password: password.into(),
        }
    }

    /// Database-user credentials.
    pub fn database(
        namespace: impl Into<String>,
        database: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self::Database {
            namespace: namespace.into(),
            database: database.into(),
            username: username.into(),
            password: password.into(),
        }
    }

    /// A pre-issued JWT/token.
    pub fn token(token: impl Into<String>) -> Self {
        Self::Token(token.into())
    }
}
