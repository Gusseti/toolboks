//! One administrator action per helper process. No egui context or host input is used.
//! Preparation must be read-only. Keep the validated action inside the returned value;
//! `apply` receives no further host data and runs only after this process's native dialog.
use crate::{AdminOutcome, AdminRequest};
use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use toolboks_addon_proto::{
    self as proto, AddonMessage, Hello, HostMessage, ProtocolError, Transport,
};

#[cfg(windows)]
mod dialog;

pub const HELPER_ARG: &str = "--toolboks-admin-helper";

/// A validated, immutable preview and the one change it describes. Preparing this value
/// must not change the system. Revalidate mutable system state in `apply` and refuse if it
/// no longer matches the preview. Do not take arbitrary commands or preview text from IPC.
pub trait AdminChange {
    fn capability(&self) -> &str;
    fn description(&self) -> String;
    fn apply(self: Box<Self>) -> Result<String, String>;
}

pub fn is_helper() -> bool {
    std::env::args().any(|arg| arg == HELPER_ARG)
}

/// Call before constructing the UI, acquiring the single-instance guard or starting workers.
pub fn run_helper(
    addon_id: &str,
    prepare: impl FnOnce(&AdminRequest) -> Result<Box<dyn AdminChange>, String>,
) -> Result<(), ProtocolError> {
    let args: Vec<_> = std::env::args().collect();
    let value = |key| {
        args.iter()
            .position(|arg| arg == key)
            .and_then(|i| args.get(i + 1))
            .ok_or(ProtocolError::Unexpected(
                "missing administrator helper connection",
            ))
    };
    let reader = std::fs::File::open(value("--pipe-in")?)?;
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(value("--pipe-out")?)?;
    writer.write_all(value("--token")?.as_bytes())?;
    writer.flush()?;
    run_helper_on(Transport::new(reader, writer), addon_id, prepare)
}

/// Dedicated connection: handshake, one request, one result, exit. Any additional message
/// (including a forwarded click/key, a forged result, or Quit) cancels rather than approves.
pub fn run_helper_on<R: Read + Send + 'static, W: Write>(
    transport: Transport<R, W>,
    addon_id: &str,
    prepare: impl FnOnce(&AdminRequest) -> Result<Box<dyn AdminChange>, String>,
) -> Result<(), ProtocolError> {
    run_with(
        transport,
        addon_id,
        prepare,
        elevated(),
        |description, cancelled| {
            #[cfg(windows)]
            {
                dialog::confirm(description, cancelled)
            }
            #[cfg(not(windows))]
            {
                let _ = (description, cancelled);
                Err("Administrator actions require Windows.".into())
            }
        },
    )
}

fn run_with<R: Read + Send + 'static, W: Write>(
    transport: Transport<R, W>,
    addon_id: &str,
    prepare: impl FnOnce(&AdminRequest) -> Result<Box<dyn AdminChange>, String>,
    is_elevated: bool,
    confirm: impl FnOnce(&str, &AtomicBool) -> Result<bool, String>,
) -> Result<(), ProtocolError> {
    let (mut reader, mut writer) = transport.split();
    proto::addon_handshake(
        &mut reader,
        &mut writer,
        &Hello::new(addon_id, env!("CARGO_PKG_VERSION")),
    )?;
    let HostMessage::AdminOperation {
        request,
        capabilities,
    } = reader.read()?
    else {
        return Err(ProtocolError::Unexpected(
            "expected one administrator operation",
        ));
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let watch = cancelled.clone();
    std::thread::Builder::new()
        .name("administrator-disconnect".into())
        .spawn(move || {
            let _ = reader.read::<HostMessage>();
            watch.store(true, Ordering::SeqCst);
        })?;
    let outcome = perform(
        &request,
        &capabilities,
        is_elevated,
        &cancelled,
        prepare,
        confirm,
    );
    writer.write(&AddonMessage::AdminResult {
        id: request.id,
        outcome,
    })
}

fn perform(
    request: &AdminRequest,
    capabilities: &[String],
    is_elevated: bool,
    cancelled: &AtomicBool,
    prepare: impl FnOnce(&AdminRequest) -> Result<Box<dyn AdminChange>, String>,
    confirm: impl FnOnce(&str, &AtomicBool) -> Result<bool, String>,
) -> AdminOutcome {
    let result = (|| {
        request.validate()?;
        if !is_elevated {
            return Err("The administrator helper has no administrator rights.".into());
        }
        let change = prepare(request)?;
        if !["administrator", "drive_optimize", "system_tuning"].contains(&change.capability())
            || !capabilities.iter().any(|cap| cap == change.capability())
        {
            return Err("This add-on was not granted access to this administrator action.".into());
        }
        let description = change.description();
        if description.trim().is_empty() || description.len() > 8000 || description.contains('\0') {
            return Err("The administrator action has no valid preview.".into());
        }
        if cancelled.load(Ordering::SeqCst)
            || !confirm(&description, cancelled)?
            || cancelled.load(Ordering::SeqCst)
        {
            return Ok(AdminOutcome::Cancelled);
        }
        change.apply().map(|json| AdminOutcome::Completed { json })
    })();
    result.unwrap_or_else(|reason| AdminOutcome::Failed { reason })
}

pub(crate) fn elevated() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = windows::Win32::Foundation::HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Shows the exact native dialog without elevation or writes. Only compiled for preview builds.
#[cfg(all(windows, feature = "admin-preview"))]
pub fn preview_dialog(description: &str) -> Result<bool, String> {
    dialog::confirm(description, &AtomicBool::new(false))
}

#[cfg(test)]
mod tests;
