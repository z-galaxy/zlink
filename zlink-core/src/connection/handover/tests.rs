use core::{
    future::Future,
    mem,
    pin::pin,
    task::{Context, Waker},
};
use std::{
    io::{PipeReader, Read, Write},
    os::unix::net::UnixStream,
};

use serde::Deserialize;
use serde_json::{Value, json};
use zoter::{Restore, Save};

use super::*;
use crate::{
    Error,
    connection::socket::Socket,
    test_utils::mock_socket::{MockSocket, MockWriteHalf},
};

/// A hand-over comes back from its record and its file descriptors as it was: the socket, what
/// was read and enqueued, the file descriptors that go with them, each where it was, and the
/// credentials that came with the latest read.
#[test]
fn record_round_trip() {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    let (read_a, write_a) = pipe();
    let (read_b, write_b) = pipe();
    let (read_c, write_c) = pipe();
    let (read_d, write_d) = pipe();
    let handover = Handover {
        socket,
        unreceived: b"{\"method\":\"a\"}\0{\"method".to_vec(),
        unreceived_fds: [
            PendingFds {
                pos: 3,
                fds: vec![write_a],
            },
            PendingFds {
                pos: 22,
                fds: vec![write_b, write_c],
            },
        ]
        .into(),
        received_credentials: Some(Arc::new(PassedCredentials::new(
            Uid::from_raw(1000),
            Gid::from_raw(100),
            Pid::from_raw(4242).unwrap(),
        ))),
        unwritten: b"{\"parameters\":{}}\0{}\0".to_vec(),
        unwritten_fds: [MessageFds {
            fds: vec![write_d],
            offset: 18,
            len: 3,
        }]
        .into(),
    };

    let mut restored = round_trip(&handover).unwrap();
    assert_eq!(restored.unreceived, handover.unreceived);
    assert_eq!(restored.unwritten, handover.unwritten);
    let creds = restored.received_credentials.as_deref().unwrap();
    assert_eq!(creds.unix_user_id().as_raw(), 1000);
    assert_eq!(creds.unix_primary_group_id().as_raw(), 100);
    assert_eq!(creds.process_id().as_raw_pid(), 4242);
    let positions: Vec<_> = restored
        .unreceived_fds
        .iter()
        .map(|pending| (pending.pos, pending.fds.len()))
        .collect();
    assert_eq!(positions, [(3, 1), (22, 2)]);
    let [msg] = <[_; 1]>::try_from(Vec::from(mem::take(&mut restored.unwritten_fds))).unwrap();
    assert_eq!((msg.offset, msg.len), (18, 3));
    // Each file descriptor is of the file that it was of.
    let restored_fds = restored
        .unreceived_fds
        .iter()
        .flat_map(|pending| &pending.fds)
        .chain(&msg.fds);
    for (fd, read) in restored_fds.zip([read_a, read_b, read_c, read_d]) {
        check_pipe(fd, read);
    }
    // So is the socket.
    restored.socket.write_all(b"ping").unwrap();
    let mut buf = [0; 4];
    peer.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
}

/// The record is the JSON object that the module's documentation shows.
#[test]
fn record_is_json() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let handover = Handover {
        socket,
        unreceived: b"{\"method\":\"a\"}\0{\"method".to_vec(),
        unreceived_fds: [PendingFds {
            pos: 3,
            fds: vec![pipe().1],
        }]
        .into(),
        received_credentials: Some(Arc::new(PassedCredentials::new(
            Uid::from_raw(1000),
            Gid::from_raw(100),
            Pid::from_raw(4242).unwrap(),
        ))),
        unwritten: b"{\"parameters\":{}}\0".to_vec(),
        unwritten_fds: [MessageFds {
            fds: vec![pipe().1],
            offset: 0,
            len: 18,
        }]
        .into(),
    };

    let record: Value = serde_json::from_slice(&record(&handover)).unwrap();
    assert_eq!(
        record,
        json!({
            "version": 1,
            "unreceived": "eyJtZXRob2QiOiJhIn0AeyJtZXRob2Q=",
            "unreceivedFds": [{ "pos": 3, "count": 1 }],
            "receivedCredentials": { "pid": 4242, "uid": 1000, "gid": 100 },
            "unwritten": "eyJwYXJhbWV0ZXJzIjp7fX0A",
            "unwrittenFds": [{ "offset": 0, "len": 18, "count": 1 }]
        })
    );
}

/// A hand-over of a connection with nothing left to receive or to write.
#[test]
fn record_of_nothing_left() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let handover = Handover {
        socket,
        unreceived: Vec::new(),
        unreceived_fds: VecDeque::new(),
        received_credentials: None,
        unwritten: Vec::new(),
        unwritten_fds: VecDeque::new(),
    };
    let record: Value = serde_json::from_slice(&record(&handover)).unwrap();
    assert_eq!(
        record,
        json!({
            "version": 1,
            "unreceived": "",
            "unreceivedFds": [],
            "receivedCredentials": null,
            "unwritten": "",
            "unwrittenFds": []
        })
    );
    assert_eq!(handover.fds().len(), 1);

    let restored = round_trip(&handover).unwrap();
    assert!(restored.unreceived.is_empty() && restored.unwritten.is_empty());
    assert!(restored.received_credentials.is_none());
}

/// A record that isn't of a hand-over, or whose file descriptors don't match it, is refused, as is
/// one that a later version of zlink made.
#[test]
fn refused_records() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let handover = Handover {
        socket,
        unreceived: b"{}\0".to_vec(),
        unreceived_fds: [PendingFds {
            pos: 0,
            fds: vec![pipe().1],
        }]
        .into(),
        received_credentials: None,
        unwritten: Vec::new(),
        unwritten_fds: VecDeque::new(),
    };
    let record = record(&handover);
    let fds = || -> Vec<OwnedFd> {
        handover
            .fds()
            .iter()
            .map(|fd| fd.try_clone_to_owned().unwrap())
            .collect()
    };
    let credentials = |pid: i32, uid: u32, gid: u32| {
        edited(&record, |json| {
            json["receivedCredentials"] = json!({ "pid": pid, "uid": uid, "gid": gid });
        })
    };

    let malformed = [
        // Not JSON.
        b"hand-over".to_vec(),
        // Not an object.
        b"[]".to_vec(),
        // Of no version.
        edited(&record, |json| {
            json.as_object_mut().unwrap().remove("version");
        }),
        // Of a version before the first.
        edited(&record, |json| json["version"] = 0.into()),
        // What was read, not in base64.
        edited(&record, |json| json["unreceived"] = "{}".into()),
        // A list of file descriptors that came with a byte past what was read.
        edited(&record, |json| json["unreceivedFds"][0]["pos"] = 3.into()),
        // More file descriptors in the list than there are.
        edited(&record, |json| json["unreceivedFds"][0]["count"] = 2.into()),
        // A list of file descriptors of a message past what was enqueued.
        edited(&record, |json| {
            json["unwrittenFds"] = json!([{ "offset": 0, "len": 1, "count": 0 }]);
        }),
        // Credentials of no process.
        credentials(0, 1000, 100),
        credentials(-1, 1000, 100),
        // Credentials of no user.
        credentials(1, u32::MAX, 100),
        // Credentials of no group.
        credentials(1, 1000, u32::MAX),
        // Cut short.
        record[..record.len() - 1].to_vec(),
    ];
    for record in malformed {
        assert_eq!(
            kind(Handover::restore(&record, fds())),
            io::ErrorKind::InvalidData
        );
    }
    // Too many file descriptors for the record.
    let mut more = fds();
    more.push(pipe().1);
    assert_eq!(
        kind(Handover::restore(&record, more)),
        io::ErrorKind::InvalidData
    );
    // A first file descriptor that isn't a Unix domain stream socket.
    let mut not_a_socket = fds();
    not_a_socket[0] = pipe().1;
    let refused = Handover::restore(&record, not_a_socket);
    assert_eq!(kind(refused), io::ErrorKind::InvalidData);

    // One of a later version.
    let later = edited(&record, |json| json["version"] = (VERSION + 1).into());
    assert_eq!(
        kind(Handover::restore(&later, fds())),
        io::ErrorKind::Unsupported
    );

    // A later version can add fields, which this one ignores.
    let longer = edited(&record, |json| json["later"] = true.into());
    let restored = Handover::restore(&longer, fds()).unwrap();
    assert_eq!(restored.unreceived, b"{}\0");
    // Credentials of a process, a user and a group come back.
    let restored = Handover::restore(&credentials(1, 0, 0), fds()).unwrap();
    let creds = restored.received_credentials.unwrap();
    assert_eq!(creds.process_id().as_raw_pid(), 1);
}

/// A record is only saved into memory of its length.
#[test]
fn record_saved_into_memory_of_its_length() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let handover = Handover {
        socket,
        unreceived: Vec::new(),
        unreceived_fds: VecDeque::new(),
        received_credentials: None,
        unwritten: Vec::new(),
        unwritten_fds: VecDeque::new(),
    };
    let len = handover.saved_len().unwrap();
    for len in [len - 1, len + 1] {
        let error = handover.save(&mut vec![0; len]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

/// The messages that a connection read but didn't receive come first in the connection that goes
/// on from it, along with their file descriptors, before what it reads.
#[tokio::test]
async fn unreceived_messages_come_first() {
    // Two whole calls and the start of a third, whose file descriptors came with the last byte of
    // the first read, and the end of the third.
    let reads: [&[u8]; 2] = [
        b"{\"method\":\"org.example.A\"}\0{\"method\":\"org.example.B\"}\0{\"method\":",
        b"\"org.example.C\"}\0",
    ];
    let (read_end, write_end) = pipe();
    let (read, _) = MockSocket::with_reads(&reads, vec![vec![write_end], Vec::new()]).split();
    let mut conn = ReadConnection::new(read, 0);
    let (call, fds) = conn.receive_call::<Method>().await.unwrap();
    assert_eq!((call.method(), fds.len()), (&Method::A, 0));

    let (read, unreceived, fds) = conn.into_unreceived();
    assert_eq!(&unreceived[..10], b"{\"method\":");
    let mut conn = ReadConnection::with_unreceived(read, 1, unreceived, fds, None);
    let (call, fds) = conn.receive_call::<Method>().await.unwrap();
    assert_eq!((call.method(), fds.len()), (&Method::B, 0));
    let (call, fds) = conn.receive_call::<Method>().await.unwrap();
    assert_eq!(call.method(), &Method::C);
    let [fd] = <[_; 1]>::try_from(fds).unwrap();
    check_pipe(&fd, read_end);
}

/// What a connection enqueued but didn't write goes first in the connection that goes on from it,
/// along with the file descriptors of its messages.
#[tokio::test]
async fn unwritten_messages_go_first() {
    let (_, write) = MockSocket::with_responses(&[]).split();
    let mut conn = WriteConnection::new(write, 0);
    let (read_end, write_end) = pipe();
    conn.enqueue(&Method::A, vec![write_end]).unwrap();
    conn.enqueue(&Method::B, Vec::new()).unwrap();

    let (_, unwritten, fds) = conn.into_unwritten();
    let mut conn = WriteConnection::with_unwritten(MockWriteHalf::new(), 1, unwritten, fds);
    conn.write(&Method::C, Vec::new()).await.unwrap();
    let write = conn.write_half();
    assert_eq!(
        write.written_data(),
        b"{\"method\":\"org.example.A\"}\0{\"method\":\"org.example.B\"}\0\
          {\"method\":\"org.example.C\"}\0"
    );
    let written = write.fds_written();
    let [fds] = written.as_slice() else {
        panic!("{written:?}");
    };
    let [fd] = fds.as_slice() else {
        panic!("{fds:?}");
    };
    check_pipe(fd, read_end);
}

/// What a flush wrote before it was dropped doesn't go again: the connection that goes on from it
/// writes the rest, along with the file descriptors that didn't go yet.
#[tokio::test]
async fn partly_written_messages_go_on_from_where_they_stopped() {
    let mut conn = WriteConnection::new(MockWriteHalf::new().writing_in_chunks(4), 0);
    let (read_a, write_a) = pipe();
    let (read_b, write_b) = pipe();
    conn.enqueue(&Method::A, vec![write_a]).unwrap();
    conn.enqueue(&Method::B, vec![write_b]).unwrap();
    // The first write takes 4 bytes of the first message, with its file descriptor, and the flush
    // is dropped as the second waits for room.
    let mut cx = Context::from_waker(Waker::noop());
    assert!(pin!(conn.flush()).poll(&mut cx).is_pending());

    let (write, unwritten, fds) = conn.into_unwritten();
    assert_eq!(write.written_data(), b"{\"me");
    check_written_fd(&write, read_a);
    assert_eq!(
        unwritten,
        b"thod\":\"org.example.A\"}\0{\"method\":\"org.example.B\"}\0"
    );
    let positions: Vec<_> = fds.iter().map(|msg| (msg.offset, msg.len)).collect();
    assert_eq!(positions, [(23, 27)]);

    let mut conn = WriteConnection::with_unwritten(MockWriteHalf::new(), 1, unwritten, fds);
    conn.flush().await.unwrap();
    let write = conn.write_half();
    assert_eq!(
        write.written_data(),
        b"thod\":\"org.example.A\"}\0{\"method\":\"org.example.B\"}\0"
    );
    assert_eq!(write.fd_positions(), [23]);
    check_written_fd(write, read_b);
}

/// A message of a record that comes with a list of no file descriptors is written as any other,
/// in as many writes as it takes, and so are the messages after it.
#[tokio::test]
async fn unwritten_messages_with_no_fds_go() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let (read_b, write_b) = pipe();
    let unwritten = b"{\"method\":\"org.example.A\"}\0{\"method\":\"org.example.B\"}\0";
    let handover = Handover {
        socket,
        unreceived: Vec::new(),
        unreceived_fds: VecDeque::new(),
        received_credentials: None,
        unwritten: unwritten.to_vec(),
        unwritten_fds: [
            MessageFds {
                fds: Vec::new(),
                offset: 0,
                len: 27,
            },
            MessageFds {
                fds: vec![write_b],
                offset: 27,
                len: 27,
            },
        ]
        .into(),
    };
    let restored = round_trip(&handover).unwrap();

    let write = MockWriteHalf::new().writing_in_chunks(5);
    let mut conn =
        WriteConnection::with_unwritten(write, 1, restored.unwritten, restored.unwritten_fds);
    conn.flush().await.unwrap();
    let write = conn.write_half();
    assert_eq!(write.written_data(), unwritten);
    assert_eq!(write.fd_positions(), [27]);
    check_written_fd(write, read_b);
}

#[derive(Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(tag = "method")]
enum Method {
    #[serde(rename = "org.example.A")]
    A,
    #[serde(rename = "org.example.B")]
    B,
    #[serde(rename = "org.example.C")]
    C,
}

/// `handover`, out of its record and duplicates of its file descriptors.
fn round_trip(handover: &Handover) -> crate::Result<Handover> {
    let fds = handover
        .fds()
        .iter()
        .map(|fd| fd.try_clone_to_owned().unwrap())
        .collect();

    Handover::restore(&record(handover), fds)
}

/// The record of `handover`.
fn record(handover: &Handover) -> Vec<u8> {
    let mut record = vec![0; handover.saved_len().unwrap()];
    handover.save(&mut record).unwrap();

    record
}

/// `record`, with its JSON edited by `edit`.
fn edited(record: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut json = serde_json::from_slice(record).unwrap();
    edit(&mut json);

    serde_json::to_vec(&json).unwrap()
}

/// The kind of the error of a restore that failed.
fn kind(restored: crate::Result<Handover>) -> io::ErrorKind {
    match restored {
        Err(Error::Io(e)) => e.kind(),
        other => panic!("{other:?}"),
    }
}

/// The two ends of a pipe, the write end as a file descriptor.
fn pipe() -> (PipeReader, OwnedFd) {
    let (read, write) = std::io::pipe().unwrap();

    (read, write.into())
}

/// Check that `write` wrote a single list of file descriptors, of the write end of the pipe that
/// `read` is the read end of.
fn check_written_fd(write: &MockWriteHalf, read: PipeReader) {
    let written = write.fds_written();
    let [fds] = written.as_slice() else {
        panic!("{written:?}");
    };
    let [fd] = fds.as_slice() else {
        panic!("{fds:?}");
    };
    check_pipe(fd, read);
}

/// Check that `fd` is of the write end of the pipe that `read` is the read end of.
fn check_pipe(fd: &OwnedFd, mut read: PipeReader) {
    let mut write = std::fs::File::from(fd.try_clone().unwrap());
    write.write_all(b"x").unwrap();
    drop(write);
    let mut byte = [0];
    read.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"x");
}
