//! Tests for the Varlink upgrade protocol.

use std::{collections::VecDeque, os::fd::OwnedFd};
use zlink::connection::socket::ReadHalf;

// -------------------------------------------------------------
// Helper function for reading exactly N bytes while collecting FDs
// -------------------------------------------------------------
pub(crate) async fn read_exact_with_fds<R: ReadHalf>(
    read_half: &mut R,
    read_buffer: &mut Vec<u8>,
    received_fds: &mut VecDeque<Vec<OwnedFd>>,
    collected_fds: &mut Vec<OwnedFd>,
    out_buf: &mut [u8],
) -> zlink::Result<()> {
    let mut bytes_needed = out_buf.len();
    let mut bytes_written = 0;

    // 1. Try to consume from read_buffer first
    if !read_buffer.is_empty() {
        let to_take = std::cmp::min(read_buffer.len(), bytes_needed);
        out_buf[bytes_written..bytes_written + to_take].copy_from_slice(&read_buffer[..to_take]);
        read_buffer.drain(..to_take);
        bytes_needed -= to_take;
        bytes_written += to_take;
    }

    // 2. Also take any pre-received FDs from received_fds
    while let Some(mut fds) = received_fds.pop_front() {
        collected_fds.append(&mut fds);
    }

    // 3. Keep reading from read_half until we have enough bytes
    while bytes_needed > 0 {
        let mut temp_buf = vec![0u8; bytes_needed];
        let (n, mut fds) = read_half.read(&mut temp_buf).await?;
        if n == 0 {
            return Err(zlink::Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF",
            )));
        }
        out_buf[bytes_written..bytes_written + n].copy_from_slice(&temp_buf[..n]);
        collected_fds.append(&mut fds);
        bytes_needed -= n;
        bytes_written += n;
    }

    Ok(())
}

mod basic_upgrade {
    use serde::{Deserialize, Serialize};
    use std::os::fd::OwnedFd;
    use zlink::{
        Server,
        connection::socket::{ReadHalf, WriteHalf},
        unix::{bind, connect},
    };

    #[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::CustomType)]
    pub(crate) struct UpgradeReply {
        pub success: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::CustomType)]
    pub(crate) struct PingReply {
        pub pong: bool,
    }

    #[derive(Debug, Clone, PartialEq, zlink::ReplyError)]
    #[zlink(interface = "org.example.upgrade")]
    pub(crate) enum UpgradeError {}

    // -------------------------------------------------------------
    // 1. Service with on_upgrade implemented
    // -------------------------------------------------------------
    pub(crate) struct UpgradedService;

    #[zlink::service(types = [UpgradeReply, PingReply])]
    impl UpgradedService {
        #[zlink(interface = "org.example.upgrade", upgrade)]
        async fn do_upgrade(&self) -> UpgradeReply {
            UpgradeReply { success: true }
        }

        #[zlink(interface = "org.example.upgrade")]
        async fn ping(&self) -> PingReply {
            PingReply { pong: true }
        }

        async fn on_upgrade<S: zlink::connection::Socket>(
            &mut self,
            mut parts: zlink::connection::ConnectionParts<S>,
        ) -> zlink::Result<()> {
            let mut read_half = parts.read_half;
            let mut write_half = parts.write_half;

            // Process leftovers or read from socket
            let mut buf = [0; 4];
            if parts.read_buffer.len() >= 4 {
                buf.copy_from_slice(&parts.read_buffer[..4]);
            } else {
                let offset = parts.read_buffer.len();
                buf[..offset].copy_from_slice(&parts.read_buffer);
                let mut temp = vec![0; 4 - offset];
                let (n, _fds) = read_half.read(&mut temp).await.unwrap();
                assert_eq!(n, 4 - offset);
                buf[offset..].copy_from_slice(&temp);
            }

            if &buf == b"ping" {
                write_half.write(b"pong", &[] as &[OwnedFd]).await.unwrap();
            }

            Ok(())
        }
    }

    // Client proxy
    #[zlink::proxy("org.example.upgrade")]
    trait UpgradeProxy {
        #[zlink(upgrade)]
        async fn do_upgrade(
            self,
        ) -> zlink::Result<zlink::connection::UpgradeReply<Self::Socket, UpgradeReply, UpgradeError>>;

        async fn ping(&mut self) -> zlink::Result<Result<PingReply, UpgradeError>>;
    }

    #[test_log::test(tokio::test(flavor = "multi_thread"))]
    async fn test_successful_upgrade() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let socket_path = dir.path().join("upgrade.sock");

        let listener = bind(&socket_path).unwrap();
        let service = UpgradedService;
        let server = Server::new(listener, service);

        let client_fut = async {
            // Connect to upgraded service
            let conn = connect(&socket_path).await.unwrap();

            // Perform the upgrade call
            let upgrade_result = conn.do_upgrade().await.unwrap();
            let reply = upgrade_result.reply.unwrap();
            let params = reply.into_parameters().unwrap();
            assert!(params.success);

            // Retrieve raw socket halves from client upgrade result parts
            let parts = upgrade_result.parts;
            let mut read_half = parts.read_half;
            let mut write_half = parts.write_half;

            // Write raw custom protocol ping
            write_half.write(b"ping", &[] as &[OwnedFd]).await.unwrap();

            // Read raw custom protocol pong
            let mut buf = [0; 4];
            let (n, _fds) = read_half.read(&mut buf).await.unwrap();
            assert_eq!(n, 4);
            assert_eq!(&buf, b"pong");
        };

        tokio::select! {
            res = server.run() => { if let Err(e) = res { panic!("Server failed: {:?}", e); } },
            _ = client_fut => {},
        }

        Ok(())
    }

    // -------------------------------------------------------------
    // 2. Service WITHOUT on_upgrade implemented (should return MethodNotImplemented)
    // -------------------------------------------------------------
    pub(crate) struct UnimplementedService;

    #[zlink::service(types = [UpgradeReply, PingReply])]
    impl UnimplementedService {
        #[zlink(interface = "org.example.upgrade", upgrade)]
        async fn do_upgrade(&self) -> UpgradeReply {
            UpgradeReply { success: true }
        }

        #[zlink(interface = "org.example.upgrade")]
        async fn ping(&self) -> PingReply {
            PingReply { pong: true }
        }
    }

    #[test_log::test(tokio::test(flavor = "multi_thread"))]
    async fn test_upgrade_not_implemented() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let socket_path = dir.path().join("unimplemented.sock");

        let listener = bind(&socket_path).unwrap();
        let service = UnimplementedService;
        let server = Server::new(listener, service);

        let client_fut = async {
            // Connect to service
            let mut conn = connect(&socket_path).await.unwrap();

            // Manual upgrade call to avoid consuming `conn` (T2)
            #[derive(Serialize, Debug)]
            struct UpgradeMethodCall {
                method: &'static str,
            }

            let call = zlink::Call::new(UpgradeMethodCall {
                method: "org.example.upgrade.DoUpgrade",
            })
            .set_upgrade(true);

            #[derive(Serialize, Deserialize, Debug)]
            struct DummyUpgradeReply {}
            #[derive(Serialize, Deserialize, Debug)]
            struct DummyError {}

            // Try to perform upgrade call - since server lacks on_upgrade, it should fail
            // with MethodNotImplemented VarlinkService error.
            let upgrade_result = conn
                .call_method::<_, DummyUpgradeReply, DummyError>(&call, vec![])
                .await;
            assert!(upgrade_result.is_err());

            let err_str = format!("{:?}", upgrade_result.unwrap_err());
            assert!(err_str.contains("MethodNotImplemented") || err_str.contains("VarlinkService"));

            // Now, verify that the SAME connection is still usable for a normal method call (T2)
            let ping_reply = conn.ping().await.unwrap().unwrap();
            assert!(ping_reply.pong);
        };

        tokio::select! {
            res = server.run() => { if let Err(e) = res { panic!("Server failed: {:?}", e); } },
            _ = client_fut => {},
        }

        Ok(())
    }
}

mod fd_upgrade {
    use super::read_exact_with_fds;
    use serde::{Deserialize, Serialize};
    use std::{
        io::Read,
        os::fd::OwnedFd,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    use zlink::{
        Server,
        connection::socket::WriteHalf,
        unix::{bind, connect},
    };

    // -------------------------------------------------------------
    // 3. Service with on_upgrade that passes and validates FDs (T1)
    // -------------------------------------------------------------
    pub(crate) struct FdUpgradeService {
        pub verified: Arc<AtomicBool>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::CustomType)]
    pub(crate) struct FdUpgradeReply {
        pub success: bool,
    }

    #[derive(Debug, Clone, PartialEq, zlink::ReplyError)]
    #[zlink(interface = "org.example.fdupgrade")]
    pub(crate) enum FdUpgradeError {}

    #[zlink::service(types = [FdUpgradeReply])]
    impl FdUpgradeService {
        #[zlink(interface = "org.example.fdupgrade", upgrade)]
        async fn do_upgrade(&self) -> FdUpgradeReply {
            FdUpgradeReply { success: true }
        }

        async fn on_upgrade<S: zlink::connection::Socket>(
            &mut self,
            mut parts: zlink::connection::ConnectionParts<S>,
        ) -> zlink::Result<()> {
            let mut read_half = parts.read_half;
            let mut write_half = parts.write_half;

            // In this test, client does NOT pipeline, so read_buffer must be empty at start
            assert!(
                parts.read_buffer.is_empty(),
                "read_buffer must be empty since client did not pipeline"
            );

            let mut collected_fds = Vec::new();

            // 1. Read the 4-byte big-endian FD count. This deliberately starts the raw frame with
            // `\0` bytes (`0x00000002` = `[0, 0, 0, 2]`) to exercise the leftover/handoff path with
            // a `\0`-leading frame — the case the buffer-boundary logic must preserve verbatim.
            let mut count_buf = [0u8; 4];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut count_buf,
            )
            .await?;

            // T1: Verify big-endian framing specifically
            let fd_count = u32::from_be_bytes(count_buf) as usize;
            assert_eq!(fd_count, 2, "Expected exactly 2 FDs");

            // 2. Read 1-byte payload length
            let mut len_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut len_buf,
            )
            .await?;

            let payload_len = len_buf[0] as usize;
            assert_eq!(payload_len, 12);

            // 3. Read payload
            let mut payload_buf = vec![0u8; payload_len];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut payload_buf,
            )
            .await?;

            assert_eq!(payload_buf, b"demo-payload");
            assert_eq!(collected_fds.len(), fd_count);

            // 4. Read trailing null terminator
            let mut term_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut term_buf,
            )
            .await?;
            assert_eq!(term_buf[0], 0, "Trailing terminator must be null byte");

            // 5. Read from the FDs and assert their content
            let mut stream0 = std::os::unix::net::UnixStream::from(collected_fds.remove(0));
            let mut content0 = String::new();
            stream0.read_to_string(&mut content0).unwrap();
            assert_eq!(content0, "fd-payload-A");

            let mut stream1 = std::os::unix::net::UnixStream::from(collected_fds.remove(0));
            let mut content1 = String::new();
            stream1.read_to_string(&mut content1).unwrap();
            assert_eq!(content1, "fd-payload-B");

            // 6. Write back confirmation
            let reply_payload = b"ok";
            let mut response = Vec::new();
            response.extend_from_slice(&0u32.to_be_bytes());
            response.push(reply_payload.len() as u8);
            response.extend_from_slice(reply_payload);
            response.push(0); // trailing null-terminator for the custom reply

            write_half.write(&response, &[] as &[OwnedFd]).await?;

            self.verified.store(true, Ordering::SeqCst);

            Ok(())
        }
    }

    // Client proxy for FD upgrade
    #[zlink::proxy("org.example.fdupgrade")]
    #[allow(unused)]
    trait FdUpgradeProxy {
        #[zlink(upgrade)]
        async fn do_upgrade(
            self,
        ) -> zlink::Result<
            zlink::connection::UpgradeReply<Self::Socket, FdUpgradeReply, FdUpgradeError>,
        >;
    }

    #[test_log::test(tokio::test(flavor = "multi_thread"))]
    async fn test_upgrade_fd_passing() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let socket_path = dir.path().join("upgrade_fd.sock");

        let listener = bind(&socket_path).unwrap();
        let verified = Arc::new(AtomicBool::new(false));
        let service = FdUpgradeService {
            verified: verified.clone(),
        };
        let server = Server::new(listener, service);

        let client_fut = async {
            // Connect to upgraded service
            let conn = connect(&socket_path).await.unwrap();

            // Perform the upgrade call
            let upgrade_result = conn.do_upgrade().await.unwrap();
            let reply = upgrade_result.reply.unwrap();
            let params = reply.into_parameters().unwrap();
            assert!(params.success);

            // Retrieve raw socket halves from client upgrade result parts
            let mut parts = upgrade_result.parts;
            let mut read_half = parts.read_half;
            let mut write_half = parts.write_half;

            // Prepare FDs to send
            use std::io::Write;
            let (r0, mut w0) = std::os::unix::net::UnixStream::pair().unwrap();
            w0.write_all(b"fd-payload-A").unwrap();
            drop(w0);
            let fd0: OwnedFd = r0.into();

            let (r1, mut w1) = std::os::unix::net::UnixStream::pair().unwrap();
            w1.write_all(b"fd-payload-B").unwrap();
            drop(w1);
            let fd1: OwnedFd = r1.into();

            let fds_to_send = vec![fd0, fd1];

            // Build frame bytes. The frame begins with the big-endian FD count (`0x00000002` =
            // `[0, 0, 0, 2]`), i.e. it starts with `\0` bytes — proving the upgrade handoff
            // preserves `\0`-leading raw frames.
            let payload = b"demo-payload";
            let mut frame_bytes = Vec::new();
            frame_bytes.extend_from_slice(&2u32.to_be_bytes()); // 2 FDs
            frame_bytes.push(payload.len() as u8);
            frame_bytes.extend_from_slice(payload);
            frame_bytes.push(0); // trailing null-terminator

            // Write raw custom protocol with FDs
            write_half.write(&frame_bytes, &fds_to_send).await.unwrap();

            // Read response
            let mut res_collected_fds = Vec::new();
            let mut res_count_buf = [0u8; 4];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut res_collected_fds,
                &mut res_count_buf,
            )
            .await
            .unwrap();

            let res_fd_count = u32::from_be_bytes(res_count_buf);
            assert_eq!(res_fd_count, 0);

            let mut res_len_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut res_collected_fds,
                &mut res_len_buf,
            )
            .await
            .unwrap();

            let res_payload_len = res_len_buf[0] as usize;
            assert_eq!(res_payload_len, 2);

            let mut res_payload_buf = vec![0u8; res_payload_len];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut res_collected_fds,
                &mut res_payload_buf,
            )
            .await
            .unwrap();

            assert_eq!(res_payload_buf, b"ok");

            let mut res_term_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut res_collected_fds,
                &mut res_term_buf,
            )
            .await
            .unwrap();
            assert_eq!(
                res_term_buf[0], 0,
                "Custom reply trailing terminator must be null byte"
            );
        };

        tokio::select! {
            res = server.run() => { if let Err(e) = res { panic!("Server failed: {:?}", e); } },
            _ = client_fut => {},
        }

        assert!(
            verified.load(Ordering::SeqCst),
            "Server should have verified the FDs and their contents"
        );

        Ok(())
    }
}

mod pipelined_upgrade {
    use super::read_exact_with_fds;
    use serde::{Deserialize, Serialize};
    use std::{
        collections::VecDeque,
        os::fd::OwnedFd,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    use zlink::{
        Server,
        connection::{
            Socket,
            socket::{ReadHalf, WriteHalf},
        },
        unix::bind,
    };

    // -------------------------------------------------------------
    // 4. Service for verifying pipelined leftover-buffer handoff (T3)
    // -------------------------------------------------------------
    pub(crate) struct PipelinedUpgradeService {
        pub verified: Arc<AtomicBool>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::CustomType)]
    pub(crate) struct PipelinedReply {
        pub success: bool,
    }

    #[derive(Debug, Clone, PartialEq, zlink::ReplyError)]
    #[zlink(interface = "org.example.pipelined")]
    #[allow(dead_code)]
    pub(crate) enum PipelinedError {}

    #[zlink::service(types = [PipelinedReply])]
    impl PipelinedUpgradeService {
        #[zlink(interface = "org.example.pipelined", upgrade)]
        async fn do_upgrade(&self) -> PipelinedReply {
            PipelinedReply { success: true }
        }

        async fn on_upgrade<S: zlink::connection::Socket>(
            &mut self,
            mut parts: zlink::connection::ConnectionParts<S>,
        ) -> zlink::Result<()> {
            let mut read_half = parts.read_half;
            let mut write_half = parts.write_half;

            // T3: Assert that read_buffer is NOT empty (because client pipelined the custom frame
            // right after upgrade request)
            assert!(
                !parts.read_buffer.is_empty(),
                "Pipelined bytes must be present in read_buffer"
            );
            // Frame = BE u32 count (4) + payload len (1) + "demo-payload" (12) + `\0` (1) = 18.
            assert_eq!(
                parts.read_buffer.len(),
                18,
                "Should have exactly 18 leftover bytes"
            );
            // The leftover frame starts with the big-endian count `0x00000000` = `[0, 0, 0, 0]`,
            // i.e. with `\0` bytes — exactly the case the boundary logic must preserve verbatim.
            assert_eq!(
                &parts.read_buffer[..4],
                &[0, 0, 0, 0],
                "Leftover frame must start with the `\\0`-leading BE count, intact"
            );

            let mut collected_fds = Vec::new();

            // Read 4-byte BE count
            let mut count_buf = [0u8; 4];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut count_buf,
            )
            .await?;
            let fd_count = u32::from_be_bytes(count_buf) as usize;
            assert_eq!(fd_count, 0, "No FDs in pipelined test");

            // Read 1-byte payload length
            let mut len_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut len_buf,
            )
            .await?;
            let payload_len = len_buf[0] as usize;
            assert_eq!(payload_len, 12);

            // Read payload
            let mut payload_buf = vec![0u8; payload_len];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut payload_buf,
            )
            .await?;
            assert_eq!(payload_buf, b"demo-payload");

            // Read trailing null terminator
            let mut term_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut parts.read_buffer,
                &mut parts.received_fds,
                &mut collected_fds,
                &mut term_buf,
            )
            .await?;
            assert_eq!(term_buf[0], 0, "Trailing terminator must be null byte");

            // Write back confirmation
            let reply_payload = b"ok";
            let mut response = Vec::new();
            response.extend_from_slice(&0u32.to_be_bytes());
            response.push(reply_payload.len() as u8);
            response.extend_from_slice(reply_payload);
            response.push(0);

            write_half.write(&response, &[] as &[OwnedFd]).await?;

            self.verified.store(true, Ordering::SeqCst);

            Ok(())
        }
    }

    #[zlink::proxy("org.example.pipelined")]
    #[allow(unused)]
    trait PipelinedProxy {
        #[zlink(upgrade)]
        async fn do_upgrade(
            self,
        ) -> zlink::Result<
            zlink::connection::UpgradeReply<Self::Socket, PipelinedReply, PipelinedError>,
        >;
    }

    #[test_log::test(tokio::test(flavor = "multi_thread"))]
    async fn test_upgrade_pipelining() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let socket_path = dir.path().join("upgrade_pipelined.sock");

        let listener = bind(&socket_path).unwrap();
        let verified = Arc::new(AtomicBool::new(false));
        let service = PipelinedUpgradeService {
            verified: verified.clone(),
        };
        let server = Server::new(listener, service);

        let client_fut = async {
            // This test deliberately bypasses the `Connection`/`call_upgrade` client API and writes
            // raw bytes to a `UnixStream`. The point is *pipelining*: sending the upgrade call and
            // the first raw post-upgrade frame in a single write, before the server has replied.
            // `call_upgrade` can't express that — it sends the call, awaits exactly one reply, and
            // only then hands back the raw socket halves. Pipelining ahead of the reply is what
            // leaves `\0`-leading raw bytes buffered in the server's reader, which is the handoff
            // path under test.
            let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
            let socket = zlink::unix::Stream::from(stream);
            let (mut read_half, mut write_half) = socket.split();

            // 1. Serialize the upgrade call exactly as the wire format expects, then append the
            //    `\0` Varlink message terminator by hand (we're framing the bytes ourselves here).
            #[derive(Serialize, Debug)]
            struct UpgradeMethodCall {
                method: &'static str,
            }
            let call = zlink::Call::new(UpgradeMethodCall {
                method: "org.example.pipelined.DoUpgrade",
            })
            .set_upgrade(true);
            let mut data = serde_json::to_vec(&call).unwrap();
            data.push(0); // Varlink message terminator.

            // Pipelined custom protocol frame. It starts with the big-endian count `0x00000000` =
            // `[0, 0, 0, 0]`, i.e. with `\0` bytes, immediately after the upgrade call's `\0`
            // terminator — exercising the `\0`-leading leftover handoff path.
            let payload = b"demo-payload";
            data.extend_from_slice(&0u32.to_be_bytes()); // 0 FDs
            data.push(payload.len() as u8);
            data.extend_from_slice(payload);
            data.push(0); // trailing terminator to satisfy Varlink null-termination

            // Send all bytes at once (pipelining)!
            write_half.write(&data, &[] as &[OwnedFd]).await.unwrap();

            // Read upgrade reply from server
            let mut reply_buf = vec![0u8; 1024];
            let (n, _fds) = read_half.read(&mut reply_buf).await.unwrap();

            // Find the boundary of the Varlink reply (first null terminator)
            let first_null_idx = reply_buf[..n]
                .iter()
                .position(|&b| b == 0)
                .expect("Should find null terminator");
            let end_idx = first_null_idx + 1;

            let varlink_reply_bytes = &reply_buf[..end_idx];
            let custom_bytes = &reply_buf[end_idx..n];

            // Verify the Varlink reply
            let reply_str = std::str::from_utf8(varlink_reply_bytes).unwrap();
            assert!(reply_str.contains("\"success\":true"));

            // Verify the custom protocol response in custom_bytes
            let mut client_read_buffer = custom_bytes.to_vec();
            let mut client_received_fds = VecDeque::new();
            let mut client_collected_fds = Vec::new();

            // Read custom confirmation BE count
            let mut res_count_buf = [0u8; 4];
            read_exact_with_fds(
                &mut read_half,
                &mut client_read_buffer,
                &mut client_received_fds,
                &mut client_collected_fds,
                &mut res_count_buf,
            )
            .await
            .unwrap();

            let res_fd_count = u32::from_be_bytes(res_count_buf);
            assert_eq!(res_fd_count, 0);

            let mut res_len_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut client_read_buffer,
                &mut client_received_fds,
                &mut client_collected_fds,
                &mut res_len_buf,
            )
            .await
            .unwrap();

            let res_payload_len = res_len_buf[0] as usize;
            assert_eq!(res_payload_len, 2);

            let mut res_payload_buf = vec![0u8; res_payload_len];
            read_exact_with_fds(
                &mut read_half,
                &mut client_read_buffer,
                &mut client_received_fds,
                &mut client_collected_fds,
                &mut res_payload_buf,
            )
            .await
            .unwrap();

            assert_eq!(res_payload_buf, b"ok");

            let mut res_term_buf = [0u8; 1];
            read_exact_with_fds(
                &mut read_half,
                &mut client_read_buffer,
                &mut client_received_fds,
                &mut client_collected_fds,
                &mut res_term_buf,
            )
            .await
            .unwrap();
            assert_eq!(
                res_term_buf[0], 0,
                "Custom reply trailing terminator must be null byte"
            );
        };

        tokio::select! {
            res = server.run() => { if let Err(e) = res { panic!("Server failed: {:?}", e); } },
            _ = client_fut => {},
        }

        assert!(
            verified.load(Ordering::SeqCst),
            "Server should have verified the pipelined leftover bytes"
        );

        Ok(())
    }
}
