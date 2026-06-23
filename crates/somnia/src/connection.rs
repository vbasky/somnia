use std::collections::BTreeMap;
use surrealdb::engine::any::{connect, Any};
use surrealdb::opt::auth::{Database, Namespace, Record, Root, Token};
use surrealdb::Surreal;

pub use somnia_core::error::SomniaError;
use somnia_core::SurrealRecord;

use crate::auth::Credentials;

/// A SurrealDB connection with typed query execution.
pub struct SomniaClient {
    inner: Surreal<Any>,
}

impl SomniaClient {
    /// Connect to a SurrealDB `endpoint` (e.g. `ws://localhost:8000`), sign in with
    /// root credentials, and select the namespace `ns` and database `db`.
    pub async fn connect(
        endpoint: &str,
        user: &str,
        pass: &str,
        ns: &str,
        db: &str,
    ) -> Result<Self, SomniaError> {
        let surreal = connect(endpoint)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        surreal
            .signin(Root {
                username: user.to_string(),
                password: pass.to_string(),
            })
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        surreal
            .use_ns(ns.to_string())
            .use_db(db.to_string())
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        Ok(Self { inner: surreal })
    }

    /// Connect to `endpoint`, authenticate with `creds`, and select the
    /// namespace `ns` and database `db`.
    ///
    /// The typed counterpart to [`connect`](Self::connect): rather than always
    /// signing in as root, it accepts any [`Credentials`] level (root,
    /// namespace, database, or a pre-issued [`Token`](Credentials::Token)).
    pub async fn connect_with(
        endpoint: &str,
        ns: &str,
        db: &str,
        creds: Credentials,
    ) -> Result<Self, SomniaError> {
        let surreal = connect(endpoint)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        let client = Self { inner: surreal };
        client.signin(&creds).await?;
        client
            .inner
            .use_ns(ns.to_string())
            .use_db(db.to_string())
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        Ok(client)
    }

    /// Connect to `endpoint` and select `ns`/`db` without authenticating.
    ///
    /// Useful for embedded engines (`mem://`, `rocksdb://…`) that run with auth
    /// disabled, or when you intend to authenticate later via
    /// [`signin_record`](Self::signin_record) / [`authenticate`](Self::authenticate).
    pub async fn connect_anonymous(
        endpoint: &str,
        ns: &str,
        db: &str,
    ) -> Result<Self, SomniaError> {
        let surreal = connect(endpoint)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        surreal
            .use_ns(ns.to_string())
            .use_db(db.to_string())
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        Ok(Self { inner: surreal })
    }

    /// Sign in on the current connection with the given [`Credentials`],
    /// returning the issued access token (empty for embedded engines that do
    /// not mint one). For [`Token`](Credentials::Token) this attaches the JWT
    /// via `authenticate` instead.
    pub async fn signin(&self, creds: &Credentials) -> Result<String, SomniaError> {
        let token = match creds {
            Credentials::Root { username, password } => {
                self.inner
                    .signin(Root {
                        username: username.clone(),
                        password: password.clone(),
                    })
                    .await
            }
            Credentials::Namespace {
                namespace,
                username,
                password,
            } => {
                self.inner
                    .signin(Namespace {
                        namespace: namespace.clone(),
                        username: username.clone(),
                        password: password.clone(),
                    })
                    .await
            }
            Credentials::Database {
                namespace,
                database,
                username,
                password,
            } => {
                self.inner
                    .signin(Database {
                        namespace: namespace.clone(),
                        database: database.clone(),
                        username: username.clone(),
                        password: password.clone(),
                    })
                    .await
            }
            Credentials::Token(token) => self.inner.authenticate(token.as_str()).await,
        }
        .map_err(|e| SomniaError::Auth(e.to_string()))?;
        Ok(access_token(token))
    }

    /// Sign in to a record-access (scope) method, returning the issued token.
    ///
    /// `params` is any serializable value (e.g. a struct or
    /// `serde_json::json!({ "email": …, "pass": … })`) whose fields are
    /// flattened into the `SIGNIN` query's `$params`.
    pub async fn signin_record<P>(
        &self,
        ns: &str,
        db: &str,
        access: &str,
        params: &P,
    ) -> Result<String, SomniaError>
    where
        P: serde::Serialize,
    {
        let params = serde_json::to_value(params)?;
        let token = self
            .inner
            .signin(Record {
                namespace: ns.to_string(),
                database: db.to_string(),
                access: access.to_string(),
                params,
            })
            .await
            .map_err(|e| SomniaError::Auth(e.to_string()))?;
        Ok(access_token(token))
    }

    /// Sign up a new record-access (scope) user, returning the issued token.
    ///
    /// Like [`signin_record`](Self::signin_record) but runs the access method's
    /// `SIGNUP` query.
    pub async fn signup_record<P>(
        &self,
        ns: &str,
        db: &str,
        access: &str,
        params: &P,
    ) -> Result<String, SomniaError>
    where
        P: serde::Serialize,
    {
        let params = serde_json::to_value(params)?;
        let token = self
            .inner
            .signup(Record {
                namespace: ns.to_string(),
                database: db.to_string(),
                access: access.to_string(),
                params,
            })
            .await
            .map_err(|e| SomniaError::Auth(e.to_string()))?;
        Ok(access_token(token))
    }

    /// Attach a previously issued JWT/token to this connection for subsequent
    /// requests.
    pub async fn authenticate(&self, token: &str) -> Result<(), SomniaError> {
        self.inner
            .authenticate(token)
            .await
            .map_err(|e| SomniaError::Auth(e.to_string()))?;
        Ok(())
    }

    /// Invalidate the current session's authentication, reverting to an
    /// unauthenticated connection.
    pub async fn invalidate(&self) -> Result<(), SomniaError> {
        self.inner
            .invalidate()
            .await
            .map_err(|e| SomniaError::Auth(e.to_string()))?;
        Ok(())
    }

    /// Run a SurrealQL statement (typically a builder's `to_surrealql()`) and
    /// deserialize the first result set into `Vec<T>`.
    pub async fn query<T>(&self, q: &(impl ToString + ?Sized)) -> Result<Vec<T>, SomniaError>
    where
        T: SurrealRecord + serde::de::DeserializeOwned,
    {
        let surql = q.to_string();
        tracing::debug!(query = %surql, "executing");
        let mut res = self
            .inner
            .query(&surql)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        let rows: Vec<serde_json::Value> =
            res.take(0).map_err(|e| SomniaError::Deser(e.to_string()))?;
        rows.into_iter()
            .map(|v| {
                let row: T = serde_json::from_value(v)?;
                Ok::<T, SomniaError>(row)
            })
            .collect::<Result<Vec<T>, SomniaError>>()
    }

    /// Like [`query`](Self::query) but accepts a `(sql, params)` tuple from
    /// [`to_surrealql_with_params`](somnia_core::query::Select::to_surrealql_with_params),
    /// binding each parameter to the SurrealDB query.
    pub async fn query_with_params<T>(
        &self,
        surql: &str,
        params: &BTreeMap<String, serde_json::Value>,
    ) -> Result<Vec<T>, SomniaError>
    where
        T: SurrealRecord + serde::de::DeserializeOwned,
    {
        tracing::debug!(query = %surql, params = ?params.len(), "executing with params");
        let mut q = self.inner.query(surql);
        for (name, value) in params {
            q = q.bind((name.as_str(), value.clone()));
        }
        let mut res = q
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        let rows: Vec<serde_json::Value> =
            res.take(0).map_err(|e| SomniaError::Deser(e.to_string()))?;
        rows.into_iter()
            .map(|v| {
                let row: T = serde_json::from_value(v)?;
                Ok::<T, SomniaError>(row)
            })
            .collect::<Result<Vec<T>, SomniaError>>()
    }

    /// Execute an [`Insert`](somnia_core::query::Insert), binding each queued
    /// record as `$data`, and return the created rows.
    pub async fn insert<T>(
        &self,
        insert: &somnia_core::query::Insert<T>,
    ) -> Result<Vec<T>, SomniaError>
    where
        T: SurrealRecord + serde::de::DeserializeOwned + serde::Serialize,
    {
        let mut results = Vec::new();
        for record in insert.data() {
            let json = serde_json::to_value(record)?;
            let q = format!("INSERT INTO {} $data RETURN AFTER", T::table_name());
            let mut res = self
                .inner
                .query(&q)
                .bind(("data", json))
                .await
                .map_err(|e| SomniaError::Connection(e.to_string()))?;
            let rows: Vec<serde_json::Value> =
                res.take(0).map_err(|e| SomniaError::Deser(e.to_string()))?;
            if let Some(row) = rows.into_iter().next() {
                results.push(serde_json::from_value(row)?);
            }
        }
        Ok(results)
    }

    /// Execute an [`Update`](somnia_core::query::Update) and return the affected rows.
    pub async fn update<T>(
        &self,
        update: &somnia_core::query::Update<T>,
    ) -> Result<Vec<T>, SomniaError>
    where
        T: SurrealRecord + serde::de::DeserializeOwned,
    {
        let surql = update.to_surrealql();
        let mut res = self
            .inner
            .query(&surql)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        let rows: Vec<serde_json::Value> =
            res.take(0).map_err(|e| SomniaError::Deser(e.to_string()))?;
        rows.into_iter()
            .map(|v| {
                let row: T = serde_json::from_value(v)?;
                Ok::<T, SomniaError>(row)
            })
            .collect::<Result<Vec<T>, SomniaError>>()
    }

    /// Execute a [`Delete`](somnia_core::query::Delete).
    pub async fn delete<T>(&self, delete: &somnia_core::query::Delete<T>) -> Result<(), SomniaError>
    where
        T: SurrealRecord,
    {
        let surql = delete.to_surrealql();
        self.inner
            .query(&surql)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        Ok(())
    }

    /// Build a [`Migrator`](crate::migrate::Migrator) for a migrations directory,
    /// sharing this client's connection.
    pub fn migrator(&self, dir: impl Into<std::path::PathBuf>) -> crate::migrate::Migrator {
        crate::migrate::Migrator::new(self.inner.clone(), dir)
    }

    /// Start a `LIVE SELECT` over `T`'s table, returning a typed stream of change
    /// [`Notification`](crate::live::Notification)s. The stream yields
    /// `Result<Notification<T>, SomniaError>` as records are created, updated, or
    /// deleted; dropping it issues `KILL` for the live query server-side.
    ///
    /// Requires a streaming connection (the embedded `mem://`/`rocksdb://`
    /// engines and the `ws://` protocol support live queries; plain HTTP does
    /// not).
    pub async fn live_select<T>(&self) -> Result<crate::live::LiveQueryStream<T>, SomniaError>
    where
        T: SurrealRecord + serde::de::DeserializeOwned + Unpin,
    {
        let stream: surrealdb::Stream<Vec<surrealdb::types::Value>> = self
            .inner
            .select(T::table_name())
            .live()
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        Ok(crate::live::LiveQueryStream::new(stream))
    }

    /// Run raw SurrealQL and return the first result set as untyped JSON values —
    /// the escape hatch for statements the typed helpers don't cover.
    pub async fn raw(&self, surql: &str) -> Result<Vec<serde_json::Value>, SomniaError> {
        let mut res = self
            .inner
            .query(surql)
            .await
            .map_err(|e| SomniaError::Connection(e.to_string()))?;
        res.take(0).map_err(|e| SomniaError::Deser(e.to_string()))
    }
}

/// Extract the access-token string from a SurrealDB auth [`Token`].
fn access_token(token: Token) -> String {
    token.access.as_insecure_token().to_string()
}
