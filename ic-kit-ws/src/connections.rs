use ic_kit::candid::parser::token::Token::Vec;
use ic_kit_certified::{label::Label, AsHashTree, Hash, Map, Seq};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::thread::sleep;

/// How many blocks should we keep in memory before considering them as garbage.
const MAX_BLOCK_HEIGHT: usize = 32;

/// An specifier for a certain block execution on the IC.
type BlockId = u64;

/// The internal representation of a connection.
type ConnectionIdInternal = u64;

pub type RawMessage = Vec<u8>;

/// Provides the connection.
pub struct WsConnections {
    connections: VecDeque<(BlockId, Map<ConnectionIdInternal, Seq<RawMessage>>)>,
    hash: Hash,
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct ConnectionId(ConnectionIdInternal);

impl WsConnections {
    /// Create a new connection manager.
    pub fn new() -> Self {
        let mut ws = Self {
            connections: VecDeque::with_capacity(MAX_BLOCK_HEIGHT),
            hash: [0; 32],
        };
        ws.recompute_hash();
        ws
    }

    /// Recompute the root hash and calls set_certified_data.
    fn recompute_hash(&mut self) {
        let mut hasher = Sha256::new();
        for (block_id, tree) in &self.connections {
            hasher.update(&block_id.to_be_bytes());
            hasher.update(&tree.root_hash());
        }
        let root_hash: Hash = hasher.finalize().into();
        self.hash = root_hash;

        // this can become user's responsibility.
        ic_kit::ic::set_certified_data(&root_hash);
    }

    pub fn send_raw<I: IntoIterator<Item = (ConnectionId, RawMessage)>>(&mut self, messages: I) {
        let block_id = get_current_block_id();
        let connections_len = self.connections.len();

        // ensure that the last connection in the list is for the current block_id.
        if connections_len == 0 || self.connections[connections_len - 1].0 != block_id {
            // remove the first element from the vector to never grow past the capacity.
            if connections_len == MAX_BLOCK_HEIGHT {
                self.connections.pop_back();
            }

            self.connections.push_front((block_id, Map::default()));
        }

        for (connection_id, message) in messages {
            self.connections[self.connections.len() - 1]
                .1
                .entry(connection_id.0)
                .or_default()
                .append(message);
        }

        self.recompute_hash();
    }

    /// Close a connection.
    pub fn close_connections<I: IntoIterator<Item = ConnectionId>>(&mut self, connections: I) {
        for (_, mut tree) in self.connections {
            for connection_id in connections {
                tree.remove(&connection_id.0);
            }
        }

        self.recompute_hash();
    }
}

/// Return an increasing numeric identifier for the current block.
fn get_current_block_id() -> BlockId {
    // ic9.time() returns the same value for the entire execution during a single entry point, this
    // guarantees that this function is also at least going to return the same value when invoked
    // throughout a single update call.
    const BLOCK_PERIOD_SECONDS: usize = 3;
    let time_seconds = ic_kit::ic::time() / 1_000_000;
    (time_seconds / BLOCK_PERIOD_SECONDS) * BLOCK_PERIOD_SECONDS
}
