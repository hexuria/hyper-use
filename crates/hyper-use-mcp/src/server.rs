//! State one stdio server keeps between tool calls.
//!
//! [`Server`] owns live CDP sessions keyed by endpoint and a bounded ring of
//! recent snapshots. It is driven by one blocking stdin reader, so there is no
//! interleaving. A failed live call drops that session. There is no retry and
//! no reconnect loop; the next call opens a fresh socket. Fixture origins do
//! not keep a transport between calls. Nothing is written to disk.

use std::collections::BTreeMap;

use hyper_use_browser::{BrowserSession, PageState, WebSocketTransport};
use hyper_use_core::InteractionManifold;
use hyper_use_observe::history::{
    detect, HistoryError, SignedSnapshot, SnapshotId, SnapshotRing, StateSignature, TemporalSignal,
};
use serde_json::Value;

use crate::error::ToolError;

/// Live sessions kept at once. Opening another drops the oldest key.
pub const MAX_LIVE_SESSIONS: usize = 4;

pub(crate) struct Snapshot {
    pub(crate) origin: String,
    pub(crate) manifold: InteractionManifold,
    pub(crate) page: PageState,
}

pub struct Server {
    sessions: BTreeMap<String, BrowserSession<WebSocketTransport>>,
    session_order: Vec<String>,
    history: SnapshotRing<Snapshot>,
}

impl SignedSnapshot for Snapshot {
    fn origin(&self) -> &str {
        &self.origin
    }

    fn state_signature(&self) -> StateSignature {
        StateSignature::from_manifold(&self.manifold)
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            sessions: BTreeMap::new(),
            session_order: Vec::new(),
            history: SnapshotRing::default(),
        }
    }

    /// One inbound JSON-RPC line. `None` means the client must not be answered.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        crate::rpc::handle_line_with(self, line)
    }

    /// Run one tool against this server's state.
    pub fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<Value, ToolError> {
        crate::tools::dispatch(self, name, arguments)
    }

    pub(crate) fn record(
        &mut self,
        origin: &str,
        manifold: InteractionManifold,
        page: PageState,
    ) -> SnapshotId {
        self.history.push(Snapshot {
            origin: origin.to_owned(),
            manifold,
            page,
        })
    }

    pub(crate) fn snapshot(&self, id: u64) -> Result<&Snapshot, ToolError> {
        self.history
            .get(SnapshotId::new(id))
            .map_err(|err| match err {
                HistoryError::Evicted { id, oldest } => ToolError::SnapshotEvicted {
                    id: id.get(),
                    oldest: oldest.get(),
                },
                HistoryError::Unknown(id) => ToolError::UnknownSnapshot(id.get()),
                other => ToolError::Browser(other.to_string()),
            })
    }

    /// Signature no-op and loop signals for an act pair already in the ring.
    /// Data only. This does not retry.
    pub(crate) fn temporal_signals(
        &self,
        before: SnapshotId,
        after: SnapshotId,
    ) -> Result<Vec<TemporalSignal>, HistoryError> {
        detect(&self.history, before, after)
    }

    /// Newest snapshot recorded for `origin`, if it is still in the ring.
    pub(crate) fn latest_for(&self, origin: &str) -> Option<SnapshotId> {
        self.history
            .iter_recent()
            .find(|(_, snapshot)| snapshot.origin == origin)
            .map(|(id, _)| id)
    }

    /// The kept session for `url`, or a fresh connection.
    pub(crate) fn take_session(
        &mut self,
        url: &str,
    ) -> Result<BrowserSession<WebSocketTransport>, ToolError> {
        if let Some(session) = self.sessions.remove(url) {
            self.session_order.retain(|key| key != url);
            return Ok(session);
        }
        let transport =
            WebSocketTransport::connect(url).map_err(|err| ToolError::Browser(err.to_string()))?;
        Ok(BrowserSession::new(transport))
    }

    /// Keep `session` for the next call on `url`.
    pub(crate) fn keep_session(&mut self, url: &str, session: BrowserSession<WebSocketTransport>) {
        if self.sessions.len() >= MAX_LIVE_SESSIONS && !self.sessions.contains_key(url) {
            let oldest = self.session_order.remove(0);
            self.sessions.remove(&oldest);
        }
        self.session_order.retain(|key| key != url);
        self.session_order.push(url.to_owned());
        self.sessions.insert(url.to_owned(), session);
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
