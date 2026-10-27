//! Helper functions for Unix sockets: FD passing, peer credentials and Varlink xattr tagging.
//!
//! These are public but hidden from documentation as they're implementation details shared between
//! runtime-specific socket implementations.

use core::mem::MaybeUninit;
use std::{
    io,
    os::fd::{AsFd, BorrowedFd},
};
#[cfg(target_os = "linux")]
use std::{os::fd::OwnedFd, path::Path};

use crate::connection::{Credentials, PassedCredentials, socket::ReadResult};

/// Receive a message from a Unix socket, including any file descriptors.
///
/// This is a low-level helper that performs the `recvmsg` syscall.
#[doc(hidden)]
pub fn recvmsg(fd: impl AsFd, buf: &mut [u8]) -> io::Result<ReadResult> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};
    use std::io::IoSliceMut;

    #[cfg(target_os = "linux")]
    let mut cmsg_buf =
        [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS), ScmCredentials(1))];
    #[cfg(not(target_os = "linux"))]
    let mut cmsg_buf = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = RecvAncillaryBuffer::new(&mut cmsg_buf);

    let mut iov = [IoSliceMut::new(buf)];
    recvmsg(fd.as_fd(), &mut iov, &mut control, RecvFlags::empty())
        .map(|msg| {
            // Extract file descriptors and credentials from ancillary data.
            let mut fds = alloc::vec::Vec::new();
            #[cfg(target_os = "linux")]
            let mut creds = None;
            for m in control.drain() {
                match m {
                    RecvAncillaryMessage::ScmRights(rights) => fds.extend(rights),
                    #[cfg(target_os = "linux")]
                    RecvAncillaryMessage::ScmCredentials(ucreds) => {
                        creds = Some(PassedCredentials::new(ucreds.uid, ucreds.gid, ucreds.pid));
                    }
                    // Ignore the rest (currently non on Linux).
                    _ => (),
                }
            }
            let result = ReadResult::new(msg.bytes);
            #[cfg(feature = "std")]
            let result = result.set_fds(fds);
            #[cfg(all(feature = "std", target_os = "linux"))]
            let result = result.set_credentials(creds);

            result
        })
        .map_err(io::Error::from)
}

/// Enable receiving of peer credentials (`SO_PASSCRED`) on the given unix socket.
///
/// Without this, the kernel discards any `SCM_CREDENTIALS` ancillary data sent by the peer.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn enable_passcred(fd: impl AsFd) -> io::Result<()> {
    rustix::net::sockopt::set_socket_passcred(fd.as_fd(), true).map_err(io::Error::from)
}

/// The role a Unix socket plays in Varlink IPC.
///
/// Recorded in the socket's `user.varlink` extended attribute, following the convention
/// established by systemd's `sd-varlink`, so that tools like `varlinkctl list-sockets` can
/// recognise Varlink sockets.
#[cfg(target_os = "linux")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketRole {
    /// A socket created through `socket()` + `connect()`.
    Client,
    /// A socket returned by `accept()`.
    Server,
    /// A socket that `listen()` is called on.
    Listen,
}

#[cfg(target_os = "linux")]
impl SocketRole {
    fn as_str(self) -> &'static str {
        match self {
            SocketRole::Client => "client",
            SocketRole::Server => "server",
            SocketRole::Listen => "listen",
        }
    }
}

/// Tag a socket with its Varlink role by setting its `user.varlink` extended attribute.
///
/// This is best effort: unsupported kernels and other failures are logged at debug level and
/// otherwise ignored, since tagging is purely informational. Errors are logged by number because
/// `defmt` can only format values implementing its `Format` trait.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn tag_socket(fd: impl AsFd, role: SocketRole) {
    use rustix::fs::{XattrFlags, fsetxattr};

    if !socket_xattr_supported() {
        return;
    }

    if let Err(e) = fsetxattr(
        fd.as_fd(),
        VARLINK_XATTR,
        role.as_str().as_bytes(),
        XattrFlags::empty(),
    ) {
        debug!(
            "Failed to set {} to {} (errno {})",
            VARLINK_XATTR,
            role.as_str(),
            e.raw_os_error()
        );
    }
}

/// Tag a listening socket as `listen` and, when `path` is an absolute path, the socket inode at
/// `path` as `entrypoint`.
///
/// `path` must be the very path that was handed to `bind()`: a path recovered through
/// `getsockname()` may be stale or belong to another namespace, so it must never be used for file
/// system operations. Only an absolute path is used, since a relative one is resolved against the
/// working directory at the time of each call, which may differ from the one `bind()` saw.
/// Listeners adopted from a file descriptor therefore pass `None` and only get the `listen` tag.
///
/// Unlike systemd, which tags the inode between `bind()` and `listen()`, this runs once the socket
/// is already listening, since the standard library performs both steps in one call. Consumers
/// enumerate listening sockets first and read the tag afterwards, so the microsecond-long window
/// at startup is of no practical consequence. Best effort, like [`tag_socket`].
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn tag_listener(fd: impl AsFd, path: Option<&Path>) {
    use rustix::fs::{XattrFlags, lsetxattr};

    if !socket_xattr_supported() {
        return;
    }

    tag_socket(fd, SocketRole::Listen);
    let Some(path) = path.filter(|path| path.is_absolute()) else {
        return;
    };

    if let Err(e) = lsetxattr(path, VARLINK_XATTR, b"entrypoint", XattrFlags::empty()) {
        debug!(
            "Failed to set {} to entrypoint (errno {})",
            VARLINK_XATTR,
            e.raw_os_error()
        );
    }
}

/// Set an extended attribute on the entrypoint socket inode at `path`.
///
/// `path` is the one handed to `bind()`. Like [`tag_listener`], this only ever writes through an
/// absolute path. This backs
/// [`Listener::set_xattr`] for Unix socket listeners; see there for the errors reported.
///
/// [`Listener::set_xattr`]: crate::Listener::set_xattr
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn set_entrypoint_xattr(path: &Path, name: &str, value: &[u8]) -> io::Result<()> {
    use rustix::fs::{XattrFlags, lsetxattr};

    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the entrypoint inode of this listener is not known",
        ));
    }
    if !socket_xattr_supported() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the kernel does not support extended attributes on socket inodes",
        ));
    }

    lsetxattr(path, name, value, XattrFlags::empty()).map_err(io::Error::from)
}

/// The value of the extended attribute `name` of the open file descriptor `fd`.
///
/// Meant for tests to read tags back; the buffer suffices for the values zlink writes.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn read_fxattr(fd: impl AsFd, name: &str) -> io::Result<Vec<u8>> {
    let mut buf = [0u8; 32];
    let len = rustix::fs::fgetxattr(fd.as_fd(), name, &mut buf).map_err(io::Error::from)?;

    Ok(buf[..len].to_vec())
}

/// The value of the extended attribute `name` of the inode at `path`, without following symlinks.
///
/// Meant for tests to read tags back; the buffer suffices for the values zlink writes.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn read_lxattr(path: impl AsRef<Path>, name: &str) -> io::Result<Vec<u8>> {
    let mut buf = [0u8; 32];
    let len = rustix::fs::lgetxattr(path.as_ref(), name, &mut buf).map_err(io::Error::from)?;

    Ok(buf[..len].to_vec())
}

/// Whether the running kernel allows `user.*` extended attributes on socket inodes.
///
/// Linux 7.1 introduced this; older kernels fail with `EPERM`. The probe runs once and its
/// verdict is cached for the lifetime of the process. Only a failure that may be over by the next
/// call, such as a shortage of resources, leaves the question open. Any other failure, such as a
/// security policy denying the probe, would recur on every call and is therefore taken as a lack
/// of support.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn socket_xattr_supported() -> bool {
    use rustix::io::Errno;
    use std::sync::OnceLock;

    static SUPPORTED: OnceLock<bool> = OnceLock::new();

    if let Some(supported) = SUPPORTED.get() {
        return *supported;
    }

    let supported = match probe_socket_xattr() {
        Ok(supported) => supported,
        // Linux keeps the attributes of all sockets in one hash table, and setting one can fail
        // with `EBUSY` while that table is being resized.
        Err(e @ (Errno::MFILE | Errno::NFILE | Errno::NOBUFS | Errno::NOMEM | Errno::BUSY)) => {
            debug!(
                "Failed to probe socket xattr support (errno {}), will retry",
                e.raw_os_error()
            );

            return false;
        }
        Err(e) => {
            debug!(
                "Failed to probe socket xattr support (errno {}), assuming none",
                e.raw_os_error()
            );

            false
        }
    };

    // A concurrent probe reaching the same answer is harmless, so ignore a losing `set`.
    let _ = SUPPORTED.set(supported);

    supported
}

/// Send a message to a Unix socket, including any file descriptors.
///
/// This is a low-level helper that performs the `sendmsg` syscall.
#[doc(hidden)]
pub fn sendmsg(
    fd: impl AsFd,
    buf: &[u8],
    fds: &[BorrowedFd<'_>],
    #[cfg(target_os = "linux")] creds: Option<&crate::connection::PassedCredentials>,
) -> io::Result<usize> {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::io::IoSlice;

    #[cfg(target_os = "linux")]
    let mut cmsg_buf =
        [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS), ScmCredentials(1))];
    #[cfg(not(target_os = "linux"))]
    let mut cmsg_buf = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut cmsg_buf);

    if !fds.is_empty() && !control.push(SendAncillaryMessage::ScmRights(fds)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many file descriptors to send",
        ));
    }

    #[cfg(target_os = "linux")]
    if let Some(creds) = creds {
        let ucred = rustix::net::UCred {
            pid: creds.process_id(),
            uid: creds.unix_user_id(),
            gid: creds.unix_primary_group_id(),
        };
        if !control.push(SendAncillaryMessage::ScmCredentials(ucred)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "failed to push credentials",
            ));
        }
    }

    let iov = [IoSlice::new(buf)];
    sendmsg(fd.as_fd(), &iov, &mut control, SendFlags::empty()).map_err(io::Error::from)
}

/// Get the peer credentials from a Unix socket.
///
/// This is a low-level helper that fetches credentials using platform-specific APIs.
///
/// # Platform Support
///
/// - **Linux/Android**: Uses `SO_PEERCRED` to get uid and pid. On Linux, also gets `SO_PEERPIDFD`
///   for process FD.
/// - **macOS/iOS**: Uses `getpeereid()` for uid and `LOCAL_PEERPID` for pid.
/// - **OpenBSD**: Uses `getpeereid()` for uid and `SO_PEERCRED` for pid.
/// - **NetBSD**: Uses `getpeereid()` for uid and `LOCAL_PEEREID` for pid.
/// - **FreeBSD/DragonFly**: Uses `getpeereid()` for uid. PID is 0 (FIXME: use `LOCAL_PEERCRED`).
pub(crate) fn get_peer_credentials(fd: impl AsFd) -> io::Result<Credentials> {
    use std::os::fd::AsRawFd;

    let fd = fd.as_fd();

    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        use std::os::fd::FromRawFd;

        // Get SO_PEERCRED (uid, gid, pid).
        let ucred = rustix::net::sockopt::socket_peercred(fd)?;
        let uid = ucred.uid;
        let pid = ucred.pid;
        let primary_gid = ucred.gid;

        // Get SO_PEERGROUPS if available (Linux-only).
        #[cfg(target_os = "linux")]
        let supplementary_gids = {
            use rustix::fs::Gid;

            let mut nr_supp_gids = INITIAL_NUMBER_SUPPLEMENTARY_GROUPS;
            let mut nr_supp_gids_in_bytes = nr_supp_gids * (size_of::<Gid>() as u32);
            let mut supp_gids: Vec<Gid> = Vec::with_capacity(nr_supp_gids as usize);

            loop {
                let ret = unsafe {
                    libc::getsockopt(
                        fd.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_PEERGROUPS,
                        supp_gids.as_mut_ptr().cast(),
                        &mut nr_supp_gids_in_bytes,
                    )
                };
                let err = io::Error::last_os_error();

                // We encountered an error which is not about the size of our passed container.
                if ret == -1 && err.raw_os_error() != Some(libc::ERANGE) {
                    return Err(err);
                }

                // If the number of groups returned is less than the requested size, we are done.
                nr_supp_gids = nr_supp_gids_in_bytes / size_of::<Gid>() as u32;
                if nr_supp_gids as usize <= supp_gids.capacity() {
                    supp_gids.shrink_to(nr_supp_gids as usize);
                    // SAFETY: `getsockopt` filled at least `nr_supp_gids` items in the buffer.
                    unsafe { supp_gids.set_len(nr_supp_gids as usize) };
                    break;
                }

                // Otherwise, the vector is too small. Resize and try again.
                // We let the standard Vector speculation over-allocation take place here on
                // purpose.
                supp_gids.reserve(nr_supp_gids as usize - supp_gids.capacity());
                // SAFETY: The number of supplementary GIDs on Linux is bounded 65k which fits in
                // u32.
                nr_supp_gids_in_bytes = (supp_gids.capacity() as u32) * (size_of::<Gid>() as u32);
            }

            supp_gids
        };

        // Get SO_PEERPIDFD if available (Linux-only).
        #[cfg(target_os = "linux")]
        let process_fd = {
            // FIXME: Replace `libc` usage with `rustix` API when it provides SO_PEERPIDFD
            // sockopt: https://github.com/bytecodealliance/rustix/pull/1474
            use core::mem::{MaybeUninit, size_of};

            let mut pidfd = MaybeUninit::<libc::c_int>::zeroed();
            let mut len = size_of::<libc::c_int>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERPIDFD,
                    pidfd.as_mut_ptr().cast(),
                    &mut len,
                )
            };

            // `getsockopt` returns `0` on success or `-1` on error.
            if ret == 0 {
                assert_eq!(
                    len as usize,
                    size_of::<libc::c_int>(),
                    "unexpected getsockopt size"
                );
                let pidfd = unsafe { pidfd.assume_init() };
                Some(unsafe { OwnedFd::from_raw_fd(pidfd) })
            } else {
                let err = io::Error::last_os_error();
                // ENOPROTOOPT means the kernel doesn't support this feature.
                if err.raw_os_error() != Some(libc::ENOPROTOOPT) {
                    return Err(err);
                }
                // No error, but SO_PEERPIDFD is not supported
                None
            }
        };

        #[cfg(target_os = "android")]
        let creds = Credentials::new(PassedCredentials::new(uid, primary_gid, pid));
        #[cfg(target_os = "linux")]
        let creds = Credentials::new(
            PassedCredentials::new(uid, primary_gid, pid),
            supplementary_gids,
            process_fd,
        );

        Ok(creds)
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        // FIXME: Replace with rustix API when it provides the required API:
        // https://github.com/bytecodealliance/rustix/issues/1533
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;

        let ret = unsafe { libc::getpeereid(fd.as_raw_fd(), &mut uid, &mut gid) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }

        let uid = rustix::process::Uid::from_raw(uid);
        let gid = rustix::process::Gid::from_raw(gid);

        // Platform-specific PID fetching.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let pid = {
            let mut pid: libc::pid_t = 0;
            let mut len = core::mem::size_of::<libc::pid_t>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_LOCAL,
                    libc::LOCAL_PEERPID,
                    (&raw mut pid).cast(),
                    &mut len,
                )
            };

            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            rustix::process::Pid::from_raw(pid)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid peer PID"))?
        };

        #[cfg(target_os = "openbsd")]
        let pid = {
            // OpenBSD's SO_PEERCRED returns struct sockpeercred { uid, gid, pid }.
            #[repr(C)]
            struct sockpeercred {
                uid: libc::uid_t,
                gid: libc::gid_t,
                pid: libc::pid_t,
            }

            let mut creds = core::mem::MaybeUninit::<sockpeercred>::zeroed();
            let mut len = core::mem::size_of::<sockpeercred>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    creds.as_mut_ptr().cast(),
                    &mut len,
                )
            };

            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            let creds = unsafe { creds.assume_init() };
            rustix::process::Pid::from_raw(creds.pid)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid peer PID"))?
        };

        #[cfg(target_os = "netbsd")]
        let pid = {
            // NetBSD's LOCAL_PEEREID returns struct unpcbid { pid, euid, egid }.
            #[repr(C)]
            struct unpcbid {
                unp_pid: libc::pid_t,
                unp_euid: libc::uid_t,
                unp_egid: libc::gid_t,
            }

            const LOCAL_PEEREID: libc::c_int = 3;

            let mut creds = core::mem::MaybeUninit::<unpcbid>::zeroed();
            let mut len = core::mem::size_of::<unpcbid>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    0, // SOL_LOCAL
                    LOCAL_PEEREID,
                    creds.as_mut_ptr().cast(),
                    &mut len,
                )
            };

            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            let creds = unsafe { creds.assume_init() };
            rustix::process::Pid::from_raw(creds.unp_pid)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid peer PID"))?
        };

        // FIXME: FreeBSD 13+ has cr_pid in xucred, DragonFly status unknown.
        #[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
        let pid = rustix::process::Pid::from_raw(0).unwrap();

        Ok(Credentials::new(PassedCredentials::new(uid, gid, pid)))
    }

    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials not supported on this platform",
        ))
    }
}

/// Probe whether `user.*` extended attributes can be set on a fresh socket.
///
/// Errors known to mean that the kernel lacks support yield `Ok(false)`; any other error is
/// returned for the caller to judge.
///
/// The probe uses a socket of the same kind as the Varlink sockets that get tagged, since a
/// security policy confining the process grants permissions per kind of socket. A process allowed
/// to use Varlink sockets is then usually allowed to create the probe too, and setting the
/// attribute on it succeeds or fails as tagging them would.
#[cfg(target_os = "linux")]
fn probe_socket_xattr() -> rustix::io::Result<bool> {
    use rustix::{
        fs::{XattrFlags, fsetxattr},
        io::Errno,
        net::{AddressFamily, SocketFlags, SocketType, socket_with},
    };

    let fd = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )?;

    match fsetxattr(&fd, "user.zlink.probe", b"1", XattrFlags::empty()) {
        Ok(()) => Ok(true),
        Err(Errno::PERM | Errno::OPNOTSUPP | Errno::NOSYS) => Ok(false),
        Err(e) => Err(e),
    }
}

/// The extended attribute holding a Varlink socket's role or entrypoint marker.
#[cfg(target_os = "linux")]
const VARLINK_XATTR: &str = "user.varlink";

/// The maximum number of file descriptors that can be sent in a single message.
///
/// The value is based on what is used in `zbus`, which comes from sdbus.
const MAX_FDS: usize = 1024;

// Linux can go up to NGROUPS_MAX supplementary groups (65K). It is safe to assume that
// most users will have a couple of supplementary groups by default. We allocate 128
// because integers are tiny.
#[cfg(target_os = "linux")]
const INITIAL_NUMBER_SUPPLEMENTARY_GROUPS: libc::socklen_t = 128 as libc::socklen_t;

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::{
        linux::net::SocketAddrExt,
        unix::net::{SocketAddr, UnixListener, UnixStream},
    };

    use super::*;

    #[test]
    fn socket_role_tags_round_trip() {
        let (a, b) = UnixStream::pair().unwrap();
        tag_socket(&a, SocketRole::Client);
        tag_socket(&b, SocketRole::Server);

        let a_value = read_fxattr(&a, VARLINK_XATTR);
        let b_value = read_fxattr(&b, VARLINK_XATTR);

        if socket_xattr_supported() {
            assert_eq!(a_value.unwrap(), b"client");
            assert_eq!(b_value.unwrap(), b"server");
        } else {
            assert!(a_value.is_err());
            assert!(b_value.is_err());
        }
    }

    #[test]
    fn listener_tags_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&path).unwrap();
        tag_listener(&listener, Some(&path));

        let listen_value = read_fxattr(&listener, VARLINK_XATTR);
        let entrypoint_value = read_lxattr(&path, VARLINK_XATTR);

        if socket_xattr_supported() {
            assert_eq!(listen_value.unwrap(), b"listen");
            assert_eq!(entrypoint_value.unwrap(), b"entrypoint");
        } else {
            assert!(listen_value.is_err());
            assert!(entrypoint_value.is_err());
        }
    }

    #[test]
    fn listener_without_path_tags_only_the_socket() {
        let name = format!(
            "zlink-{}-listener_without_path_tags_only_the_socket",
            std::process::id()
        );
        let addr = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let listener = UnixListener::bind_addr(&addr).unwrap();

        // There is no inode to tag; only the socket itself gets a role.
        tag_listener(&listener, None);

        let listen_value = read_fxattr(&listener, VARLINK_XATTR);
        if socket_xattr_supported() {
            assert_eq!(listen_value.unwrap(), b"listen");
        } else {
            assert!(listen_value.is_err());
        }
    }

    #[test]
    fn relative_path_is_never_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&path).unwrap();

        // The inode exists, but a relative path could resolve elsewhere by the time it is used,
        // so neither helper touches the file system with one.
        tag_listener(&listener, Some(Path::new("test.sock")));
        assert!(read_lxattr(&path, VARLINK_XATTR).is_err());

        let err =
            set_entrypoint_xattr(Path::new("test.sock"), "user.zlink.test", b"1").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn set_entrypoint_xattr_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sock");
        let _listener = UnixListener::bind(&path).unwrap();

        let result = set_entrypoint_xattr(&path, "user.zlink.test", b"1");

        if socket_xattr_supported() {
            result.unwrap();
            assert_eq!(read_lxattr(&path, "user.zlink.test").unwrap(), b"1");
        } else {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
        }
    }
}
