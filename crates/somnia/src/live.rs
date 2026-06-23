//! Live queries: a typed notification stream over `LIVE SELECT`.
//!
//! [`SomniaClient::live_select`](crate::SomniaClient::live_select) starts a live
//! query on a record type's table and returns a [`LiveQueryStream`], a
//! [`futures_core::Stream`] of [`Notification`]s carrying the change [`Action`]
//! and the deserialized record. Dropping the stream issues `KILL` for the live
//! query server-side, so simply letting it fall out of scope stops it.
//!
//! ```ignore
//! use futures::StreamExt;
//!
//! let mut stream = client.live_select::<Asset>().await?;
//! while let Some(item) = stream.next().await {
//!     let note = item?;
//!     println!("{:?} -> {:?}", note.action, note.data);
//! }
//! // `stream` dropped here → KILL sent.
//! ```

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use serde::de::DeserializeOwned;
use surrealdb::types::{Action as CoreAction, Value};

pub use uuid::Uuid;

use crate::SomniaError;

/// The change that triggered a live-query [`Notification`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A record was created.
    Create,
    /// A record was updated.
    Update,
    /// A record was deleted.
    Delete,
}

/// A single live-query notification: the change [`Action`] and the deserialized
/// record `T`. `query_id` identifies the originating live query.
#[derive(Debug, Clone)]
pub struct Notification<T> {
    pub query_id: Uuid,
    pub action: Action,
    pub data: T,
}

/// A stream of typed [`Notification`]s from a `LIVE SELECT`, yielding
/// `Result<Notification<T>, SomniaError>`.
///
/// Dropping the stream issues `KILL` for the underlying live query, so there is
/// no explicit teardown call — just let it go out of scope. The stream ends
/// (`None`) when the live query is killed.
#[must_use = "live-query streams are inert until polled; dropping one issues KILL"]
pub struct LiveQueryStream<T> {
    inner: surrealdb::Stream<Vec<Value>>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> LiveQueryStream<T> {
    pub(crate) fn new(inner: surrealdb::Stream<Vec<Value>>) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

impl<T: DeserializeOwned + Unpin> Stream for LiveQueryStream<T> {
    type Item = Result<Notification<T>, SomniaError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Some(Err(SomniaError::Connection(e.to_string()))))
            }
            Poll::Ready(Some(Ok(note))) => {
                let action = match note.action {
                    CoreAction::Create => Action::Create,
                    CoreAction::Update => Action::Update,
                    CoreAction::Delete => Action::Delete,
                    // The live query was killed: end the stream.
                    CoreAction::Killed => return Poll::Ready(None),
                    // The live query's WHERE/projection raised an error; the
                    // payload carries the message as a string.
                    _ => {
                        let msg = match note.data.into_json_value() {
                            serde_json::Value::String(s) => s,
                            other => other.to_string(),
                        };
                        return Poll::Ready(Some(Err(SomniaError::Query(msg))));
                    }
                };
                let query_id = note.query_id.into_inner();
                // Convert through `serde_json::Value` (record ids → strings) so
                // `Thing<T>` and friends deserialize, mirroring `query()`.
                match serde_json::from_value::<T>(note.data.into_json_value()) {
                    Ok(data) => Poll::Ready(Some(Ok(Notification {
                        query_id,
                        action,
                        data,
                    }))),
                    Err(e) => Poll::Ready(Some(Err(SomniaError::Deser(e.to_string())))),
                }
            }
        }
    }
}
