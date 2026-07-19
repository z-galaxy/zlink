//! Tests for file descriptor passing with service macro.

use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::{self},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn fd_passing() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    let listener = bind(&socket_path).unwrap();
    let service = FdService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    let mut conn = connect(socket_path).await?;

    // Send multiple FDs and read from a specific one by index.
    let (r0, mut w0) = UnixStream::pair()?;
    let (r1, mut w1) = UnixStream::pair()?;
    let (r2, mut w2) = UnixStream::pair()?;
    w0.write_all(b"data-zero")?;
    w1.write_all(b"data-one")?;
    w2.write_all(b"data-two")?;
    drop((w0, w1, w2));
    let fds = vec![r0.into(), r1.into(), r2.into()];
    // Read from index 1.
    let data = conn.read_fd(1, fds).await?.unwrap();
    assert_eq!(data.contents, "data-one");

    // Invalid index returns an error.
    let (r, mut w) = UnixStream::pair()?;
    w.write_all(b"some data")?;
    drop(w);
    let result = conn.read_fd(5, vec![r.into()]).await?;
    assert!(matches!(result, Err(FdError::InvalidIndex { index: 5 })));

    // Receive FDs from the service. Each handle has a name and fd_index referencing the FD vector.
    let names = vec!["config.txt".into(), "data.bin".into(), "log.txt".into()];
    let (result, fds) = conn.open_fds(names).await?;
    let handles = result.unwrap().handles;
    assert_eq!(handles.len(), 3);
    assert_eq!(fds.len(), 3);
    // Verify each handle's name and that the FD at fd_index contains the name as content.
    for handle in &handles {
        let fd = &fds[handle.fd_index as usize];
        let cloned_fd = fd.try_clone()?;
        let mut stream = UnixStream::from(cloned_fd);
        let mut buf = String::new();
        stream.read_to_string(&mut buf)?;
        assert_eq!(buf, handle.name);
    }

    // Receive zero FDs from the service.
    let (result, fds) = conn.open_fds(Vec::new()).await?;
    let handles = result.unwrap().handles;
    assert!(handles.is_empty());
    assert!(fds.is_empty());

    // Receive an FD on success path and verify the handle's index references the correct FD.
    let (result, fds) = conn.try_open_fd("success.txt".into(), false).await?;
    let handle = result.unwrap();
    assert_eq!(handle.name, "success.txt");
    assert_eq!(handle.fd_index, 0);
    assert_eq!(fds.len(), 1);
    let mut stream = UnixStream::from(fds.into_iter().next().unwrap());
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    assert_eq!(buf, "success.txt");

    // Receive an FD on error path and verify the diagnostic content.
    let (result, fds) = conn.try_open_fd("missing.txt".into(), true).await?;
    let err = result.unwrap_err();
    assert!(matches!(err, FdError::NotFound { name } if name == "missing.txt"));
    assert_eq!(fds.len(), 1);
    let mut stream = UnixStream::from(fds.into_iter().next().unwrap());
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    assert_eq!(buf, "error-diagnostic");

    Ok(())
}

// Response type for FD operations. The `fd_index` field references a position in the FD vector.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, introspect::Type)]
pub(crate) struct FdHandle {
    pub name: String,
    pub fd_index: u32,
}

// Response type for FD operations returning multiple handles.
#[derive(Debug, Clone, Serialize, Deserialize, introspect::Type)]
pub(crate) struct Handles {
    pub handles: Vec<FdHandle>,
}

// Response type for read operations
#[derive(Debug, Clone, Serialize, Deserialize, introspect::Type)]
pub(crate) struct Contents {
    pub contents: String,
}

// Error type for FD operations.
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.fd")]
pub(crate) enum FdError {
    InvalidIndex { index: u32 },
    NotFound { name: String },
}

// A service that tests file descriptor passing.
struct FdService;

#[zlink::service(interface = "org.example.fd")]
impl FdService {
    /// Receive FDs and read from the one at the given index.
    async fn read_fd(
        &self,
        fd_index: u32,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<Contents, FdError> {
        use std::{io::Read, os::unix::net::UnixStream};

        let Some(fd) = fds.into_iter().nth(fd_index as usize) else {
            return Err(FdError::InvalidIndex { index: fd_index });
        };
        let mut stream = UnixStream::from(fd);
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        Ok(Contents { contents: buf })
    }

    /// Open a list of named FDs and return handles with their indexes.
    #[zlink(return_fds)]
    async fn open_fds(&self, names: Vec<String>) -> (Handles, Vec<std::os::fd::OwnedFd>) {
        use std::{io::Write, os::unix::net::UnixStream};

        let mut handles = Vec::new();
        let mut fds = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            let (r, mut w) = UnixStream::pair().unwrap();
            // Write the name as the FD content for verification.
            w.write_all(name.as_bytes()).unwrap();
            drop(w);
            handles.push(FdHandle {
                name,
                fd_index: i as u32,
            });
            fds.push(r.into());
        }
        (Handles { handles }, fds)
    }

    /// Try to open an FD. On success, return the handle with its index. On error, return the
    /// error alongside a diagnostic FD.
    #[zlink(return_fds)]
    async fn try_open_fd(
        &self,
        name: String,
        should_fail: bool,
    ) -> (Result<FdHandle, FdError>, Vec<std::os::fd::OwnedFd>) {
        use std::{io::Write, os::unix::net::UnixStream};

        let (r, mut w) = UnixStream::pair().unwrap();
        if should_fail {
            w.write_all(b"error-diagnostic").unwrap();
            drop(w);
            (
                Err(FdError::NotFound { name }),
                vec![std::os::fd::OwnedFd::from(r)],
            )
        } else {
            w.write_all(name.as_bytes()).unwrap();
            drop(w);
            (
                Ok(FdHandle { name, fd_index: 0 }),
                vec![std::os::fd::OwnedFd::from(r)],
            )
        }
    }
}

// Proxy for FD service.
#[zlink::proxy("org.example.fd")]
trait FdProxy {
    async fn read_fd(
        &mut self,
        fd_index: u32,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> zlink::Result<Result<Contents, FdError>>;

    #[zlink(return_fds)]
    async fn open_fds(
        &mut self,
        names: Vec<String>,
    ) -> zlink::Result<(Result<Handles, FdError>, Vec<std::os::fd::OwnedFd>)>;

    #[zlink(return_fds)]
    async fn try_open_fd(
        &mut self,
        name: String,
        should_fail: bool,
    ) -> zlink::Result<(Result<FdHandle, FdError>, Vec<std::os::fd::OwnedFd>)>;
}
