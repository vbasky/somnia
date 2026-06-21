use std::collections::BTreeMap;
use surrealdb::engine::any::{connect, Any};
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

pub use somnia_core::error::SomniaError;
use somnia_core::SurrealRecord;

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
