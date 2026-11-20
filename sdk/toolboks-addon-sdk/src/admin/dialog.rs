//! A Windows dialog created on the helper's thread. No host HWND, input batch or approval
//! message ever enters this module. Native controls keep OS keyboard/accessibility support.
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

const DIALOG_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX((2 * size_of::<isize>()) as i32);
const CONFIRM: u16 = 1001;
const DESCRIPTION: u16 = 1002;
struct State<'a> {
    description: &'a str,
    cancelled: &'a AtomicBool,
    owner: u32,
}

// Standard DLGTEMPLATE and DLGITEMTEMPLATE records must be DWORD aligned. Store them in
// u32 words, then encode each field explicitly (the Windows structures have packed fields).
fn template() -> Vec<u32> {
    fn word(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend(value.to_le_bytes());
    }
    fn dword(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend(value.to_le_bytes());
    }
    fn string(bytes: &mut Vec<u8>, value: &str) {
        for c in value.encode_utf16().chain(Some(0)) {
            word(bytes, c);
        }
    }
    fn control(bytes: &mut Vec<u8>, style: u32, rect: [u16; 4], id: u16, class: u16, label: &str) {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        dword(bytes, WS_CHILD.0 | WS_VISIBLE.0 | style);
        dword(bytes, 0);
        for n in rect {
            word(bytes, n);
        }
        word(bytes, id);
        word(bytes, 0xffff);
        word(bytes, class);
        string(bytes, label);
        word(bytes, 0);
    }
    let mut b = Vec::new();
    dword(
        &mut b,
        WS_POPUP.0
            | WS_CAPTION.0
            | WS_SYSMENU.0
            | DS_MODALFRAME as u32
            | DS_SETFONT as u32
            | DS_CENTER as u32,
    );
    dword(&mut b, 0);
    word(&mut b, 3);
    for n in [0, 0, 400, 260] {
        word(&mut b, n);
    }
    word(&mut b, 0);
    word(&mut b, 0);
    string(&mut b, "Confirm administrator change");
    word(&mut b, 9);
    string(&mut b, "Segoe UI");
    control(
        &mut b,
        WS_TABSTOP.0
            | WS_VSCROLL.0
            | ES_MULTILINE as u32
            | ES_READONLY as u32
            | ES_AUTOVSCROLL as u32,
        [12, 12, 376, 208],
        DESCRIPTION,
        0x81,
        "",
    );
    control(
        &mut b,
        WS_TABSTOP.0 | BS_PUSHBUTTON as u32,
        [244, 230, 68, 20],
        CONFIRM,
        0x80,
        "Confirm",
    );
    control(
        &mut b,
        WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
        [320, 230, 68, 20],
        IDCANCEL.0 as u16,
        0x80,
        "Cancel",
    );
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect()
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    if message == WM_INITDIALOG {
        // SAFETY: DialogBox passes our State, which remains alive throughout its modal loop.
        let state = unsafe { &mut *(lparam.0 as *mut State<'_>) };
        unsafe {
            SetWindowLongPtrW(hwnd, DIALOG_USER, lparam.0);
            GetWindowThreadProcessId(hwnd, Some(&mut state.owner));
            let _ = SetDlgItemTextW(
                hwnd,
                DESCRIPTION as i32,
                &HSTRING::from(
                    state
                        .description
                        .replace("\r\n", "\n")
                        .replace('\n', "\r\n"),
                ),
            );
            SetTimer(Some(hwnd), 1, 100, None);
            if let Ok(button) = GetDlgItem(Some(hwnd), IDCANCEL.0) {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(button));
            }
        }
        return 0; // Focus is deliberately on Cancel; Enter cannot accept a newly opened dialog.
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, DIALOG_USER) } as *const State<'_>;
    if pointer.is_null() {
        return 0;
    }
    let state = unsafe { &*pointer };
    let own_window = state.owner == std::process::id();
    let id = (wparam.0 & 0xffff) as u16;
    match message {
        WM_COMMAND if id == CONFIRM && own_window && !state.cancelled.load(Ordering::SeqCst) => {
            let _ = unsafe { EndDialog(hwnd, CONFIRM as isize) };
            1
        }
        WM_CLOSE => {
            let _ = unsafe { EndDialog(hwnd, IDCANCEL.0 as isize) };
            1
        }
        WM_COMMAND if id == IDCANCEL.0 as u16 => {
            let _ = unsafe { EndDialog(hwnd, IDCANCEL.0 as isize) };
            1
        }
        WM_TIMER if !own_window || state.cancelled.load(Ordering::SeqCst) => {
            let _ = unsafe { EndDialog(hwnd, IDCANCEL.0 as isize) };
            1
        }
        _ => 0,
    }
}

pub(super) fn confirm(description: &str, cancelled: &AtomicBool) -> Result<bool, String> {
    let (decision, owner) = show(description, cancelled)?;
    Ok(decision && owner == std::process::id())
}
fn show(description: &str, cancelled: &AtomicBool) -> Result<(bool, u32), String> {
    let template = template();
    let mut state = State {
        description,
        cancelled,
        owner: 0,
    };
    // SAFETY: the DWORD-aligned template and state live until the native modal loop returns.
    // No parent HWND is taken from IPC: the window belongs only to this process.
    let result = unsafe {
        DialogBoxIndirectParamW(
            None,
            template.as_ptr().cast(),
            None,
            Some(procedure),
            LPARAM((&mut state as *mut State<'_>) as isize),
        )
    };
    if result == -1 || state.owner == 0 {
        return Err(format!(
            "Could not open the confirmation window: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((result == CONFIRM as isize, state.owner))
}

#[cfg(test)]
#[test]
#[ignore = "opens a native window; requires an owner-approved GUI test session"]
fn dialog_window_is_created_by_this_process_and_cancellation_closes_it() {
    // This creates the actual native window and verifies its Windows process id. It needs no
    // UAC prompt and performs no write. A cancelled connection closes through the real timer.
    let (confirmed, owner) =
        show("Test only. Nothing will change.", &AtomicBool::new(true)).unwrap();
    assert!(!confirmed);
    assert_eq!(owner, std::process::id());
}

#[cfg(test)]
#[test]
fn template_keeps_room_for_long_previews() {
    // Guards the window size, so a long preview needs little scrolling while the buttons stay on
    // a 1366x768 screen at 125 % and a 1920x1080 screen at 175 %.
    let words: Vec<u16> = template()
        .iter()
        .flat_map(|d| [*d as u16, (*d >> 16) as u16])
        .collect();
    let (count, width, height) = (words[4], words[7], words[8]);
    assert_eq!((count, width, height), (3, 400, 260));
    let skip_string = |mut i: usize| {
        while words[i] != 0 {
            i += 1;
        }
        i + 1
    };
    // Title, then the point size word and the font name.
    let mut i = skip_string(skip_string(11) + 1);
    let mut controls = Vec::new();
    for _ in 0..count {
        i += i % 2; // Each control starts on a DWORD boundary.
        let style = u32::from(words[i]) | (u32::from(words[i + 1]) << 16);
        let [x, y, cx, cy, id] = [4, 5, 6, 7, 8].map(|k| words[i + k]);
        assert!(
            x + cx <= width && y + cy <= height,
            "control {id} is outside the window"
        );
        controls.push((id, style, cx, cy));
        i = skip_string(i + 11) + 1; // Class atom, title, then the creation data word.
    }
    let edit = controls.iter().find(|c| c.0 == DESCRIPTION).unwrap();
    assert!(edit.2 >= 370 && edit.3 >= 200);
    let cancel = controls.iter().find(|c| c.0 == IDCANCEL.0 as u16).unwrap();
    assert_ne!(
        cancel.1 & BS_DEFPUSHBUTTON as u32,
        0,
        "Cancel must stay the default button"
    );
}
