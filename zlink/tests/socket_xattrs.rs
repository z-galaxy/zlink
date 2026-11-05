//! Integration tests for the `user.varlink` tagging of Unix sockets and `Listener::set_xattr`.
//!
//! Kernels older than Linux 7.1 refuse extended attributes on sockets, so every test asserts the
//! tags when the kernel supports them and their absence (with tagging staying out of the way)
//! when it does not.

#![cfg(all(feature = "server", target_os = "linux"))]

use std::os::{
    fd::{AsFd, OwnedFd},
    unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream},
};

use tempfile::TempDir;
use zlink::{
    Error, Listener as _,
    unix_utils::{read_fxattr, read_lxattr, socket_xattr_supported},
};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix;
#[cfg(feature = "tokio")]
use zlink::tokio::unix;
#[cfg(all(feature = "zruntime", not(any(feature = "tokio", feature = "smol"))))]
use zlink::zruntime::unix;

#[tokio::test]
async fn sockets_carry_varlink_role() {
    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("roles.sock");

    let mut listener = unix::bind(&socket_path).unwrap();
    // A Unix socket connect completes as soon as it is queued in the listener's backlog.
    let client = unix::connect(&socket_path).await.unwrap();
    let accepted = listener.accept().await.unwrap().unwrap();

    let listener_value = read_fxattr(&listener, VARLINK_XATTR);
    let path_value = read_lxattr(&socket_path, VARLINK_XATTR);
    let (client_read, _) = client.split();
    let client_value = read_fxattr(client_read.read_half().as_fd(), VARLINK_XATTR);
    let (accepted_read, _) = accepted.split();
    let accepted_value = read_fxattr(accepted_read.read_half().as_fd(), VARLINK_XATTR);

    if socket_xattr_supported() {
        assert_eq!(listener_value.unwrap(), b"listen");
        assert_eq!(path_value.unwrap(), b"entrypoint");
        assert_eq!(client_value.unwrap(), b"client");
        assert_eq!(accepted_value.unwrap(), b"server");
    } else {
        assert!(listener_value.is_err());
        assert!(path_value.is_err());
        assert!(client_value.is_err());
        assert!(accepted_value.is_err());
    }
}

#[tokio::test]
async fn adopted_listener_tags_only_the_socket() {
    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("adopted.sock");

    let fd: OwnedFd = StdUnixListener::bind(&socket_path).unwrap().into();
    let listener = unix::Listener::try_from(fd).unwrap();

    let listener_value = read_fxattr(&listener, VARLINK_XATTR);
    if socket_xattr_supported() {
        assert_eq!(listener_value.unwrap(), b"listen");
    } else {
        assert!(listener_value.is_err());
    }
    // The path the socket was bound to is not known, so the inode is left alone.
    assert!(read_lxattr(&socket_path, VARLINK_XATTR).is_err());
}

#[tokio::test]
async fn set_xattr_on_entrypoint() {
    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("custom.sock");

    let listener = unix::bind(&socket_path).unwrap();
    let result = listener.set_xattr("user.zlink.test", "1");

    if socket_xattr_supported() {
        result.unwrap();
        assert_eq!(read_lxattr(&socket_path, "user.zlink.test").unwrap(), b"1");
    } else {
        match result.unwrap_err() {
            Error::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::Unsupported),
            e => panic!("unexpected error: {e:?}"),
        }
    }
}

#[tokio::test]
async fn relative_bind_path_is_never_used_for_xattrs() {
    let bind_dir = TempDir::new().unwrap();
    let other_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().unwrap();

    std::env::set_current_dir(bind_dir.path()).unwrap();
    let listener = unix::bind("relative.sock").unwrap();
    // A regular file where a naive relative lookup would land after the directory change. It
    // accepts `user.*` attributes on every kernel, so it would expose a misdirected write.
    std::env::set_current_dir(other_dir.path()).unwrap();
    let decoy = other_dir.path().join("relative.sock");
    std::fs::write(&decoy, b"").unwrap();

    let result = listener.set_xattr("user.zlink.test", "1");
    std::env::set_current_dir(original_dir).unwrap();

    // A relative path is not trusted at all: the socket inode is not tagged, no attribute is
    // written anywhere and the caller learns why.
    assert!(read_lxattr(bind_dir.path().join("relative.sock"), VARLINK_XATTR).is_err());
    assert!(read_lxattr(&decoy, "user.zlink.test").is_err());
    match result.unwrap_err() {
        Error::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
        e => panic!("unexpected error: {e:?}"),
    }
}

#[tokio::test]
async fn ready_listener_does_not_support_xattrs() {
    let (socket, _peer) = StdUnixStream::pair().unwrap();
    let listener = zlink::ReadyListener::new(unix::Stream::try_from(socket).unwrap());

    match listener.set_xattr("user.zlink.test", "1").unwrap_err() {
        Error::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::Unsupported),
        e => panic!("unexpected error: {e:?}"),
    }
}

#[tokio::test]
async fn set_xattr_on_adopted_listener_fails() {
    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("adopted.sock");

    let fd: OwnedFd = StdUnixListener::bind(&socket_path).unwrap().into();
    let listener = unix::Listener::try_from(fd).unwrap();

    match listener.set_xattr("user.zlink.test", "1").unwrap_err() {
        Error::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
        e => panic!("unexpected error: {e:?}"),
    }
}

const VARLINK_XATTR: &str = "user.varlink";
