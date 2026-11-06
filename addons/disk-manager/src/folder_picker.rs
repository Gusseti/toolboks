//! Native Windows folder selection on its own COM apartment; the UI thread stays responsive.
use std::sync::mpsc;

pub fn start(
    initial: String,
    ctx: egui::Context,
) -> mpsc::Receiver<Result<Option<String>, String>> {
    let (tx, rx) = mpsc::channel();
    #[cfg(windows)]
    let owner =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
    #[cfg(not(windows))]
    let owner = 0isize;
    std::thread::spawn(move || {
        let _ = tx.send(choose(initial, owner));
        ctx.request_repaint();
    });
    rx
}

#[cfg(windows)]
fn choose(initial: String, owner: isize) -> Result<Option<String>, String> {
    use windows::{
        core::{w, HRESULT, HSTRING, PCWSTR},
        Win32::{
            Foundation::HWND,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
                FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
                SIGDN_FILESYSPATH,
            },
        },
    };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    // Objects stay on this worker. Copy the result before freeing its COM allocation.
    let result = (|| -> windows::core::Result<Option<String>> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let _apartment = Apartment;
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()?
                    | FOS_PICKFOLDERS
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_NOCHANGEDIR,
            )?;
            dialog.SetTitle(w!("Choose a folder to analyze"))?;
            if !initial.trim().is_empty() {
                let path = HSTRING::from(initial.trim());
                let item: windows::core::Result<IShellItem> =
                    SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None);
                if let Ok(item) = item {
                    let _ = dialog.SetDefaultFolder(&item);
                }
            }
            match dialog.Show(if owner == 0 {
                None
            } else {
                Some(HWND(owner as *mut _))
            }) {
                Ok(()) => {}
                Err(e) if e.code() == HRESULT::from_win32(1223) => return Ok(None),
                Err(e) => return Err(e),
            }
            let raw = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = raw.to_string();
            CoTaskMemFree(Some(raw.0.cast()));
            Ok(Some(path?))
        }
    })();
    result.map_err(|e| format!("Could not open the folder picker: {e}"))
}
#[cfg(not(windows))]
fn choose(_initial: String, _owner: isize) -> Result<Option<String>, String> {
    Err("Enter a folder path to scan on this platform.".into())
}
