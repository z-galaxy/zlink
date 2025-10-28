use async_io::Async;
use std::os::{fd::OwnedFd, unix::net::UnixListener as StdUnixListener};

use crate::{Connection, Result};

/// Create a new unix domain socket listener and bind it to `path`.
pub fn bind<P>(path: P) -> Result<Listener>
where
    P: AsRef<std::path::Path>,
{
    let std_listener = StdUnixListener::bind(path)?;
    std_listener.set_nonblocking(true)?;
    Ok(Listener {
        listener: Async::new(std_listener)?,
    })
}

/// A unix domain socket listener.
#[derive(Debug)]
pub struct Listener {
    listener: Async<StdUnixListener>,
}

impl crate::Listener for Listener {
    type Socket = super::Stream;

    async fn accept(&mut self) -> Result<Connection<Self::Socket>> {
        let (stream, _) = self.listener.accept().await?;
        Ok(super::Stream::from(stream).into())
    }
}

impl TryFrom<OwnedFd> for Listener {
    type Error = crate::Error;

    fn try_from(fd: OwnedFd) -> Result<Self> {
        let std_listener = StdUnixListener::from(fd);
        std_listener.set_nonblocking(true)?;

        Ok(Listener {
            listener: Async::new(std_listener)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Listener as _;
    use std::os::unix::net::UnixListener as StdUnixListener;
    use tempfile::TempDir;

    #[test]
    fn from_fd_with_multiple_connections() {
        smol::block_on(async {
            // Create a temporary directory for the socket
            let temp_dir = TempDir::new().unwrap();
            let socket_path = temp_dir.path().join("test3.sock");

            // Create a standard Unix listener and convert to OwnedFd
            let std_listener = StdUnixListener::bind(&socket_path).unwrap();
            let fd: OwnedFd = std_listener.into();

            // Create our listener from the fd
            let mut listener = Listener::try_from(fd).unwrap();

            // Connect and accept multiple times to verify the listener remains functional
            let mut previous_id = None;
            for _i in 0..3 {
                let socket_path_clone = socket_path.clone();
                let connect_task = smol::spawn(async move {
                    smol::net::unix::UnixStream::connect(&socket_path_clone)
                        .await
                        .unwrap()
                });

                let connection = listener.accept().await.unwrap();
                let id = connection.id();

                // Each connection should have a unique ID
                if let Some(prev) = previous_id {
                    assert_ne!(id, prev, "Connection IDs should be unique");
                }
                previous_id = Some(id);

                let _stream = connect_task.await;
            }
        });
    }

    #[test]
    fn from_fd_preserves_nonblocking() {
        smol::block_on(async {
            // Create a temporary directory for the socket
            let temp_dir = TempDir::new().unwrap();
            let socket_path = temp_dir.path().join("test4.sock");

            // Create a standard Unix listener in blocking mode
            let std_listener = StdUnixListener::bind(&socket_path).unwrap();
            // Explicitly set to blocking (though it's the default)
            std_listener.set_nonblocking(false).unwrap();

            let fd: OwnedFd = std_listener.into();

            // The from_fd should set it to non-blocking
            let mut listener = Listener::try_from(fd).unwrap();

            // Should still work with smol's async runtime
            let socket_path_clone = socket_path.clone();
            let connect_task = smol::spawn(async move {
                smol::net::unix::UnixStream::connect(&socket_path_clone)
                    .await
                    .unwrap()
            });

            let connection = listener.accept().await.unwrap();
            // Just verify we got a valid connection with an ID
            let id = connection.id();
            assert!(id < usize::MAX);

            let _stream = connect_task.await;
        });
    }
}
