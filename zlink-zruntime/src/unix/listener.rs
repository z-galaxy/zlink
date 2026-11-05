use std::{
    fmt,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::net::UnixListener as StdUnixListener,
    },
    path::Path,
};

#[cfg(target_os = "linux")]
use std::path::PathBuf;

use zruntime::{Local, Mode, Runtime, Shared, SharedRuntime, net::unix};

#[cfg(target_os = "linux")]
use crate::unix_utils::{self, SocketRole};
use crate::{Connection, Result};

/// Create a new unix domain socket listener and bind it to `path`.
///
/// The listener, and the sockets it accepts, are on the runtime [`SharedRuntime::current`]
/// returns, so they can be used from any thread and polled by any executor. [`bind_local`] binds
/// on the local runtime the calling thread is running instead.
///
/// On Linux the listener socket is tagged with a `user.varlink` extended attribute set to `listen`
/// and, if `path` is absolute, the socket inode created there with one set to `entrypoint`, so that
/// tools like `varlinkctl list-sockets` recognise them as Varlink sockets. A relative `path` is not
/// used for that, since it would be resolved against whatever the working directory is at the
/// time. Tagging is best effort; kernels without support (older than Linux 7.1) are silently
/// tolerated.
pub fn bind<P>(path: P) -> Result<Listener>
where
    P: AsRef<Path>,
{
    bind_on(&SharedRuntime::current()?, path.as_ref())
}

/// Create a new unix domain socket listener and bind it to `path`, on the local runtime the calling
/// thread is running.
///
/// The listener, and the sockets it accepts, stay on the calling thread, and make progress while
/// the thread runs that runtime, as a task of it or inside its [`Runtime::block_on`]. Fails where
/// the thread is running no local runtime.
///
/// The sockets are tagged as by [`bind`].
pub fn bind_local<P>(path: P) -> Result<Listener<Local>>
where
    P: AsRef<Path>,
{
    bind_on(&super::running_local()?, path.as_ref())
}

fn bind_on<M>(runtime: &Runtime<M>, path: &Path) -> Result<Listener<M>>
where
    M: Mode,
{
    let listener = unix::UnixListener::bind(runtime, path)?;

    Ok(Listener::new(
        listener,
        #[cfg(target_os = "linux")]
        Some(path.to_owned()),
    ))
}

/// A unix domain socket listener.
///
/// `M` is the flavour of the zruntime runtime the listener is on, as for [`super::Stream`]. The
/// sockets it accepts go on the same runtime.
///
/// On Linux, listeners are tagged `listen`, the socket inode [`bind`] creates at an absolute path
/// `entrypoint` and sockets returned by [`crate::Listener::accept`] `server`. Listeners adopted
/// from a file descriptor or bound to a relative path only get the `listen` tag, since the inode
/// they were bound to cannot be located safely. Tagging is best effort; kernels without support
/// (older than Linux 7.1) are silently tolerated.
pub struct Listener<M = Shared>
where
    M: Mode,
{
    listener: unix::UnixListener<M>,
    /// The path [`bind`] was given, if this listener came from it.
    #[cfg(target_os = "linux")]
    path: Option<PathBuf>,
}

impl<M> fmt::Debug for Listener<M>
where
    M: Mode,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Listener");
        debug.field("listener", &self.listener);
        #[cfg(target_os = "linux")]
        debug.field("path", &self.path);

        debug.finish()
    }
}

impl<M> Listener<M>
where
    M: Mode,
{
    /// Wrap a bound listener, tagging it and, when `path` is known, its entrypoint inode.
    fn new(
        listener: unix::UnixListener<M>,
        #[cfg(target_os = "linux")] path: Option<PathBuf>,
    ) -> Self {
        #[cfg(target_os = "linux")]
        unix_utils::tag_listener(&listener, path.as_deref());

        Self {
            listener,
            #[cfg(target_os = "linux")]
            path,
        }
    }
}

impl<M> crate::Listener for Listener<M>
where
    M: Mode,
{
    type Socket = super::Stream<M>;

    async fn accept(&mut self) -> Result<Option<Connection<Self::Socket>>> {
        let (stream, _) = self.listener.accept().await?;
        #[cfg(target_os = "linux")]
        unix_utils::tag_socket(&stream, SocketRole::Server);

        Ok(Some(super::Stream::try_from(stream)?.into()))
    }

    #[cfg(target_os = "linux")]
    fn set_xattr(&self, name: &str, value: impl AsRef<[u8]>) -> Result<()> {
        let Some(path) = &self.path else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the entrypoint inode of a listener adopted from a file descriptor is not known",
            )
            .into());
        };

        unix_utils::set_entrypoint_xattr(path, name, value.as_ref()).map_err(Into::into)
    }
}

/// The listener is adopted as one that was not bound by [`bind`]: see [`Listener`].
impl<M> From<unix::UnixListener<M>> for Listener<M>
where
    M: Mode,
{
    fn from(listener: unix::UnixListener<M>) -> Self {
        Listener::new(
            listener,
            #[cfg(target_os = "linux")]
            None,
        )
    }
}

/// The listener is on the runtime [`SharedRuntime::current`] returns.
impl TryFrom<OwnedFd> for Listener {
    type Error = crate::Error;

    fn try_from(fd: OwnedFd) -> Result<Self> {
        let listener = StdUnixListener::from(fd);

        Ok(unix::UnixListener::from_std(&SharedRuntime::current()?, listener)?.into())
    }
}

impl<M> AsFd for Listener<M>
where
    M: Mode,
{
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.listener.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Listener as _, unix::connect_local};
    use std::os::unix::net::UnixListener as StdUnixListener;
    use tempfile::TempDir;
    use zruntime::{LocalRuntime, net::unix::UnixStream};

    #[test]
    fn from_fd_with_multiple_connections() {
        zruntime::block_on(async {
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
                let connect_task = zruntime::spawn(async move {
                    let runtime = SharedRuntime::current().unwrap();
                    UnixStream::<Shared>::connect(&runtime, &socket_path_clone)
                        .await
                        .unwrap()
                });

                let connection = listener.accept().await.unwrap().unwrap();
                let id = connection.id();

                // Each connection should have a unique ID
                if let Some(prev) = previous_id {
                    assert_ne!(id, prev, "Connection IDs should be unique");
                }
                previous_id = Some(id);

                let _stream = connect_task.await.unwrap();
            }
        });
    }

    #[test]
    fn from_fd_preserves_nonblocking() {
        zruntime::block_on(async {
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

            // Should still work with zruntime's async runtime
            let socket_path_clone = socket_path.clone();
            let connect_task = zruntime::spawn(async move {
                let runtime = SharedRuntime::current().unwrap();
                UnixStream::<Shared>::connect(&runtime, &socket_path_clone)
                    .await
                    .unwrap()
            });

            let connection = listener.accept().await.unwrap().unwrap();
            // Just verify we got a valid connection with an ID
            let id = connection.id();
            assert!(id < usize::MAX);

            let _stream = connect_task.await.unwrap();
        });
    }

    /// A connection on a local runtime carries a reply, with the listener, the socket it accepts
    /// and the one that connects to it all on that runtime.
    #[test]
    fn local_connections() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("local.sock");
        let runtime = LocalRuntime::new().unwrap();

        let params = runtime.block_on(async {
            let mut listener = bind_local(&socket_path).unwrap();
            let client = connect_local(&socket_path).await.unwrap();
            let server = listener.accept().await.unwrap().unwrap();

            let (_, mut write) = server.split();
            let reply = crate::Reply::new(Some("pong".to_string())).set_continues(Some(false));
            write.send_reply(&reply, vec![]).await.unwrap();

            let (mut read, _) = client.split();
            let (reply, _fds) = read.receive_reply::<String, ()>().await.unwrap();

            reply.unwrap().into_parameters().unwrap()
        });

        assert_eq!(params, "pong");
    }

    /// Nothing is there to bind on outside a local runtime.
    #[test]
    fn bind_local_needs_a_local_runtime() {
        let temp_dir = TempDir::new().unwrap();

        assert!(bind_local(temp_dir.path().join("none.sock")).is_err());
        assert!(
            zruntime::block_on(async { bind_local(temp_dir.path().join("none.sock")) }).is_err()
        );
    }
}
