use serde::Deserialize;
use std::os::{fd::OwnedFd, unix::net::UnixStream};
use zlink::{Connection, connection::socket::ReadHalf, proxy, test_utils::mock_socket::MockSocket};

#[proxy("org.example.upgrade")]
trait UpgradeProxy {
    #[zlink(upgrade)]
    async fn download(&mut self, name: &str) -> zlink::Result<Result<Download, DownloadError>>;

    #[zlink(upgrade, return_fds)]
    async fn download_with_fds(
        &mut self,
        name: &str,
    ) -> zlink::Result<(Result<Download, DownloadError>, Vec<OwnedFd>)>;
}

#[derive(Debug, Deserialize)]
struct Download {
    size: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "error")]
enum DownloadError {
    #[serde(rename = "org.example.upgrade.NotFound")]
    NotFound,
}

#[tokio::test]
async fn upgrade_method() {
    // The reply is followed by the data of another protocol, which starts with a NUL byte.
    let reads: [&[u8]; 1] = [b"{\"parameters\":{\"size\":3}}\0\0ab"];
    let socket = MockSocket::with_reads(&reads, vec![]);
    let mut conn = Connection::new(socket);

    let download = conn.download("blob").await.unwrap().unwrap();
    assert_eq!(download.size, 3);

    let (read, write) = conn.split();
    let mut read = read.into_read_half();
    assert_eq!(read_all(&mut read).await, [(b"\0ab".to_vec(), 0)]);
    let write = write.into_write_half();
    let written = write.written_data();
    let call: serde_json::Value = serde_json::from_slice(&written[..written.len() - 1]).unwrap();
    assert_eq!(call["method"], "org.example.upgrade.Download");
    assert_eq!(call["parameters"]["name"], "blob");
    assert_eq!(call["upgrade"], true);
}

#[tokio::test]
async fn upgrade_method_refused() {
    // An error reply refuses the upgrade, leaving a Varlink connection.
    let reads: [&[u8]; 2] = [
        b"{\"error\":\"org.example.upgrade.NotFound\"}\0",
        b"{\"parameters\":{\"size\":3}}\0",
    ];
    let socket = MockSocket::with_reads(&reads, vec![]);
    let mut conn = Connection::new(socket);

    let error = conn.download("blob").await.unwrap().unwrap_err();
    assert!(matches!(error, DownloadError::NotFound));

    let download = conn.download("blob").await.unwrap().unwrap();
    assert_eq!(download.size, 3);
}

#[tokio::test]
async fn upgrade_method_with_fds() {
    // The FD comes along with the first byte of the reply, which is all that the first read
    // returns. The second read returns the rest of the reply and the data of another protocol.
    let (fd, _other_end) = UnixStream::pair().unwrap();
    let reads: [&[u8]; 2] = [b"{\"parameters\":{\"size\":3}}", b"\0raw"];
    let socket = MockSocket::with_reads(&reads, vec![vec![fd.into()]]);
    let mut conn = Connection::new(socket);

    let (download, fds) = conn.download_with_fds("blob").await.unwrap();
    assert_eq!(download.unwrap().size, 3);
    assert_eq!(fds.len(), 1);

    let (read, _write) = conn.split();
    let mut read = read.into_read_half();
    assert_eq!(read_all(&mut read).await, [(b"raw".to_vec(), 0)]);
}

/// The data and the number of FDs of each read from `read`, until the end of the data.
async fn read_all<R>(read: &mut R) -> Vec<(Vec<u8>, usize)>
where
    R: ReadHalf,
{
    let mut buf = [0; 64];
    let mut reads = Vec::new();
    loop {
        let result = read.read(&mut buf).await.unwrap();
        let bytes_read = result.bytes_read();
        if bytes_read == 0 {
            return reads;
        }
        reads.push((buf[..bytes_read].to_vec(), result.fds().len()));
    }
}
