//! What the benchmark targets share: the calls they make, and the in-memory socket they make them
//! through.

use serde::{Deserialize, Serialize};
use zlink_core::connection::socket::{ReadHalf, Socket, WriteHalf};

pub const NUM_CALLS: usize = 20;

// Method definitions for benchmarking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
pub enum TestMethod {
    Ping { id: u32 },
    Compute { values: Vec<u32> },
}

// Bidirectional mock socket for in-memory communication.
#[derive(Debug)]
pub struct BiPipeSocket {
    client_to_server: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    server_to_client: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

impl BiPipeSocket {
    pub fn new_pair() -> (Self, Self) {
        let (c2s_tx, c2s_rx) = tokio::sync::mpsc::unbounded_channel();
        let (s2c_tx, s2c_rx) = tokio::sync::mpsc::unbounded_channel();

        let client = BiPipeSocket {
            client_to_server: c2s_tx,
            server_to_client: s2c_rx,
        };

        let server = BiPipeSocket {
            client_to_server: s2c_tx,
            server_to_client: c2s_rx,
        };

        (client, server)
    }
}

impl Socket for BiPipeSocket {
    type ReadHalf = BiPipeReadHalf;
    type WriteHalf = BiPipeWriteHalf;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        (
            BiPipeReadHalf {
                receiver: self.server_to_client,
                buffer: Vec::new(),
                pos: 0,
            },
            BiPipeWriteHalf {
                sender: self.client_to_server,
            },
        )
    }
}

#[derive(Debug)]
pub struct BiPipeReadHalf {
    receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    buffer: Vec<u8>,
    pos: usize,
}

impl ReadHalf for BiPipeReadHalf {
    async fn read(
        &mut self,
        buf: &mut [u8],
    ) -> zlink_core::Result<zlink_core::connection::socket::ReadResult> {
        // If we have buffered data, return it.
        if self.pos < self.buffer.len() {
            let to_read = (self.buffer.len() - self.pos).min(buf.len());
            buf[..to_read].copy_from_slice(&self.buffer[self.pos..self.pos + to_read]);
            self.pos += to_read;
            return Ok(zlink_core::connection::socket::ReadResult::new(to_read));
        }

        // Otherwise, wait for new data.
        match self.receiver.recv().await {
            Some(data) => {
                self.buffer = data;
                self.pos = 0;
                let to_read = self.buffer.len().min(buf.len());
                buf[..to_read].copy_from_slice(&self.buffer[..to_read]);
                self.pos = to_read;
                Ok(zlink_core::connection::socket::ReadResult::new(to_read))
            }
            None => {
                // Connection closed.
                Ok(zlink_core::connection::socket::ReadResult::new(0))
            }
        }
    }
}

#[derive(Debug)]
pub struct BiPipeWriteHalf {
    sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl WriteHalf for BiPipeWriteHalf {
    async fn write(
        &mut self,
        buf: &[u8],
        #[cfg(feature = "std")] _fds: &[impl std::os::fd::AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] _credentials: Option<
            &zlink_core::connection::PassedCredentials,
        >,
    ) -> zlink_core::Result<()> {
        self.sender
            .send(buf.to_vec())
            .map_err(|_| zlink_core::Error::UnexpectedEof)?;
        Ok(())
    }
}
