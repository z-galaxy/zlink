//! Realistic end-to-end test: "log tail attach" combining zlink's upgrade
//! feature with out-of-band file-descriptor passing.
//!
//! The scenario mirrors how systemd services hand off file descriptors after a
//! Varlink method call (e.g. machined's `Open` returning a `ptyFileDescriptor`):
//!
//!   1. The client connects over a real Unix domain socket to a real tokio server task and first
//!      performs a *normal* Varlink call, `Stats()`, to prove ordinary request/response framing
//!      works on the connection.
//!   2. The client then calls the `upgrade` method `Attach(name)`. After the reply, the connection
//!      switches to a raw binary protocol.
//!   3. In its `on_upgrade` handler the server materializes a *random* number (1-5) of temp files
//!      with random small sizes (0-100 bytes each) into real `std::fs::File`s, converts each into
//!      an `OwnedFd`, and sends a single raw frame of two big-endian `u32`s — the fd count and the
//!      combined byte total — followed by NO inline payload. The file contents travel *out of band*
//!      as ancillary data (SCM_RIGHTS); the receiver gets the open file descriptors themselves.
//!   4. The client reads the two `u32`s, collects exactly `fd_count` descriptors, reads every one
//!      to EOF, and asserts the combined byte count read back through the fds equals the announced
//!      total. Random counts/sizes (including possibly-empty files) exercise multi-fd passing
//!      rather than a single hard-coded payload.

#![cfg(all(feature = "service", feature = "introspection", feature = "proxy"))]

use std::{
    collections::VecDeque,
    io::{Read, Seek, Write},
    os::fd::OwnedFd,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    connection::socket::{ReadHalf, WriteHalf},
    unix::{bind, connect},
};

/// Single-byte request the client sends over the upgraded connection to ask the
/// server to hand off the logs. Making the raw protocol client-initiated avoids
/// pipelining raw bytes into the same burst as the Varlink upgrade reply (which
/// the framing layer expects to end the burst with a `\0`).
const REQ_BYTE: u8 = 0x05;

// -------------------------------------------------------------------------
// Wire types for the `org.example.LogService` interface.
// -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::CustomType)]
struct StatsReply {
    #[serde(rename = "lineCount")]
    line_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::CustomType)]
struct AttachReply {
    ready: bool,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, zlink::introspect::ReplyError)]
#[zlink(interface = "org.example.LogService")]
enum LogError {
    /// The requested log stream does not exist.
    NotFound,
}

// -------------------------------------------------------------------------
// Helper: read exactly `out_buf.len()` bytes from the upgraded read half while
// collecting any ancillary file descriptors. Leftover bytes/fds buffered during
// the Varlink phase (`read_buffer` / `received_fds`) are drained first.
// -------------------------------------------------------------------------
async fn read_exact_with_fds<R: ReadHalf>(
    read_half: &mut R,
    read_buffer: &mut Vec<u8>,
    received_fds: &mut VecDeque<Vec<OwnedFd>>,
    collected_fds: &mut Vec<OwnedFd>,
    out_buf: &mut [u8],
) -> zlink::Result<()> {
    // `rest` always points at the still-unfilled tail of `out_buf`. Each step fills a prefix and
    // re-slices `rest` to the remainder via `split_at_mut`, so there's no manual index arithmetic
    // (and thus no room for an off-by-one slice panic).
    let mut rest = out_buf;

    // Consume any leftover bytes buffered during the Varlink phase first.
    if !read_buffer.is_empty() {
        let to_take = read_buffer.len().min(rest.len());
        let (head, tail) = rest.split_at_mut(to_take);
        head.copy_from_slice(&read_buffer[..to_take]);
        read_buffer.drain(..to_take);
        rest = tail;
    }

    // Take any fds that arrived alongside those leftover bytes.
    while let Some(mut fds) = received_fds.pop_front() {
        collected_fds.append(&mut fds);
    }

    // Read the rest straight off the socket, gathering fds as they arrive. Reading directly into
    // `rest` means the socket can never write past the buffer.
    while !rest.is_empty() {
        let (n, mut fds) = read_half.read(rest).await?;
        collected_fds.append(&mut fds);
        if n == 0 {
            return Err(zlink::Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF during upgraded read",
            )));
        }
        // `read` reports bytes written into `rest`, so `n <= rest.len()`; clamp defensively so a
        // misbehaving impl can never make the re-slice below panic.
        let (_filled, tail) = rest.split_at_mut(n.min(rest.len()));
        rest = tail;
    }

    Ok(())
}

// -------------------------------------------------------------------------
// The logging service.
// -------------------------------------------------------------------------

/// Number of log lines the service reports via the normal `Stats` method.
const STATS_LINE_COUNT: i64 = 4;

struct LogService {
    /// Set once `on_upgrade` has handed off the fds, so the test can assert the
    /// server actually reached the upgrade path.
    served: Arc<AtomicBool>,
    /// The combined byte total the server actually generated and announced, so
    /// the test can sanity-check it against the size read back through the fds.
    total_served: Arc<AtomicU64>,
}

#[zlink::service(types = [StatsReply, AttachReply])]
impl LogService {
    /// Normal Varlink method, exercised before the upgrade to prove ordinary
    /// request/response framing works on the connection.
    #[zlink(interface = "org.example.LogService")]
    async fn stats(&self) -> Result<StatsReply, LogError> {
        Ok(StatsReply {
            line_count: STATS_LINE_COUNT,
        })
    }

    /// Upgrade method: the client asks to attach to a named log stream. After
    /// the reply the connection switches to the raw fd-passing protocol handled
    /// in `on_upgrade`.
    #[zlink(interface = "org.example.LogService", upgrade)]
    async fn attach(&self, name: String) -> Result<AttachReply, LogError> {
        if name == "main" {
            Ok(AttachReply { ready: true })
        } else {
            Err(LogError::NotFound)
        }
    }

    async fn on_upgrade<S: zlink::connection::Socket>(
        &mut self,
        mut parts: zlink::connection::ConnectionParts<S>,
    ) -> zlink::Result<()> {
        let mut read_half = parts.read_half;
        let mut write_half = parts.write_half;

        // The raw protocol is client-initiated: wait for the client's request
        // byte before sending anything. This guarantees the client has already
        // consumed the Varlink upgrade reply, so our raw frame is never mixed
        // into the same read burst as that reply.
        let mut req = [0u8; 1];
        let (n, _fds) = read_half.read(&mut req).await?;
        if n != 1 || req[0] != REQ_BYTE {
            return Err(zlink::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "client did not send the expected request byte",
            )));
        }

        // Materialize a random number of small temp files with random sizes, then hand off their
        // open descriptors. Reading the bytes back through the received fds is what proves the
        // out-of-band transfer worked, and the random multiplicity exercises multi-fd passing.
        let mut rng = rand::rng();
        let fd_count = rng.random_range(1..=5u32);

        let mut fds: Vec<OwnedFd> = Vec::with_capacity(fd_count as usize);
        let mut total: u64 = 0;
        for _ in 0..fd_count {
            let size = rng.random_range(0..=100usize); // deliberately allows empty files.
            // Random bytes so a mis-routed or truncated fd would be noticeable.
            let mut contents = vec![0u8; size];
            rng.fill_bytes(&mut contents);

            let mut file = tempfile::tempfile().expect("create temp file");
            file.write_all(&contents).expect("write temp file");
            file.flush().expect("flush temp file");
            file.rewind().expect("rewind temp file");

            total += size as u64;
            fds.push(OwnedFd::from(file));
        }

        // The wire frame is two big-endian u32s — the fd count and the combined byte total — with
        // no inline payload; the bytes themselves ride along as the passed descriptors
        // (SCM_RIGHTS).
        let mut frame = Vec::with_capacity(8);
        frame.extend_from_slice(&fd_count.to_be_bytes());
        frame.extend_from_slice(&(total as u32).to_be_bytes());
        write_half.write(&frame, &fds).await?;

        self.total_served.store(total, Ordering::SeqCst);
        self.served.store(true, Ordering::SeqCst);

        Ok(())
    }
}

// -------------------------------------------------------------------------
// Client proxy.
// -------------------------------------------------------------------------

#[zlink::proxy("org.example.LogService")]
trait LogProxy {
    async fn stats(&mut self) -> zlink::Result<Result<StatsReply, LogError>>;

    #[zlink(upgrade)]
    async fn attach(
        self,
        name: &str,
    ) -> zlink::Result<zlink::connection::UpgradeReply<Self::Socket, AttachReply, LogError>>;
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn log_tail_attach() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("logservice.sock");

    let listener = bind(&socket_path)?;
    let served = Arc::new(AtomicBool::new(false));
    let total_served = Arc::new(AtomicU64::new(0));
    let service = LogService {
        served: served.clone(),
        total_served: total_served.clone(),
    };
    let server = Server::new(listener, service);

    let client_fut = async {
        let mut conn = connect(&socket_path).await.unwrap();

        // 1. Prove normal Varlink works before upgrading. `attach` consumes the connection, so
        //    `stats` must run on `&mut conn` first.
        let stats = conn.stats().await.unwrap().unwrap();
        assert_eq!(stats.line_count, STATS_LINE_COUNT);

        // 2. Upgrade: attach to the "main" log stream.
        let res = conn.attach("main").await.unwrap();
        let reply = res.reply.unwrap();
        let params = reply.into_parameters().unwrap();
        assert!(params.ready);

        // 3. Switch to the raw protocol: read the two-u32 header frame plus the fds.
        let mut parts = res.parts;
        let mut read_half = parts.read_half;
        let mut write_half = parts.write_half;
        let mut collected_fds = Vec::new();

        // Ask the server (over the raw protocol) to hand off the logs.
        write_half
            .write(&[REQ_BYTE], &[] as &[OwnedFd])
            .await
            .unwrap();

        // Header: big-endian [fd_count, total_size].
        let mut header = [0u8; 8];
        read_exact_with_fds(
            &mut read_half,
            &mut parts.read_buffer,
            &mut parts.received_fds,
            &mut collected_fds,
            &mut header,
        )
        .await
        .unwrap();
        let announced_fd_count = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let announced_total = u32::from_be_bytes(header[4..].try_into().unwrap()) as u64;

        assert_eq!(
            collected_fds.len(),
            announced_fd_count,
            "received fd count must match the announced count"
        );
        assert!(
            (1..=5).contains(&announced_fd_count),
            "server should hand off 1-5 fds, got {announced_fd_count}"
        );

        // 4. Read every received fd to EOF and sum the bytes; the combined total read back out of
        //    band must equal what the server announced.
        let mut combined = 0u64;
        for fd in collected_fds.drain(..) {
            let mut file = std::fs::File::from(fd);
            let mut contents = Vec::new();
            let n = file.read_to_end(&mut contents).unwrap();
            assert_eq!(n, contents.len());
            combined += n as u64;
        }

        assert_eq!(
            combined, announced_total,
            "combined bytes read through the fds must equal the announced total"
        );
    };

    tokio::select! {
        res = server.run() => { if let Err(e) = res { panic!("Server failed: {e:?}"); } },
        _ = client_fut => {},
    }

    assert!(
        served.load(Ordering::SeqCst),
        "server should have served the log fds via on_upgrade"
    );
    // The total must fit the u32 wire field given the size bounds (max 5 * 100 = 500 bytes).
    assert!(
        total_served.load(Ordering::SeqCst) <= 5 * 100,
        "server total exceeds the documented size bounds"
    );

    Ok(())
}
