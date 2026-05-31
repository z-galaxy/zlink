use crate::{Call, Connection, test_utils::mock_socket::MockSocket};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct DummyUpgradeCall {
    #[serde(rename = "upgrade")]
    upgrade: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct DummyUpgradeReply {
    success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
enum DummyError {}

#[tokio::test]
async fn test_upgrade_and_pipelining() {
    // Construct a response containing the upgrade reply terminated by a *single* Varlink `\0`,
    // immediately followed by raw, pipelined custom-protocol bytes that START with `\0` bytes —
    // e.g. a big-endian `u32` frame count `0x00000001` (`[0, 0, 0, 1]`) plus a payload byte.
    //
    // This is the realistic case that the old sentinel-scanning logic corrupted: it mistook the
    // leading `\0` of the raw frame for its own end-of-burst sentinel and dropped/discarded it.
    // The leftover bytes must now be preserved verbatim.
    let mut response_bytes = b"{\"parameters\":{\"success\":true}}".to_vec();
    response_bytes.push(0); // single Varlink message terminator
    response_bytes.extend_from_slice(&[0, 0, 0, 1, 42]); // raw frame: big-endian u32 = 1, payload

    // Convert the bytes to a string representation for MockSocket::with_responses.
    // MockSocket null-terminates the message and, as it is the last one, appends another trailing
    // `\0` (so the whole burst ends in `\0\0`, which the reader needs to detect end-of-burst).
    let response_str = unsafe { std::str::from_utf8_unchecked(&response_bytes) };

    let socket = MockSocket::with_responses(&[response_str]);
    let connection = Connection::new(socket);

    let call = Call::new(DummyUpgradeCall { upgrade: true }).set_upgrade(true);

    // Call upgrade which consumes the connection
    let upgrade_reply = connection
        .call_upgrade::<_, DummyUpgradeReply, DummyError>(&call, vec![])
        .await
        .unwrap();

    // Verify the reply was parsed correctly
    let reply = upgrade_reply.reply.unwrap();
    assert!(reply.parameters().unwrap().success);

    // The leftover raw bytes must be preserved verbatim, including the leading `\0`s of the frame
    // count. The two trailing `\0`s are MockSocket's null-termination of the (single) response.
    let parts = upgrade_reply.parts;
    assert_eq!(parts.read_buffer, vec![0, 0, 0, 1, 42, 0, 0]);
}
