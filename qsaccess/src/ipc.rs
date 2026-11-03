// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Single instance. The first `qsaccess` to start listens on a per-user
//! endpoint; a later one sends it its arguments and exits.
//!
//! - Windows: the named pipe `\\.\pipe\quartz-access-<user SID>`, which
//!   refuses remote clients.
//! - Linux: the Unix socket `$XDG_RUNTIME_DIR/quartz-access.sock`, else
//!   `quartz-access.sock` in the settings directory; mode 0600. A stale
//!   socket left by a crashed instance is removed.
//!
//! A message is one frame: `QSA1`, the argument count, then each argument as
//! a length and UTF-8 bytes. Lengths are little-endian `u32`.

use std::io::{self, Read, Write};
use std::path::Path;

/// Frame magic.
const MAGIC: &[u8; 4] = b"QSA1";
/// Largest frame accepted.
pub const MAX_FRAME: usize = 64 * 1024;

/// Encodes arguments as one frame.
pub fn encode(args: &[String]) -> Vec<u8> {
    let mut out = Vec::from(&MAGIC[..]);
    out.extend_from_slice(&(args.len() as u32).to_le_bytes());
    for a in args {
        out.extend_from_slice(&(a.len() as u32).to_le_bytes());
        out.extend_from_slice(a.as_bytes());
    }
    out
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad frame: {what}"))
}

/// Decodes one frame.
pub fn decode(frame: &[u8]) -> io::Result<Vec<String>> {
    if frame.len() > MAX_FRAME {
        return Err(bad("too long"));
    }
    let (magic, mut rest) = frame.split_at_checked(4).ok_or_else(|| bad("short"))?;
    if magic != MAGIC {
        return Err(bad("magic"));
    }
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

/// Reads one frame from a connection the sender closes when done.
fn read_frame(mut r: impl Read) -> io::Result<Vec<String>> {
    let mut buf = Vec::new();
    r.by_ref()
        .take(MAX_FRAME as u64 + 1)
        .read_to_end(&mut buf)?;
    decode(&buf)
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

/// The argument that asks the running instance for its session on a
/// system: `--session-token <system> <reply endpoint>`. It answers on the
/// reply endpoint with one frame: the token as hex, or nothing when it has
/// no session on the system.
pub const SESSION_TOKEN: &str = "--session-token";

/// A reply endpoint's name: the instance endpoint's own name, then
/// `-reply-` and the asking process's ID, so the instance answers only an
/// endpoint of the same user.
pub fn reply_name(main: &Endpoint, pid: u64) -> String {
    format!("{}-reply-{pid}", main.id())
}

/// The reply endpoint `name`, if it is one of `main`'s (see
/// [`reply_name`]).
pub fn reply_endpoint(main: &Endpoint, name: &str) -> Option<Endpoint> {
    let pid = name.strip_prefix(&format!("{}-reply-", main.id()))?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Endpoint::from_id(name))
}

#[cfg(unix)]
pub use unix::{Endpoint, Server};
#[cfg(windows)]
pub use windows::{Endpoint, Server};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

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

        /// The endpoint as a string another process can be given.
        pub fn id(&self) -> String {
            self.path.display().to_string()
        }

        /// The endpoint [`Endpoint::id`] gave.
        pub fn from_id(id: &str) -> Endpoint {
            Endpoint { path: id.into() }
        }

        /// Whether an instance is listening here. Nothing is sent: the
        /// instance sees an empty connection and ignores it.
        pub fn is_running(&self) -> bool {
            UnixStream::connect(&self.path).is_ok()
        }

        /// Sends `args` to the instance listening here. False if none is.
        pub fn send(&self, args: &[String]) -> io::Result<bool> {
            match UnixStream::connect(&self.path) {
                Ok(mut s) => {
                    s.write_all(&encode(args))?;
                    s.shutdown(std::net::Shutdown::Write)?;
                    Ok(true)
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    Ok(false)
                }
                Err(e) => Err(e),
            }
        }
    }

    /// The listening side. Dropping it removes the socket file.
    pub struct Server {
        listener: Option<UnixListener>,
        path: PathBuf,
    }

    impl Server {
        /// Binds the endpoint, first removing a stale socket.
        pub fn bind(endpoint: &Endpoint) -> io::Result<Server> {
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
            Ok(Server {
                listener: Some(listener),
                path,
            })
        }

        /// Listens on a background thread, calling `on_args` for every
        /// message. The returned server must be kept until exit.
        pub fn spawn(mut self, mut on_args: impl FnMut(Vec<String>) + Send + 'static) -> Server {
            let listener = self.listener.take().expect("not yet spawned");
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream.and_then(read_frame) {
                        Ok(args) => on_args(args),
                        Err(e) => crate::log!("instance message ignored: {e}"),
                    }
                }
            });
            self
        }
    }

    impl Drop for Server {
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
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    /// A named pipe.
    #[derive(Clone, Debug)]
    pub struct Endpoint {
        name: String,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The current user's SID as a string, `S-1-5-21-...`.
    fn user_sid() -> io::Result<String> {
        // SAFETY: plain Win32 calls on the current process token. Buffers are
        // sized from the first GetTokenInformation call and kept aligned for
        // TOKEN_USER; every handle and allocation is released before return.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
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

    impl Endpoint {
        /// The per-user pipe. `config_dir` is not used on Windows.
        pub fn for_user(_config_dir: Option<&Path>) -> io::Result<Endpoint> {
            Ok(Endpoint::named(&format!("quartz-access-{}", user_sid()?)))
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

        /// The endpoint as a string another process can be given.
        pub fn id(&self) -> String {
            self.name.clone()
        }

        /// The endpoint [`Endpoint::id`] gave.
        pub fn from_id(id: &str) -> Endpoint {
            Endpoint {
                name: id.to_owned(),
            }
        }

        /// Whether an instance is listening here. Nothing is sent: the
        /// instance sees an empty connection and ignores it.
        pub fn is_running(&self) -> bool {
            for _ in 0..10 {
                match OpenOptions::new().write(true).open(&self.name) {
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

        /// Sends `args` to the instance listening here. False if none is.
        pub fn send(&self, args: &[String]) -> io::Result<bool> {
            for _ in 0..10 {
                match OpenOptions::new().write(true).open(&self.name) {
                    Ok(mut f) => {
                        allow_foreground(&f);
                        f.write_all(&encode(args))?;
                        return Ok(true);
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
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
    fn allow_foreground(f: &File) {
        let mut pid = 0u32;
        // SAFETY: the handle is a connected pipe client owned by `f`.
        unsafe {
            if GetNamedPipeServerProcessId(f.as_raw_handle() as HANDLE, &mut pid) != 0 {
                AllowSetForegroundWindow(pid);
            }
        }
    }

    fn create_instance(name: &[u16], first: bool) -> io::Result<HANDLE> {
        let mut open = PIPE_ACCESS_INBOUND;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
        // SAFETY: `name` is NUL-terminated; default security (the creator,
        // SYSTEM and administrators may write).
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open,
                mode,
                PIPE_UNLIMITED_INSTANCES,
                0,
                MAX_FRAME as u32,
                0,
                null(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            let e = io::Error::last_os_error();
            if first && e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, e));
            }
            return Err(e);
        }
        Ok(h)
    }

    /// The listening side.
    pub struct Server {
        name: Vec<u16>,
        first: Option<usize>,
    }

    impl Server {
        /// Creates the first instance of the pipe. Fails with `AlreadyExists`
        /// if another process owns the name.
        pub fn bind(endpoint: &Endpoint) -> io::Result<Server> {
            let name = wide(&endpoint.name);
            let h = create_instance(&name, true)?;
            Ok(Server {
                name,
                first: Some(h as usize),
            })
        }

        /// Listens on a background thread, calling `on_args` for every
        /// message. The returned server must be kept until exit.
        pub fn spawn(mut self, mut on_args: impl FnMut(Vec<String>) + Send + 'static) -> Server {
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
                    // Make the next instance before closing this one, so the
                    // name is never free for another process to take.
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
                        match read_frame(f) {
                            Ok(args) => on_args(args),
                            Err(e) => crate::log!("instance message ignored: {e}"),
                        }
                    }
                    current = next;
                }
            });
            self
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            if let Some(h) = self.first.take() {
                // SAFETY: an instance created by bind and never spawned.
                unsafe { CloseHandle(h as HANDLE) };
            }
        }
    }
}

static THIS: std::sync::OnceLock<Endpoint> = std::sync::OnceLock::new();

/// Keeps this instance's endpoint, for checking reply endpoints against.
pub fn remember(e: &Endpoint) {
    let _ = THIS.set(e.clone());
}

/// This instance's endpoint, once kept.
pub fn remembered() -> Option<&'static Endpoint> {
    THIS.get()
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
        assert!(decode(&[b'Q', b'S', b'A', b'1', 0xFF, 0xFF, 0xFF, 0xFF]).is_err());
        assert!(decode(&vec![0; MAX_FRAME + 1]).is_err());
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

    #[test]
    fn second_start_hands_off_to_first() {
        let ep = endpoint("handoff");
        let Startup::Primary(server) = start(&ep, &args(&["one"])).unwrap() else {
            panic!("first start should be primary");
        };
        let (tx, rx) = mpsc::channel();
        let _server = server.spawn(move |a| tx.send(a).unwrap());
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
        let (tx, rx) = mpsc::channel();
        let _server = server.spawn(move |a| tx.send(a).unwrap());
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
        let (tx, rx) = mpsc::channel();
        let _server = server.spawn(move |a| tx.send(a).unwrap());
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
