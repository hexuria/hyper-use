//! State one stdio server keeps between tool calls.
//!
//! [`Server`] owns live CDP sessions keyed by endpoint and a bounded ring of
//! recent snapshots. It is driven by one blocking stdin reader, so there is no
//! interleaving. A failed live call drops that session. There is no retry and
//! no reconnect loop; the next call opens a fresh socket. Fixture origins do
//! not keep a transport between calls. Nothing is written to disk.
//!
//! A [`CdpConnector`] opens the transport for a `cdp` endpoint. The default
//! is [`WebSocketTransport::connect`]. Tests pass a connector that returns a
//! [`ultra_instinct_browser::ReplayTransport`], so the live-session paths (reuse,
//! stale observation, eviction, a dropped session) run without Chrome.
//!
//! The server also remembers the last few locate calls so a repeated
//! identical query on an unchanged page can be reported (see `repeat`).

use std::collections::BTreeMap;

use serde_json::Value;
use ultra_instinct_browser::{
    BrowserSession, CdpError, CdpTransport, PageState, WebSocketTransport,
};
use ultra_instinct_core::InteractionManifold;
use ultra_instinct_observe::history::{
    detect, HistoryError, SignedSnapshot, SnapshotId, SnapshotRing, StateSignature, TemporalSignal,
};

use crate::error::ToolError;
use crate::repeat::{LocateRepeats, QueryKey};

/// Live sessions kept at once. Opening another drops the oldest key.
pub const MAX_LIVE_SESSIONS: usize = 4;

pub(crate) struct Snapshot {
    pub(crate) origin: String,
    pub(crate) manifold: InteractionManifold,
    pub(crate) page: PageState,
}

/// Opens the CDP transport for one endpoint.
pub type CdpConnector = Box<dyn FnMut(&str) -> Result<Box<dyn CdpTransport>, CdpError>>;

type LiveSession = BrowserSession<Box<dyn CdpTransport>>;

pub struct Server {
    connector: CdpConnector,
    sessions: BTreeMap<String, LiveSession>,
    session_order: Vec<String>,
    history: SnapshotRing<Snapshot>,
    locate_repeats: LocateRepeats,
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
    /// A server whose `cdp` endpoints are live websockets.
    pub fn new() -> Self {
        Self::with_connector(|url| {
            WebSocketTransport::connect(url)
                .map(|transport| Box::new(transport) as Box<dyn CdpTransport>)
        })
    }

    /// A server whose `cdp` endpoints are opened by `connector`. Used by the
    /// mock environment tests; the stdio binary uses [`Server::new`].
    pub fn with_connector(
        connector: impl FnMut(&str) -> Result<Box<dyn CdpTransport>, CdpError> + 'static,
    ) -> Self {
        Self {
            connector: Box::new(connector),
            sessions: BTreeMap::new(),
            session_order: Vec::new(),
            history: SnapshotRing::default(),
            locate_repeats: LocateRepeats::default(),
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
    #[allow(dead_code)]
    pub(crate) fn temporal_signals(
        &self,
        before: SnapshotId,
        after: SnapshotId,
    ) -> Result<Vec<TemporalSignal>, HistoryError> {
        detect(&self.history, before, after)
    }

    /// Count this locate call among the recent ones with the same origin,
    /// page signature, and normalized query. Data only. This does not retry.
    pub(crate) fn record_locate(
        &mut self,
        origin: &str,
        manifold: &InteractionManifold,
        query: QueryKey,
    ) -> usize {
        self.locate_repeats
            .record(origin, StateSignature::from_manifold(manifold), query)
    }

    /// Newest snapshot recorded for `origin`, if it is still in the ring.
    #[allow(dead_code)]
    pub(crate) fn latest_for(&self, origin: &str) -> Option<SnapshotId> {
        self.history
            .iter_recent()
            .find(|(_, snapshot)| snapshot.origin == origin)
            .map(|(id, _)| id)
    }

    /// The kept session for `url`, or a fresh connection.
    pub(crate) fn take_session(&mut self, url: &str) -> Result<LiveSession, ToolError> {
        if let Some(session) = self.sessions.remove(url) {
            self.session_order.retain(|key| key != url);
            return Ok(session);
        }
        let transport = (self.connector)(url).map_err(|err| ToolError::Browser(err.to_string()))?;
        Ok(BrowserSession::new(transport))
    }

    /// Endpoints with a kept live session, oldest first.
    pub fn live_sessions(&self) -> &[String] {
        &self.session_order
    }

    /// Keep `session` for the next call on `url`.
    pub(crate) fn keep_session(&mut self, url: &str, session: LiveSession) {
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
