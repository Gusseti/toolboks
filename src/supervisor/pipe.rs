//! The connection to one add-on launch: two named pipes that only the current user can open,
//! one per direction, and the random token the add-on must present.
//!
//! Two one-way pipes instead of one duplex pipe: a blocked read on a synchronous pipe handle
//! holds up a write on the same handle, and the host reads and writes from different threads.

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{
    LocalFree, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PeekNamedPipe,
    SetNamedPipeHandleState, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::win::Handle;

/// The token is this many random bytes, sent as twice as many hex characters.
const TOKEN_BYTES: usize = 32;
pub const TOKEN_CHARS: usize = TOKEN_BYTES * 2;
const BUFFER_BYTES: u32 = 64 * 1024;
const POLL: Duration = Duration::from_millis(10);

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// The add-on did not connect and present its token in time.
    TimedOut,
    /// The add-on process ended before it connected.
    Exited,
    /// The launch was cancelled by the host.
    Cancelled,
    /// A process other than the one the host started opened a pipe.
    WrongClient,
    /// The add-on presented the wrong token.
    WrongToken,
    Io(String),
}

fn io_error(e: impl std::fmt::Display) -> ConnectError {
    ConnectError::Io(e.to_string())
}

/// Random bytes from the system generator, as lowercase hex.
pub fn random_hex(bytes: usize) -> io::Result<String> {
    let mut buffer = vec![0u8; bytes];
    // SAFETY: the buffer is a live slice; the system-preferred generator needs no handle.
    let status = unsafe { BCryptGenRandom(None, &mut buffer, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_err() {
        return Err(io::Error::other("the system random generator failed"));
    }
    Ok(toolboks_catalog::hex_encode(&buffer))
}

/// Frees a block the system allocated with `LocalAlloc`.
struct Local(*mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a successful Win32 call that documents LocalFree.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}

/// The current user's SID as text, for example `S-1-5-21-…-1001`.
pub fn current_user_sid() -> io::Result<String> {
    #[repr(C)]
    struct Buffer {
        user: TOKEN_USER,
        sid: [u8; SECURITY_MAX_SID_SIZE as usize],
    }
    let mut token = HANDLE::default();
    // SAFETY: `token` is a valid out-parameter; the handle is wrapped for closing right away.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }?;
    let token = Handle(token);
    let mut buffer = Buffer {
        user: TOKEN_USER::default(),
        sid: [0; SECURITY_MAX_SID_SIZE as usize],
    };
    let mut returned = 0;
    // SAFETY: the aligned buffer has room for TOKEN_USER and the longest SID.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some((&mut buffer as *mut Buffer).cast()),
            size_of::<Buffer>() as u32,
            &mut returned,
        )
    }?;
    let mut text = PWSTR::null();
    // SAFETY: the SID points into `buffer`, which GetTokenInformation filled.
    unsafe { ConvertSidToStringSidW(buffer.user.User.Sid, &mut text) }?;
    let _free = Local(text.0.cast());
    // SAFETY: on success `text` is a NUL-terminated string owned by `_free`.
    unsafe { text.to_string() }.map_err(io::Error::other)
}

/// A security descriptor that lets the current user in and nobody else: one allow entry, and
/// `P` so nothing is inherited. Everyone without an entry is denied, other users included.
struct OnlyCurrentUser {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl OnlyCurrentUser {
    fn new() -> io::Result<Self> {
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", current_user_sid()?));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is freed in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }?;
        Ok(Self { descriptor })
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for OnlyCurrentUser {
    fn drop(&mut self) {
        let _ = Local(self.descriptor.0);
    }
}

fn create(path: &str) -> io::Result<Handle> {
    let security = OnlyCurrentUser::new()?;
    let attributes = security.attributes();
    // One instance, created here first: nobody else can already own this name. Remote clients
    // are refused. `PIPE_NOWAIT` only until the add-on has connected (see `connect`). Each pipe
    // carries one direction only, but the host's handle is opened both ways: switching the
    // pipe to blocking mode afterwards needs write access to it.
    // SAFETY: the name is NUL-terminated and the attributes outlive the call.
    let handle = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(path),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            BUFFER_BYTES,
            BUFFER_BYTES,
            0,
            Some(&attributes),
        )
    };
    if handle.is_invalid() {
        return Err(io::Error::last_os_error());
    }
    Ok(Handle(handle))
}

/// The host's ends of the two pipes of one launch, before the add-on has connected.
pub struct PipePair {
    /// The add-on reads host messages here (`--pipe-in`).
    pub to_addon_path: String,
    /// The add-on writes its messages here (`--pipe-out`).
    pub from_addon_path: String,
    /// The add-on must write this to `from_addon` before anything else.
    pub token: String,
    to_addon: Handle,
    from_addon: Handle,
}

impl PipePair {
    pub fn create() -> io::Result<Self> {
        let name = format!(r"\\.\pipe\toolboks-addon-{}", random_hex(16)?);
        let to_addon_path = format!("{name}-in");
        let from_addon_path = format!("{name}-out");
        Ok(Self {
            to_addon: create(&to_addon_path)?,
            from_addon: create(&from_addon_path)?,
            to_addon_path,
            from_addon_path,
            token: random_hex(TOKEN_BYTES)?,
        })
    }

    #[cfg(test)]
    pub fn handles(&self) -> [HANDLE; 2] {
        [self.to_addon.0, self.from_addon.0]
    }

    /// Waits until the add-on has opened both pipes and presented the token, then returns
    /// blocking files for the two directions (read from the add-on, write to the add-on).
    ///
    /// `client` is the process id the host started, when it knows it: any other process on a
    /// pipe is refused. `keep_waiting` is asked between polls; it returns an error to give up
    /// (the process ended, the launch was cancelled).
    pub fn connect(
        self,
        client: Option<u32>,
        deadline: Instant,
        keep_waiting: &mut dyn FnMut() -> Result<(), ConnectError>,
    ) -> Result<(File, File), ConnectError> {
        for pipe in [&self.to_addon, &self.from_addon] {
            while !connected(pipe.0)? {
                wait(deadline, keep_waiting)?;
            }
            if let Some(expected) = client {
                let mut actual = 0;
                // SAFETY: the pipe is connected and `actual` is a valid out-parameter.
                unsafe { GetNamedPipeClientProcessId(pipe.0, &mut actual) }.map_err(io_error)?;
                if actual != expected {
                    return Err(ConnectError::WrongClient);
                }
            }
            // From here on reads and writes block, as `File` expects.
            let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
            // SAFETY: `mode` outlives the call.
            unsafe { SetNamedPipeHandleState(pipe.0, Some(&mode), None, None) }
                .map_err(io_error)?;
        }

        while available(self.from_addon.0)? < TOKEN_CHARS {
            wait(deadline, keep_waiting)?;
        }
        let mut presented = [0u8; TOKEN_CHARS];
        let mut read = 0;
        // SAFETY: the buffer is a live slice and at least that many bytes are waiting.
        unsafe { ReadFile(self.from_addon.0, Some(&mut presented), Some(&mut read), None) }
            .map_err(io_error)?;
        if read as usize != TOKEN_CHARS || !same(&presented, self.token.as_bytes()) {
            return Err(ConnectError::WrongToken);
        }

        let Self {
            to_addon,
            from_addon,
            ..
        } = self;
        Ok((into_file(from_addon), into_file(to_addon)))
    }
}

fn into_file(handle: Handle) -> File {
    let raw = handle.0 .0;
    std::mem::forget(handle);
    // SAFETY: the handle is an open pipe that nothing else owns any more.
    unsafe { File::from_raw_handle(raw) }
}

/// Compares without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

fn wait(
    deadline: Instant,
    keep_waiting: &mut dyn FnMut() -> Result<(), ConnectError>,
) -> Result<(), ConnectError> {
    keep_waiting()?;
    if Instant::now() >= deadline {
        return Err(ConnectError::TimedOut);
    }
    std::thread::sleep(POLL);
    Ok(())
}

/// Whether a client has opened the pipe. Never blocks while the pipe is in `PIPE_NOWAIT`.
fn connected(pipe: HANDLE) -> Result<bool, ConnectError> {
    // SAFETY: plain call on an open pipe handle without overlapped I/O.
    match unsafe { ConnectNamedPipe(pipe, None) } {
        Ok(()) => Ok(true),
        Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => Ok(true),
        Err(e) if e.code() == ERROR_PIPE_LISTENING.to_hresult() => Ok(false),
        Err(e) => Err(io_error(e)),
    }
}

/// How many bytes wait to be read. An error means the other end is gone.
pub fn available(pipe: HANDLE) -> Result<usize, ConnectError> {
    let mut total = 0;
    // SAFETY: only the byte count is asked for; `total` is a valid out-parameter.
    unsafe { PeekNamedPipe(pipe, None, 0, None, Some(&mut total), None) }
        .map_err(|_| ConnectError::Exited)?;
    Ok(total as usize)
}

/// The same for a pipe that is already a `File`.
pub fn available_in(file: &File) -> Result<usize, ConnectError> {
    use std::os::windows::io::AsRawHandle;
    available(HANDLE(file.as_raw_handle()))
}
