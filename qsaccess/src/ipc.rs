// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Single instance. The first `qsaccess` to start listens on a per-user
//! endpoint; a later one sends it its arguments and exits, or (`qsaccess
//! run`) stays on the link to read what the instance sends back.
//!
//! - Windows: the named pipe `\\.\pipe\quartz-access-<user SID>`, made with
//!   `PIPE_REJECT_REMOTE_CLIENTS` and a DACL that lets the current user
//!   alone open it.
//! - Linux: the Unix socket `$XDG_RUNTIME_DIR/quartz-access.sock`, else
//!   `quartz-access.sock` in the settings directory; mode 0600. A stale
//!   socket left by a crashed instance is removed.
//!
//! Both ends check who is at the other before anything is read: the
//! client process's user SID on Windows, `SO_PEERCRED`'s uid on Linux,
//! which must be this process's user ([`Peer::current`]). A peer of another
//! user is refused, its link closed unread, and `QAD8040` logged.
//!
//! A link carries frames both ways: `QSA2`, the length of the rest, the
//! argument count, then each argument as a length and UTF-8 bytes. Lengths
//! are little-endian `u32`. Nothing secret ever goes over a link: a session
//! token stays in the instance that holds it.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Frame magic.
const MAGIC: &[u8; 4] = b"QSA2";
/// Largest frame accepted.
pub const MAX_FRAME: usize = 64 * 1024;

/// Encodes arguments as one frame.
pub fn encode(args: &[String]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(args.len() as u32).to_le_bytes());
    for a in args {
        body.extend_from_slice(&(a.len() as u32).to_le_bytes());
        body.extend_from_slice(a.as_bytes());
    }
    let mut out = Vec::from(&MAGIC[..]);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad frame: {what}"))
}

/// Decodes one whole frame.
pub fn decode(frame: &[u8]) -> io::Result<Vec<String>> {
    if frame.len() > MAX_FRAME {
        return Err(bad("too long"));
    }
    let (magic, rest) = frame.split_at_checked(4).ok_or_else(|| bad("short"))?;
    if magic != MAGIC {
        return Err(bad("magic"));
    }
    let (len, body) = rest.split_at_checked(4).ok_or_else(|| bad("short"))?;
    if u32::from_le_bytes(len.try_into().expect("four bytes")) as usize != body.len() {
        return Err(bad("length"));
    }
    decode_body(body)
}

fn decode_body(mut rest: &[u8]) -> io::Result<Vec<String>> {
    let u32_at = |rest: &mut &[u8]| -> io::Result<usize> {
        let (n, r) = rest.split_at_checked(4).ok_or_else(|| bad("short"))?;
        *rest = r;
        Ok(u32::from_le_bytes(n.try_into().expect("four bytes")) as usize)
    };
    let count = u32_at(&mut rest)?;
    if count > MAX_FRAME / 4 {
        return Err(bad("count"));
    }
    let mut args = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u32_at(&mut rest)?;
        let (s, r) = rest.split_at_checked(len).ok_or_else(|| bad("short"))?;
        rest = r;
        args.push(String::from_utf8(s.to_vec()).map_err(|_| bad("utf-8"))?);
    }
    if !rest.is_empty() {
        return Err(bad("trailing bytes"));
    }
    Ok(args)
}

/// Reads the next frame; `None` when the other end closed between frames.
fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<String>>> {
    let mut head = [0u8; 8];
    let mut got = 0;
    while got < head.len() {
        match r.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(bad("short")),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if got == 0 && is_closed(&e) => return Ok(None),
            Err(e) => return Err(e),
        }
    }
    if &head[..4] != MAGIC {
        return Err(bad("magic"));
    }
    let len = u32::from_le_bytes(head[4..].try_into().expect("four bytes")) as usize;
    if len + head.len() > MAX_FRAME {
        return Err(bad("too long"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    decode_body(&body).map(Some)
}

/// Whether a read failed only because the other end has gone (a pipe's
/// broken-pipe error).
fn is_closed(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::BrokenPipe || (cfg!(windows) && e.raw_os_error() == Some(109)) // ERROR_BROKEN_PIPE
}

/// Who a process runs as: the user's SID on Windows, the uid on Linux.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer(pub String);

impl Peer {
    /// This process's user.
    pub fn current() -> io::Result<Peer> {
        imp::current_peer()
    }
}

fn refused(side: &str, peer: &Peer) -> io::Error {
    let m = qsa_msg::message(qsa_msg::id::FOREIGN_PEER_REFUSED, &[]);
    crate::log!("{m} ({side} ran as {})", peer.0);
    io::Error::new(io::ErrorKind::PermissionDenied, m.to_string())
}

/// One link between an instance and a later `qsaccess`, its peer checked.
/// Dropping it closes it.
pub struct Link {
    stream: imp::Stream,
}

impl Link {
    /// Sends one frame.
    pub fn send(&mut self, args: &[String]) -> io::Result<()> {
        self.stream.write_all(&encode(args))?;
        self.stream.flush()
    }

    /// The next frame; `None` once the other end has closed.
    pub fn recv(&mut self) -> io::Result<Option<Vec<String>>> {
        read_frame(&mut self.stream)
    }
}

/// How this process starts.
pub enum Startup {
    /// No other instance: this one owns the endpoint.
    Primary(Server),
    /// The arguments were handed to the running instance.
    Secondary,
}

/// Hands `args` to a running instance, or takes the endpoint.
pub fn start(endpoint: &Endpoint, args: &[String]) -> io::Result<Startup> {
    if endpoint.send(args)? {
        return Ok(Startup::Secondary);
    }
    match Server::bind(endpoint) {
        Ok(s) => Ok(Startup::Primary(s)),
        // Another instance won a race to bind; hand off to it.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if endpoint.send(args)? {
                Ok(Startup::Secondary)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

impl Endpoint {
    /// A link to the instance listening here, its user checked; `None` when
    /// none is listening.
    pub fn open(&self) -> io::Result<Option<Link>> {
        self.open_expecting(&Peer::current()?)
    }

    /// [`Endpoint::open`], the instance expected to run as `expected`.
    pub fn open_expecting(&self, expected: &Peer) -> io::Result<Option<Link>> {
        let Some(stream) = self.connect()? else {
            return Ok(None);
        };
        let peer = imp::server_peer(&stream)?;
        if peer != *expected {
            return Err(refused("the instance", &peer));
        }
        Ok(Some(Link { stream }))
    }

    /// Sends `args` to the instance listening here and closes the link.
    /// False if none is.
    pub fn send(&self, args: &[String]) -> io::Result<bool> {
        match self.open()? {
            Some(mut l) => {
                imp::allow_foreground(&l.stream);
                l.send(args)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// The listening side.
pub struct Server {
    inner: imp::Listener,
    expected: Peer,
    refused: Arc<AtomicU64>,
}

impl Server {
    /// Takes the endpoint, for links from this process's user only. Fails
    /// with `AlreadyExists` if another instance has it.
    pub fn bind(endpoint: &Endpoint) -> io::Result<Server> {
        Server::bind_expecting(endpoint, Peer::current()?)
    }

    /// [`Server::bind`], taking links only from `expected`: for the test of
    /// a peer of another user, which a test cannot be.
    pub fn bind_expecting(endpoint: &Endpoint, expected: Peer) -> io::Result<Server> {
        Ok(Server {
            inner: imp::Listener::bind(endpoint)?,
            expected,
            refused: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Links refused so far, their peer another user.
    pub fn refused(&self) -> u64 {
        self.refused.load(Ordering::SeqCst)
    }

    /// Listens on a background thread. Each link's first frame goes to
    /// `on_message` with the link, to answer on or drop. A link whose peer
    /// is not the expected user is closed unread. The returned server must
    /// be kept until exit.
    pub fn spawn(
        mut self,
        mut on_message: impl FnMut(Vec<String>, Link) + Send + 'static,
    ) -> Server {
        let expected = self.expected.clone();
        let count = Arc::clone(&self.refused);
        self.inner.spawn(move |stream| {
            let peer = match imp::client_peer(&stream) {
                Ok(p) => p,
                Err(e) => {
                    crate::log!("instance link ignored: its user is not known: {e}");
                    return;
                }
            };
            if peer != expected {
                count.fetch_add(1, Ordering::SeqCst);
                drop(refused("the program connecting", &peer));
                return;
            }
            let mut link = Link { stream };
            match link.recv() {
                Ok(Some(args)) => on_message(args, link),
                // A check whether an instance runs: nothing sent.
                Ok(None) => {}
                Err(e) => crate::log!("instance message ignored: {e}"),
            }
        });
        self
    }
}

#[cfg(unix)]
use unix as imp;
#[cfg(unix)]
pub use unix::Endpoint;
#[cfg(windows)]
use windows as imp;
#[cfg(windows)]
pub use windows::Endpoint;

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    pub(super) type Stream = UnixStream;

    /// A Unix socket path.
    #[derive(Clone, Debug)]
    pub struct Endpoint {
        path: PathBuf,
    }

    /// Socket file name.
    const SOCKET: &str = "quartz-access.sock";

    impl Endpoint {
        /// The per-user endpoint: in `$XDG_RUNTIME_DIR`, else in `config_dir`.
        pub fn for_user(config_dir: Option<&Path>) -> io::Result<Endpoint> {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute() && p.is_dir());
            let dir = runtime
                .or_else(|| config_dir.map(Path::to_path_buf))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no runtime directory"))?;
            Ok(Endpoint {
                path: dir.join(SOCKET),
            })
        }

        /// An endpoint at an explicit path (tests).
        pub fn at(path: impl Into<PathBuf>) -> Endpoint {
            Endpoint { path: path.into() }
        }

        /// The socket path.
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Whether an instance is listening here. Nothing is sent: the
        /// instance sees an empty link and ignores it.
        pub fn is_running(&self) -> bool {
            UnixStream::connect(&self.path).is_ok()
        }

        pub(super) fn connect(&self) -> io::Result<Option<UnixStream>> {
            match UnixStream::connect(&self.path) {
                Ok(s) => Ok(Some(s)),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        }
    }

    pub(super) fn allow_foreground(_s: &UnixStream) {}

    #[cfg(target_os = "linux")]
    mod cred {
        use std::ffi::c_void;
        use std::os::fd::AsRawFd;

        /// `struct ucred`.
        #[repr(C)]
        struct Ucred {
            pid: i32,
            uid: u32,
            gid: u32,
        }

        const SOL_SOCKET: i32 = 1;
        const SO_PEERCRED: i32 = 17;

        unsafe extern "C" {
            fn getsockopt(fd: i32, level: i32, name: i32, val: *mut c_void, len: *mut u32) -> i32;
            fn getuid() -> u32;
        }

        /// The uid of the process at the other end of `s`: for a client,
        /// the listener's at its bind; for the listener, the client's at
        /// its connect.
        pub fn peer_uid(s: &impl AsRawFd) -> std::io::Result<u32> {
            let mut c = Ucred {
                pid: 0,
                uid: u32::MAX,
                gid: 0,
            };
            let mut len = std::mem::size_of::<Ucred>() as u32;
            // SAFETY: `c` is a `struct ucred` and `len` its size; the fd is
            // a connected Unix socket owned by `s`.
            let r = unsafe {
                getsockopt(
                    s.as_raw_fd(),
                    SOL_SOCKET,
                    SO_PEERCRED,
                    (&raw mut c).cast(),
                    &mut len,
                )
            };
            if r != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(c.uid)
        }

        pub fn uid() -> u32 {
            // SAFETY: getuid has no preconditions and cannot fail.
            unsafe { getuid() }
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn current_peer() -> io::Result<Peer> {
        Ok(Peer(cred::uid().to_string()))
    }

    #[cfg(target_os = "linux")]
    fn peer_of(s: &UnixStream) -> io::Result<Peer> {
        cred::peer_uid(s).map(|u| Peer(u.to_string()))
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn current_peer() -> io::Result<Peer> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials are read on Linux only",
        ))
    }

    #[cfg(not(target_os = "linux"))]
    fn peer_of(_s: &UnixStream) -> io::Result<Peer> {
        current_peer()
    }

    pub(super) fn server_peer(s: &UnixStream) -> io::Result<Peer> {
        peer_of(s)
    }

    pub(super) fn client_peer(s: &UnixStream) -> io::Result<Peer> {
        peer_of(s)
    }

    /// The listening socket. Dropping it removes the socket file.
    pub(super) struct Listener {
        listener: Option<UnixListener>,
        path: PathBuf,
    }

    impl Listener {
        /// Binds the endpoint, first removing a stale socket.
        pub(super) fn bind(endpoint: &Endpoint) -> io::Result<Listener> {
            let path = endpoint.path.clone();
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if !meta.file_type().is_socket() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("{} exists and is not a socket", path.display()),
                    ));
                }
                match UnixStream::connect(&path) {
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "another instance is listening",
                        ));
                    }
                    Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                        std::fs::remove_file(&path)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            let listener = UnixListener::bind(&path).map_err(|e| {
                if e.kind() == io::ErrorKind::AddrInUse {
                    io::Error::new(io::ErrorKind::AlreadyExists, e)
                } else {
                    e
                }
            })?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            Ok(Listener {
                listener: Some(listener),
                path,
            })
        }

        /// Accepts on a background thread, handing each link to `on_link`.
        pub(super) fn spawn(&mut self, mut on_link: impl FnMut(UnixStream) + Send + 'static) {
            let listener = self.listener.take().expect("not yet spawned");
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(s) => on_link(s),
                        Err(e) => crate::log!("instance link not accepted: {e}"),
                    }
                }
            });
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
        GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    pub(super) type Stream = File;

    /// A named pipe.
    #[derive(Clone, Debug)]
    pub struct Endpoint {
        name: String,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The user SID, as a string (`S-1-5-21-...`), of the process `process`
    /// (a handle with query rights).
    fn process_sid(process: HANDLE) -> io::Result<String> {
        // SAFETY: plain Win32 calls on a process token. Buffers are sized
        // from the first GetTokenInformation call and kept aligned for
        // TOKEN_USER; every handle and allocation is released before return.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(token);
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s: *mut u16 = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
                return Err(io::Error::last_os_error());
            }
            let n = (0..).take_while(|&i| *s.add(i) != 0).count();
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
            LocalFree(s.cast());
            Ok(sid)
        }
    }

    /// The user SID of process `pid`.
    fn pid_sid(pid: u32) -> io::Result<String> {
        // SAFETY: OpenProcess with query rights only; the handle is closed
        // before return.
        unsafe {
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if p.is_null() {
                return Err(io::Error::last_os_error());
            }
            let sid = process_sid(p);
            CloseHandle(p);
            sid
        }
    }

    pub(super) fn current_peer() -> io::Result<Peer> {
        // SAFETY: the pseudo-handle of this process needs no closing.
        process_sid(unsafe { GetCurrentProcess() }).map(Peer)
    }

    pub(super) fn server_peer(f: &File) -> io::Result<Peer> {
        let mut pid = 0u32;
        // SAFETY: the handle is a connected pipe client owned by `f`.
        if unsafe { GetNamedPipeServerProcessId(f.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        pid_sid(pid).map(Peer)
    }

    pub(super) fn client_peer(f: &File) -> io::Result<Peer> {
        let mut pid = 0u32;
        // SAFETY: the handle is a connected pipe instance owned by `f`.
        if unsafe { GetNamedPipeClientProcessId(f.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        pid_sid(pid).map(Peer)
    }

    impl Endpoint {
        /// The per-user pipe. `config_dir` is not used on Windows.
        pub fn for_user(_config_dir: Option<&Path>) -> io::Result<Endpoint> {
            Ok(Endpoint::named(&format!(
                "quartz-access-{}",
                current_peer()?.0
            )))
        }

        /// A pipe with an explicit name (tests).
        pub fn named(name: &str) -> Endpoint {
            Endpoint {
                name: format!(r"\\.\pipe\{name}"),
            }
        }

        /// The full pipe name.
        pub fn name(&self) -> &str {
            &self.name
        }

        /// Whether an instance is listening here. Nothing is sent: the
        /// instance sees an empty link and ignores it.
        pub fn is_running(&self) -> bool {
            for _ in 0..10 {
                match OpenOptions::new().read(true).write(true).open(&self.name) {
                    Ok(_) => return true,
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                        let name = wide(&self.name);
                        // SAFETY: `name` is a NUL-terminated UTF-16 string.
                        unsafe { WaitNamedPipeW(name.as_ptr(), 200) };
                    }
                    Err(_) => return false,
                }
            }
            true
        }

        pub(super) fn connect(&self) -> io::Result<Option<File>> {
            for _ in 0..10 {
                match OpenOptions::new().read(true).write(true).open(&self.name) {
                    Ok(f) => return Ok(Some(f)),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                        let name = wide(&self.name);
                        // SAFETY: `name` is a NUL-terminated UTF-16 string.
                        unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "instance pipe busy",
            ))
        }
    }

    /// Lets the running instance bring its window to the front.
    pub(super) fn allow_foreground(f: &File) {
        let mut pid = 0u32;
        // SAFETY: the handle is a connected pipe client owned by `f`.
        unsafe {
            if GetNamedPipeServerProcessId(f.as_raw_handle() as HANDLE, &mut pid) != 0 {
                AllowSetForegroundWindow(pid);
            }
        }
    }

    /// A security descriptor whose DACL lets `sid` alone have the pipe;
    /// freed with `LocalFree`.
    fn only_user(sid: &str) -> io::Result<PSECURITY_DESCRIPTOR> {
        // Protected (no inherited entries), generic all for the user.
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'd
        // descriptor.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(sd)
    }

    fn create_instance(name: &[u16], first: bool) -> io::Result<HANDLE> {
        let mut open = PIPE_ACCESS_DUPLEX;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
        let sd = only_user(&current_peer()?.0)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        // SAFETY: `name` is NUL-terminated and `sa` points at a valid
        // descriptor, freed once the pipe is made.
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open,
                mode,
                PIPE_UNLIMITED_INSTANCES,
                MAX_FRAME as u32,
                MAX_FRAME as u32,
                0,
                &sa,
            )
        };
        let e = io::Error::last_os_error();
        // SAFETY: allocated by ConvertStringSecurityDescriptor...W.
        unsafe { LocalFree(sd) };
        if h == INVALID_HANDLE_VALUE {
            if first && e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, e));
            }
            return Err(e);
        }
        Ok(h)
    }

    /// The first instance of the pipe, until spawned.
    pub(super) struct Listener {
        name: Vec<u16>,
        first: Option<usize>,
    }

    impl Listener {
        /// Creates the first instance of the pipe. Fails with
        /// `AlreadyExists` if another process owns the name.
        pub(super) fn bind(endpoint: &Endpoint) -> io::Result<Listener> {
            let name = wide(&endpoint.name);
            let h = create_instance(&name, true)?;
            Ok(Listener {
                name,
                first: Some(h as usize),
            })
        }

        /// Accepts on a background thread, handing each link to `on_link`.
        pub(super) fn spawn(&mut self, mut on_link: impl FnMut(File) + Send + 'static) {
            let name = self.name.clone();
            let first = self.first.take().expect("not yet spawned");
            std::thread::spawn(move || {
                let mut current = first as HANDLE;
                loop {
                    // SAFETY: `current` is a pipe instance this thread owns.
                    // A client may connect, write its frame and close before
                    // this call: that is ERROR_NO_DATA, and the frame is
                    // still waiting in the pipe to be read.
                    let ok = unsafe { ConnectNamedPipe(current, null_mut()) } != 0
                        || matches!(
                            io::Error::last_os_error().raw_os_error(),
                            Some(e) if e == ERROR_PIPE_CONNECTED as i32 || e == ERROR_NO_DATA as i32
                        );
                    // Make the next instance before handing this one on, so
                    // the name is never free for another process to take.
                    let next = match create_instance(&name, false) {
                        Ok(h) => h,
                        Err(e) => {
                            crate::log!("instance pipe stopped: {e}");
                            // SAFETY: closing our own handle.
                            unsafe { CloseHandle(current) };
                            return;
                        }
                    };
                    // SAFETY: we own `current`; the File closes it on drop.
                    let f = unsafe { File::from_raw_handle(current as RawHandle) };
                    if ok {
                        on_link(f);
                    }
                    current = next;
                }
            });
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            if let Some(h) = self.first.take() {
                // SAFETY: an instance created by bind and never spawned.
                unsafe { CloseHandle(h as HANDLE) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn frame_round_trip() {
        for a in [args(&[]), args(&["--x", "", "é ü", "a b"])] {
            assert_eq!(decode(&encode(&a)).unwrap(), a);
            let mut r = io::Cursor::new([encode(&a), encode(&a)].concat());
            assert_eq!(read_frame(&mut r).unwrap(), Some(a.clone()));
            assert_eq!(read_frame(&mut r).unwrap(), Some(a.clone()));
            assert_eq!(read_frame(&mut r).unwrap(), None);
        }
    }

    #[test]
    fn frame_rejects_garbage() {
        assert!(decode(b"").is_err());
        assert!(decode(b"XXXX\0\0\0\0").is_err());
        let mut f = encode(&args(&["abc"]));
        f.pop();
        assert!(decode(&f).is_err());
        let mut f = encode(&args(&["abc"]));
        f.push(0);
        assert!(decode(&f).is_err());
        assert!(decode(&[b'Q', b'S', b'A', b'2', 4, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]).is_err());
        assert!(decode(&vec![0; MAX_FRAME + 1]).is_err());
        // The old one-way frame is not taken.
        assert!(decode(b"QSA1\0\0\0\0").is_err());
        let mut r = io::Cursor::new(b"QSA2\xFF\xFF\xFF\x7F".to_vec());
        assert!(read_frame(&mut r).is_err());
    }

    fn endpoint(tag: &str) -> Endpoint {
        #[cfg(windows)]
        {
            Endpoint::named(&format!("quartz-access-test-{tag}-{}", std::process::id()))
        }
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("qsa-ipc-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Endpoint::at(dir.join("s.sock"))
        }
    }

    fn receive(server: Server) -> (Server, mpsc::Receiver<Vec<String>>) {
        let (tx, rx) = mpsc::channel();
        let s = server.spawn(move |a, _| tx.send(a).unwrap());
        (s, rx)
    }

    #[test]
    fn second_start_hands_off_to_first() {
        let ep = endpoint("handoff");
        let Startup::Primary(server) = start(&ep, &args(&["one"])).unwrap() else {
            panic!("first start should be primary");
        };
        let (_server, rx) = receive(server);
        for n in 0..3 {
            let a = args(&["open", &format!("file{n}.qsd")]);
            assert!(matches!(start(&ep, &a).unwrap(), Startup::Secondary));
            assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), a);
        }
    }

    #[test]
    fn is_running_sends_nothing() {
        let ep = endpoint("ping");
        assert!(!ep.is_running());
        let Startup::Primary(server) = start(&ep, &[]).unwrap() else {
            panic!("first start should be primary");
        };
        let (_server, rx) = receive(server);
        assert!(ep.is_running());
        assert!(ep.send(&args(&["--quit"])).unwrap());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            args(&["--quit"])
        );
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the check is not a request"
        );
    }

    #[test]
    fn quick_clients_are_not_lost() {
        // Clients that write and close at once, many in a row, as a burst
        // of double-clicks does: every frame must arrive.
        let ep = endpoint("burst");
        let Startup::Primary(server) = start(&ep, &[]).unwrap() else {
            panic!("first start should be primary");
        };
        let (_server, rx) = receive(server);
        let n = 40;
        for i in 0..n {
            assert!(ep.send(&args(&["open", &format!("{i}.qsd")])).unwrap());
        }
        let mut got: Vec<String> = (0..n)
            .map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap()[1].clone())
            .collect();
        got.sort();
        let mut want: Vec<String> = (0..n).map(|i| format!("{i}.qsd")).collect();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn a_link_answers_both_ways() {
        let ep = endpoint("duplex");
        let server = Server::bind(&ep).unwrap().spawn(|a, mut link| {
            for i in 0..3 {
                link.send(&args(&["line", &format!("{} {i}", a[0])]))
                    .unwrap();
            }
        });
        let mut l = ep.open().unwrap().expect("listening");
        l.send(&args(&["hello"])).unwrap();
        for i in 0..3 {
            assert_eq!(
                l.recv().unwrap().unwrap(),
                args(&["line", &format!("hello {i}")])
            );
        }
        assert_eq!(l.recv().unwrap(), None, "closed when the handler is done");
        assert_eq!(server.refused(), 0);
    }

    #[test]
    fn a_peer_of_another_user_is_refused_both_ways() {
        let me = Peer::current().unwrap();
        assert!(!me.0.is_empty());
        #[cfg(windows)]
        let other = Peer("S-1-5-21-1-2-3-1001".into());
        #[cfg(unix)]
        let other = Peer(format!("{}", me.0.parse::<u64>().unwrap() + 1));
        // An instance that takes only `other`: this process's link is
        // refused, closed unread, and counted.
        let ep = endpoint("foreign");
        let (server, rx) = receive(Server::bind_expecting(&ep, other.clone()).unwrap());
        let mut l = ep.open().unwrap().expect("listening");
        let _ = l.send(&args(&["open", "x.qsd"]));
        assert!(matches!(l.recv(), Ok(None) | Err(_)));
        assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
        assert_eq!(server.refused(), 1);
        // A client that expects `other` refuses this process's instance.
        let ep = endpoint("foreign-server");
        let (_server, rx) = receive(Server::bind(&ep).unwrap());
        let e = ep.open_expecting(&other).err().expect("refused");
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains("QAD8040"), "{e}");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_is_replaced_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let ep = endpoint("stale");
        // A listener dropped without unlinking leaves a stale socket file.
        drop(std::os::unix::net::UnixListener::bind(ep.path()).unwrap());
        assert!(ep.path().exists());
        assert!(!ep.send(&args(&["x"])).unwrap());
        let Startup::Primary(server) = start(&ep, &[]).unwrap() else {
            panic!("stale socket should be replaced");
        };
        let mode = std::fs::metadata(ep.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        drop(server);
        assert!(!ep.path().exists());
    }
}
