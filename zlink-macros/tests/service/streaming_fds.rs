//! Tests for streaming service methods with FD passing (#[zlink(more, return_fds)]).

use super::fd_passing::{FdError, FdHandle};
use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::Type,
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn streaming_with_fds() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    let listener = bind(&socket_path).unwrap();
    let service = StreamingFdService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    let mut conn = connect(socket_path).await?;

    // =========================================================================
    // Test 1: Stream output FDs (return_fds + more)
    // =========================================================================
    {
        let names = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        let mut stream = std::pin::pin!(conn.stream_fds(names).await?);

        // Collect all stream items.
        let mut handles = Vec::new();
        let mut all_fds = Vec::new();
        while let Some(result) = stream.next().await {
            let (result, fds) = result?;
            let handle = result.unwrap();
            handles.push(handle);
            all_fds.extend(fds);
        }

        // Should have received 3 handles with 3 FDs.
        assert_eq!(handles.len(), 3);
        assert_eq!(all_fds.len(), 3);

        // Verify each handle's FD contains the expected content.
        for (i, handle) in handles.iter().enumerate() {
            assert_eq!(handle.fd_index, i as u32);
            let fd = all_fds[handle.fd_index as usize].try_clone()?;
            let mut stream = UnixStream::from(fd);
            let mut buf = String::new();
            stream.read_to_string(&mut buf)?;
            assert_eq!(buf, handle.name);
        }
    }

    // =========================================================================
    // Test 2: Stream input FDs (fds + more)
    // =========================================================================
    {
        // Create 3 FDs with known content.
        let (r0, mut w0) = UnixStream::pair()?;
        let (r1, mut w1) = UnixStream::pair()?;
        let (r2, mut w2) = UnixStream::pair()?;
        w0.write_all(b"content-zero")?;
        w1.write_all(b"content-one")?;
        w2.write_all(b"content-two")?;
        drop((w0, w1, w2));

        let fds = vec![r0.into(), r1.into(), r2.into()];
        let mut stream = std::pin::pin!(conn.read_fds_streaming(fds).await?);

        // Collect all stream items.
        let mut results = Vec::new();
        while let Some(result) = stream.next().await {
            let read_result = result?.unwrap();
            results.push(read_result);
        }

        // Should have received 3 results.
        assert_eq!(results.len(), 3);

        // Verify each result has the expected content.
        assert_eq!(results[0].fd_index, 0);
        assert_eq!(results[0].content, "content-zero");
        assert_eq!(results[1].fd_index, 1);
        assert_eq!(results[1].content, "content-one");
        assert_eq!(results[2].fd_index, 2);
        assert_eq!(results[2].content, "content-two");
    }

    Ok(())
}

/// Response for streaming FD read operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct FdReadResult {
    fd_index: u32,
    content: String,
}

/// A service that streams file descriptors.
struct StreamingFdService;

#[zlink::service(interface = "org.example.streamingFd")]
impl StreamingFdService {
    /// Stream FDs with handles, one per name. Each stream item contains a handle and the FD.
    #[zlink(more, return_fds)]
    async fn stream_fds(
        &self,
        more: bool,
        names: Vec<String>,
    ) -> impl futures_util::Stream<Item = (zlink::Reply<FdHandle>, Vec<std::os::fd::OwnedFd>)> + Unpin
    {
        use std::{io::Write, os::unix::net::UnixStream};

        // If more=false, only return the first item.
        let names: Vec<String> = if more {
            names
        } else {
            names.into_iter().take(1).collect()
        };

        let n = names.len();
        futures_util::stream::iter(names.into_iter().enumerate().map(move |(i, name)| {
            let (r, mut w) = UnixStream::pair().unwrap();
            w.write_all(name.as_bytes()).unwrap();
            drop(w);
            let handle = FdHandle {
                name,
                fd_index: i as u32,
            };
            let reply = zlink::Reply::new(Some(handle)).set_continues(Some(i < n - 1));
            (reply, vec![r.into()])
        }))
    }

    /// Receive FDs and stream back the content read from each one.
    #[zlink(more)]
    async fn read_fds_streaming(
        &self,
        more: bool,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> impl futures_util::Stream<Item = zlink::Reply<FdReadResult>> + Unpin {
        use std::{io::Read, os::unix::net::UnixStream};

        // If more=false, only return the first result.
        let fds: Vec<std::os::fd::OwnedFd> = if more {
            fds
        } else {
            fds.into_iter().take(1).collect()
        };

        let n = fds.len();
        futures_util::stream::iter(fds.into_iter().enumerate().map(move |(i, fd)| {
            let mut stream = UnixStream::from(fd);
            let mut content = String::new();
            stream.read_to_string(&mut content).unwrap();
            let result = FdReadResult {
                fd_index: i as u32,
                content,
            };
            zlink::Reply::new(Some(result)).set_continues(Some(i < n - 1))
        }))
    }
}

/// Proxy for streaming FD service.
#[zlink::proxy("org.example.streamingFd")]
trait StreamingFdProxy {
    #[zlink(more, return_fds)]
    async fn stream_fds(
        &mut self,
        names: Vec<String>,
    ) -> zlink::Result<
        impl futures_util::Stream<
            Item = zlink::Result<(Result<FdHandle, FdError>, Vec<std::os::fd::OwnedFd>)>,
        >,
    >;

    #[zlink(more)]
    async fn read_fds_streaming(
        &mut self,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<FdReadResult, ()>>>>;
}
