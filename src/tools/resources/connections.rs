//! The Connections page: who this PC is talking to right now, with an inspector on the right.
//!
//! The tables come from the Windows IP Helper API (no admin, no packet capture) on a worker
//! thread that only runs while the page is on screen. Host names are looked up only when the
//! user presses Resolve. Everything lives in memory; nothing is written to disk.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{
    GetNameInfoW, WSACleanup, WSAStartup, AF_INET, AF_INET6, NI_NAMEREQD, SOCKADDR, WSADATA,
};

use super::processes::{
    confirm_text, end_process, is_protected, kv, result_line, sort_arrow, visible, ActionError,
    INSPECTOR_PAD, INSPECTOR_WIDTH, PROTECTED_NOTE, REFRESH, ROW_HEIGHT,
};
use crate::ui::{CARD_PAD, GAP, RED};

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const NO_ERROR: u32 = 0;
const BANNER: &str = "This shows who your PC talks to right now. Names are only looked up when you press Resolve. Nothing is saved.";

// ---------------------------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    fn text(self) -> &'static str {
        match self {
            Proto::Tcp => "TCP",
            Proto::Udp => "UDP",
        }
    }
}

/// A TCP state as Windows numbers it (MIB_TCP_STATE).
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
struct TcpState(u32);

/// Plain-word state: Connected / Listening / Waiting / Closing.
fn state_text(state: TcpState) -> &'static str {
    match state.0 {
        5 => "Connected",
        2 => "Listening",
        3 | 4 => "Waiting",
        _ => "Closing",
    }
}

/// One connection or socket as sampled.
#[derive(Clone, Debug, PartialEq)]
struct Conn {
    pid: u32,
    name: String,
    exe: Option<String>,
    proto: Proto,
    local: (IpAddr, u16),
    /// None for UDP and for listening sockets.
    remote: Option<(IpAddr, u16)>,
    state: Option<TcpState>,
}

impl Conn {
    /// Stable key so the selection survives the refresh.
    fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.proto.text(),
            addr_text(self.local),
            self.remote.map_or(String::new(), addr_text),
            self.pid
        )
    }

    fn state_label(&self) -> &'static str {
        self.state.map_or("", state_text)
    }
}

/// "1.2.3.4:80" or "[::1]:80".
fn addr_text((ip, port): (IpAddr, u16)) -> String {
    match ip {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

fn remote_text(c: &Conn) -> String {
    c.remote.map_or_else(|| "-".to_string(), addr_text)
}

/// A port from a Windows DWORD: network order in the low 16 bits.
fn port_from_dword(dw: u32) -> u16 {
    u16::from_be((dw & 0xFFFF) as u16)
}

fn rd_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd_v4(b: &[u8], at: usize) -> Option<IpAddr> {
    let s = b.get(at..at + 4)?;
    Some(IpAddr::V4(Ipv4Addr::new(s[0], s[1], s[2], s[3])))
}

/// IPv6 from 16 bytes in network order; the scope id is not read.
fn rd_v6(b: &[u8], at: usize) -> Option<IpAddr> {
    let s: [u8; 16] = b.get(at..at + 16)?.try_into().ok()?;
    Some(IpAddr::V6(Ipv6Addr::from(s)))
}

/// A raw row from one of the four tables, before the process name is known.
struct Raw {
    pid: u32,
    proto: Proto,
    local: (IpAddr, u16),
    remote: Option<(IpAddr, u16)>,
    state: Option<TcpState>,
}

/// Walks `rows` fixed-size records after the 4-byte count.
fn parse_table(
    bytes: &[u8],
    row_len: usize,
    mut one: impl FnMut(&[u8]) -> Option<Raw>,
) -> Vec<Raw> {
    let Some(count) = rd_u32(bytes, 0) else {
        return Vec::new();
    };
    (0..count as usize)
        .map_while(|i| bytes.get(4 + i * row_len..4 + (i + 1) * row_len))
        .filter_map(&mut one)
        .collect()
}

/// MIB_TCPROW_OWNER_PID: state, local addr, local port, remote addr, remote port, pid.
fn parse_tcp4(bytes: &[u8]) -> Vec<Raw> {
    parse_table(bytes, 24, |r| {
        let state = TcpState(rd_u32(r, 0)?);
        let local = (rd_v4(r, 4)?, port_from_dword(rd_u32(r, 8)?));
        let remote = (rd_v4(r, 12)?, port_from_dword(rd_u32(r, 16)?));
        Some(tcp_raw(rd_u32(r, 20)?, local, remote, state))
    })
}

/// MIB_TCP6ROW_OWNER_PID: local addr, scope, port, remote addr, scope, port, state, pid.
fn parse_tcp6(bytes: &[u8]) -> Vec<Raw> {
    parse_table(bytes, 56, |r| {
        let local = (rd_v6(r, 0)?, port_from_dword(rd_u32(r, 20)?));
        let remote = (rd_v6(r, 24)?, port_from_dword(rd_u32(r, 44)?));
        let state = TcpState(rd_u32(r, 48)?);
        Some(tcp_raw(rd_u32(r, 52)?, local, remote, state))
    })
}

fn tcp_raw(pid: u32, local: (IpAddr, u16), remote: (IpAddr, u16), state: TcpState) -> Raw {
    // A listening socket has no peer yet.
    let remote = (state.0 != 2).then_some(remote);
    Raw {
        pid,
        proto: Proto::Tcp,
        local,
        remote,
        state: Some(state),
    }
}

/// MIB_UDPROW_OWNER_PID: local addr, local port, pid.
fn parse_udp4(bytes: &[u8]) -> Vec<Raw> {
    parse_table(bytes, 12, |r| {
        let local = (rd_v4(r, 0)?, port_from_dword(rd_u32(r, 4)?));
        Some(udp_raw(rd_u32(r, 8)?, local))
    })
}

/// MIB_UDP6ROW_OWNER_PID: local addr, scope, port, pid.
fn parse_udp6(bytes: &[u8]) -> Vec<Raw> {
    parse_table(bytes, 28, |r| {
        let local = (rd_v6(r, 0)?, port_from_dword(rd_u32(r, 20)?));
        Some(udp_raw(rd_u32(r, 24)?, local))
    })
}

fn udp_raw(pid: u32, local: (IpAddr, u16)) -> Raw {
    Raw {
        pid,
        proto: Proto::Udp,
        local,
        remote: None,
        state: None,
    }
}

/// True for 127.0.0.0/8 and ::1 (also in IPv4-mapped form).
fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v| v.is_loopback()),
    }
}

/// Entries that stay on this PC: a loopback peer, or a loopback-only socket with no peer.
/// Sockets bound to 0.0.0.0 or :: are reachable from outside and are kept.
fn is_own(c: &Conn) -> bool {
    match c.remote {
        Some((ip, _)) if is_loopback(ip) => true,
        Some((ip, _)) if !ip.is_unspecified() => false,
        _ => is_loopback(c.local.0),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortKey {
    Process,
    Pid,
    Proto,
    Local,
    Remote,
    State,
}

/// Orders rows by `key`; ties fall back to process name, then pid, then addresses.
fn sort_conns(rows: &mut [Conn], key: SortKey, descending: bool) {
    rows.sort_by(|a, b| {
        let ord = match key {
            SortKey::Process => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::Proto => a.proto.cmp(&b.proto),
            SortKey::Local => a.local.cmp(&b.local),
            SortKey::Remote => a.remote.cmp(&b.remote),
            SortKey::State => a.state_label().cmp(b.state_label()),
        };
        let ord = if descending { ord.reverse() } else { ord };
        ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then(a.pid.cmp(&b.pid))
            .then(a.local.cmp(&b.local))
            .then(a.remote.cmp(&b.remote))
    });
}

/// Applies the two quick filters.
fn quick_filter(rows: &[Conn], connected_only: bool, hide_own: bool) -> Vec<Conn> {
    rows.iter()
        .filter(|c| !connected_only || c.state.is_none_or(|s| s.0 == 5))
        .filter(|c| !hide_own || !is_own(c))
        .cloned()
        .collect()
}

/// Keeps rows whose process, pid, address or port contains `filter` (case-insensitive).
fn filter_conns(rows: Vec<Conn>, filter: &str) -> Vec<Conn> {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|c| {
            c.name.to_lowercase().contains(&needle)
                || c.pid.to_string().contains(&needle)
                || addr_text(c.local).contains(&needle)
                || c.remote.is_some_and(|r| addr_text(r).contains(&needle))
        })
        .collect()
}

/// The `n` processes with the most connections, most first; ties by name.
fn top_processes(rows: &[Conn], n: usize) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for c in rows {
        counts
            .entry(c.name.to_lowercase())
            .or_insert_with(|| (c.name.clone(), 0))
            .1 += 1;
    }
    let mut v: Vec<(String, usize)> = counts.into_values().collect();
    v.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    v.truncate(n);
    v
}

/// "spotify.exe 23, chrome.exe 14" or an empty string.
fn summary_text(top: &[(String, usize)]) -> String {
    top.iter()
        .map(|(n, c)| format!("{n} {c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Plain text for the clipboard.
fn copy_row(c: &Conn) -> String {
    let mut s = format!(
        "{}, pid {}, {}, local {}, remote {}",
        c.name,
        c.pid,
        c.proto.text(),
        addr_text(c.local),
        remote_text(c)
    );
    if c.state.is_some() {
        s.push_str(", ");
        s.push_str(c.state_label());
    }
    s
}

/// Joins raw rows with process names. Unknown pids get "Unknown".
fn name_rows(raw: Vec<Raw>, names: &HashMap<u32, (String, Option<String>)>) -> Vec<Conn> {
    raw.into_iter()
        .map(|r| {
            let (name, exe) = match names.get(&r.pid) {
                Some((n, e)) => (n.clone(), e.clone()),
                None if r.pid == 0 => ("System".to_string(), None),
                None => ("Unknown".to_string(), None),
            };
            Conn {
                pid: r.pid,
                name,
                exe,
                proto: r.proto,
                local: r.local,
                remote: r.remote,
                state: r.state,
            }
        })
        .collect()
}

/// What the Resolve button shows once the lookup is done.
fn resolved_text(name: &Option<String>) -> String {
    name.clone().unwrap_or_else(|| "No name found".to_string())
}

// ---------------------------------------------------------------------------------------------
// Windows calls
// ---------------------------------------------------------------------------------------------

/// The standard two-call pattern: ask for the size, then fill a buffer, retrying if it grew.
fn fetch(call: impl Fn(*mut core::ffi::c_void, *mut u32) -> u32) -> Result<Vec<u8>, u32> {
    let mut size: u32 = 0;
    for _ in 0..5 {
        // Whole u32 words keep the buffer aligned for the API.
        let mut buf = vec![0u32; (size as usize).div_ceil(4).max(1)];
        let ptr = if size == 0 {
            std::ptr::null_mut()
        } else {
            buf.as_mut_ptr().cast()
        };
        match call(ptr, &mut size) {
            NO_ERROR => {
                let bytes: Vec<u8> = buf.iter().flat_map(|w| w.to_ne_bytes()).collect();
                return Ok(bytes[..(size as usize).min(bytes.len())].to_vec());
            }
            ERROR_INSUFFICIENT_BUFFER => size = size.saturating_add(512),
            other => return Err(other),
        }
    }
    Err(ERROR_INSUFFICIENT_BUFFER)
}

fn read_tcp(family: u32) -> Result<Vec<u8>, u32> {
    // SAFETY: `fetch` passes a buffer of at least `*size` bytes (or null with size 0).
    fetch(|p, s| unsafe {
        GetExtendedTcpTable(
            (!p.is_null()).then_some(p),
            s,
            false,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    })
}

fn read_udp(family: u32) -> Result<Vec<u8>, u32> {
    // SAFETY: as above.
    fetch(|p, s| unsafe {
        GetExtendedUdpTable(
            (!p.is_null()).then_some(p),
            s,
            false,
            family,
            UDP_TABLE_OWNER_PID,
            0,
        )
    })
}

/// Every TCP and UDP socket on the machine, IPv4 and IPv6. Read-only.
fn read_all() -> Result<Vec<Raw>, String> {
    let fail = |what: &str, e: u32| format!("Could not read the {what} list (error {e})");
    let mut raw = Vec::new();
    raw.extend(parse_tcp4(
        &read_tcp(AF_INET.0 as u32).map_err(|e| fail("TCP", e))?,
    ));
    raw.extend(parse_tcp6(
        &read_tcp(AF_INET6.0 as u32).map_err(|e| fail("TCP", e))?,
    ));
    raw.extend(parse_udp4(
        &read_udp(AF_INET.0 as u32).map_err(|e| fail("UDP", e))?,
    ));
    raw.extend(parse_udp6(
        &read_udp(AF_INET6.0 as u32).map_err(|e| fail("UDP", e))?,
    ));
    Ok(raw)
}

/// 4-byte aligned socket address storage big enough for IPv6.
#[repr(C, align(4))]
struct SockBuf([u8; 28]);

/// Builds a sockaddr_in / sockaddr_in6 by hand; returns it with its length.
fn sockaddr(ip: IpAddr) -> (SockBuf, i32) {
    let mut b = SockBuf([0; 28]);
    match ip {
        IpAddr::V4(v4) => {
            b.0[0..2].copy_from_slice(&AF_INET.0.to_ne_bytes());
            b.0[4..8].copy_from_slice(&v4.octets());
            (b, 16)
        }
        IpAddr::V6(v6) => {
            b.0[0..2].copy_from_slice(&AF_INET6.0.to_ne_bytes());
            b.0[8..24].copy_from_slice(&v6.octets());
            (b, 28)
        }
    }
}

/// Reverse lookup over the network. None when no name exists.
fn reverse_lookup(ip: IpAddr) -> Option<String> {
    let mut data = WSADATA::default();
    // SAFETY: plain FFI; `data` is a valid out parameter.
    if unsafe { WSAStartup(0x0202, &mut data) } != 0 {
        return None;
    }
    let (sa, len) = sockaddr(ip);
    let mut host = [0u16; 1025];
    // SAFETY: `sa` holds a valid sockaddr of `len` bytes and `host` is a writable buffer.
    let rc = unsafe {
        GetNameInfoW(
            (&sa as *const SockBuf).cast::<SOCKADDR>(),
            windows::Win32::Networking::WinSock::socklen_t(len),
            Some(&mut host),
            None,
            NI_NAMEREQD as i32,
        )
    };
    // SAFETY: balances the successful WSAStartup above.
    unsafe { WSACleanup() };
    if rc != 0 {
        return None;
    }
    let end = host.iter().position(|&c| c == 0).unwrap_or(host.len());
    let name = String::from_utf16_lossy(&host[..end]);
    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------------------------
// Sampling worker
// ---------------------------------------------------------------------------------------------

type Snapshot = Result<Vec<Conn>, String>;

struct Worker {
    rx: Receiver<Snapshot>,
    last_ping: Arc<AtomicU64>,
    epoch: Instant,
}

impl Worker {
    fn start(ctx: &egui::Context) -> Worker {
        let (tx, rx) = mpsc::channel();
        let last_ping = Arc::new(AtomicU64::new(0));
        let epoch = Instant::now();
        let ping = Arc::clone(&last_ping);
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("connection-sampler".into())
            .spawn(move || {
                let Ok(mut sys) = System::new() else {
                    return;
                };
                let kind = ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet);
                let mut last: Option<Instant> = None;
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let now_ms = epoch.elapsed().as_millis() as u64;
                    if !visible(ping.load(Ordering::Relaxed), now_ms) {
                        last = None;
                        continue;
                    }
                    if last.is_some_and(|t| t.elapsed() < REFRESH) {
                        continue;
                    }
                    last = Some(Instant::now());
                    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
                    let names: HashMap<u32, (String, Option<String>)> = sys
                        .processes()
                        .values()
                        .map(|p| {
                            (
                                p.pid().as_u32(),
                                (
                                    p.name().to_string_lossy().into_owned(),
                                    p.exe().map(|e| e.display().to_string()),
                                ),
                            )
                        })
                        .collect();
                    let snap = read_all().map(|raw| name_rows(raw, &names));
                    if tx.send(snap).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        // If the thread cannot start, the channel just stays empty.
        let _ = spawned;
        Worker {
            rx,
            last_ping,
            epoch,
        }
    }

    fn ping(&self) {
        let now_ms = self.epoch.elapsed().as_millis() as u64;
        self.last_ping.store(now_ms + 1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------------------------

enum Intent {
    Copy(String),
    Resolve(IpAddr),
    AskEnd(u32),
    ConfirmEnd,
    CancelEnd,
}

pub(super) struct Page {
    worker: Option<Worker>,
    live: Vec<Conn>,
    frozen: Option<Vec<Conn>>,
    error: Option<String>,
    got_data: bool,
    filter: String,
    connected_only: bool,
    hide_own: bool,
    banner_open: bool,
    sort_key: SortKey,
    descending: bool,
    selected: Option<String>,
    /// Pid waiting for the second click on End process.
    confirm: Option<u32>,
    denied: HashSet<u32>,
    end_job: Option<Receiver<(String, bool)>>,
    end_line: Option<String>,
    /// Looked-up names, kept in memory only.
    names: HashMap<IpAddr, Option<String>>,
    resolving: Option<(IpAddr, Receiver<Option<String>>)>,
}

impl Page {
    pub(super) fn new() -> Self {
        Self {
            worker: None,
            live: Vec::new(),
            frozen: None,
            error: None,
            got_data: false,
            filter: String::new(),
            connected_only: true,
            hide_own: true,
            banner_open: true,
            sort_key: SortKey::Process,
            descending: false,
            selected: None,
            confirm: None,
            denied: HashSet::new(),
            end_job: None,
            end_line: None,
            names: HashMap::new(),
            resolving: None,
        }
    }

    /// Takes in whatever the workers have produced. Never blocks.
    pub(super) fn tick(&mut self) {
        if let Some(worker) = &self.worker {
            for snap in worker.rx.try_iter() {
                self.got_data = true;
                match snap {
                    Ok(rows) => {
                        self.live = rows;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        let done = self.end_job.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some((line, denied_pid)) = done {
            self.end_job = None;
            self.end_line = Some(line);
            if denied_pid {
                if let Some(pid) = self.selected_pid() {
                    self.denied.insert(pid);
                }
            }
        }
        let name = self
            .resolving
            .as_ref()
            .and_then(|(ip, rx)| rx.try_recv().ok().map(|n| (*ip, n)));
        if let Some((ip, n)) = name {
            self.resolving = None;
            self.names.insert(ip, n);
        }
    }

    fn selected_pid(&self) -> Option<u32> {
        let key = self.selected.as_ref()?;
        self.live.iter().find(|c| &c.key() == key).map(|c| c.pid)
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) {
        let worker = self.worker.get_or_insert_with(|| Worker::start(ui.ctx()));
        worker.ping();

        let shown: &[Conn] = self.frozen.as_deref().unwrap_or(&self.live);
        let base = quick_filter(shown, self.connected_only, self.hide_own);
        let top = top_processes(&base, 5);
        let mut rows = filter_conns(base, &self.filter);
        sort_conns(&mut rows, self.sort_key, self.descending);
        if let Some(sel) = &self.selected {
            if !rows.iter().any(|c| &c.key() == sel) {
                self.selected = None;
                self.confirm = None;
            }
        }
        let chosen: Option<Conn> = self
            .selected
            .as_ref()
            .and_then(|k| rows.iter().find(|c| &c.key() == k))
            .cloned();

        let mut intent: Option<Intent> = None;
        let inspector = egui::Frame::NONE
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(INSPECTOR_PAD);
        let panel = egui::Panel::right("connections_inspector")
            .resizable(false)
            .exact_size(INSPECTOR_WIDTH)
            .frame(inspector)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        intent = self.inspector(ui, chosen.as_ref());
                    });
            });
        let rect = panel.response.rect;
        ui.painter().vline(
            rect.left(),
            rect.y_range(),
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        let side = egui::Frame::NONE.inner_margin(CARD_PAD);
        egui::CentralPanel::default().frame(side).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = Vec2::new(6.0, 1.0);
            self.banner(ui);
            self.toolbar(ui, rows.len());
            let line = summary_text(&top);
            if !line.is_empty() {
                ui.label(
                    RichText::new(format!("Most connections: {line}"))
                        .small()
                        .weak(),
                );
            }
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).color(RED).small());
            }
            ui.add_space(2.0);
            self.table(ui, &rows);
        });

        if let Some(intent) = intent {
            self.apply(intent, ui.ctx(), chosen.as_ref());
        }
    }

    fn banner(&mut self, ui: &mut egui::Ui) {
        if !self.banner_open {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(BANNER).small().weak());
            if ui.small_button("Dismiss").clicked() {
                self.banner_open = false;
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, count: usize) {
        let mut paused = self.frozen.is_some();
        let was_paused = paused;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter by process, address or port")
                    .desired_width(190.0),
            );
            ui.checkbox(&mut self.connected_only, "Connected only");
            ui.checkbox(&mut self.hide_own, "Hide this PC's own (127.0.0.1 / ::1)");
            ui.toggle_value(&mut paused, if was_paused { "Paused" } else { "Pause" })
                .on_hover_text("Freeze the list so rows stop moving. Reading carries on.");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if !self.got_data && self.error.is_none() {
                    "Reading...".to_string()
                } else if count == 1 {
                    "1 connection".to_string()
                } else {
                    format!("{count} connections")
                };
                ui.label(RichText::new(text).weak());
            });
        });
        if paused != was_paused {
            self.frozen = paused.then(|| self.live.clone());
        }
    }

    fn table(&mut self, ui: &mut egui::Ui, rows: &[Conn]) {
        let mut sort_click: Option<SortKey> = None;
        let mut click: Option<String> = None;
        let (sort_key, descending) = (self.sort_key, self.descending);
        let selected = self.selected.clone();

        TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .column(Column::remainder().at_least(90.0).clip(true))
            .column(Column::exact(52.0))
            .column(Column::exact(44.0))
            .column(Column::exact(150.0).clip(true))
            .column(Column::exact(150.0).clip(true))
            .column(Column::exact(72.0))
            .header(ROW_HEIGHT, |mut header| {
                let mut sortable = |label: &str, key: SortKey| {
                    header.col(|ui| {
                        let arrow = sort_arrow(sort_key == key, descending);
                        let b = egui::Button::new(
                            RichText::new(format!("{label}{arrow}")).small().strong(),
                        )
                        .frame(false);
                        if ui.add(b).clicked() {
                            sort_click = Some(key);
                        }
                    });
                };
                sortable("Process", SortKey::Process);
                sortable("PID", SortKey::Pid);
                sortable("Proto", SortKey::Proto);
                sortable("Local", SortKey::Local);
                sortable("Remote", SortKey::Remote);
                sortable("State", SortKey::State);
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let c = &rows[row.index()];
                    let key = c.key();
                    row.set_selected(selected.as_deref() == Some(key.as_str()));
                    let cell = |ui: &mut egui::Ui, text: String| {
                        ui.add(egui::Label::new(text).selectable(false).truncate());
                    };
                    row.col(|ui| cell(ui, c.name.clone()));
                    row.col(|ui| cell(ui, c.pid.to_string()));
                    row.col(|ui| cell(ui, c.proto.text().to_string()));
                    row.col(|ui| cell(ui, addr_text(c.local)));
                    row.col(|ui| cell(ui, remote_text(c)));
                    row.col(|ui| cell(ui, c.state_label().to_string()));
                    if row.response().clicked() {
                        click = Some(key);
                    }
                });
            });

        if let Some(key) = click {
            self.selected = Some(key);
            self.confirm = None;
        }
        if let Some(key) = sort_click {
            if self.sort_key == key {
                self.descending = !self.descending;
            } else {
                self.sort_key = key;
                self.descending = false;
            }
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui, chosen: Option<&Conn>) -> Option<Intent> {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        let Some(c) = chosen else {
            ui.label(RichText::new("Nothing selected.").weak());
            ui.label(RichText::new("Click a row to see details.").weak().small());
            self.end_result(ui);
            return None;
        };
        let mut intent = None;
        ui.label(RichText::new(&c.name).strong());
        kv(ui, "PID", &c.pid.to_string());
        if let Some(path) = &c.exe {
            kv(ui, "Path", path);
        }
        kv(ui, "Protocol", c.proto.text());
        kv(ui, "Local", &addr_text(c.local));
        kv(ui, "Remote", &remote_text(c));
        if c.state.is_some() {
            kv(ui, "State", c.state_label());
        }
        ui.separator();

        if let Some(r) = c.remote {
            let ip = r.0;
            if ui.button("Copy remote address").clicked() {
                intent = Some(Intent::Copy(addr_text(r)));
            }
            let busy = self.resolving.is_some();
            match self.names.get(&ip) {
                Some(name) => {
                    kv(ui, "Host name", &resolved_text(name));
                }
                None => {
                    let waiting = self.resolving.as_ref().is_some_and(|(i, _)| *i == ip);
                    if ui
                        .add_enabled(!busy, egui::Button::new("Resolve host name"))
                        .on_hover_text("Looks up the name over the network")
                        .clicked()
                    {
                        intent = Some(Intent::Resolve(ip));
                    }
                    let note = if waiting {
                        "Looking up..."
                    } else {
                        "Looks up the name over the network"
                    };
                    ui.label(RichText::new(note).weak().small());
                }
            }
        }
        if ui.button("Copy row").clicked() {
            intent = Some(Intent::Copy(copy_row(c)));
        }
        ui.separator();

        let own_pid = std::process::id();
        let protected = is_protected(c.pid, &c.name, own_pid, &self.denied);
        let busy = self.end_job.is_some();
        if self.confirm.is_some_and(|p| p != c.pid) {
            self.confirm = None;
        }
        if protected {
            ui.add_enabled(false, egui::Button::new("End process"));
            ui.label(RichText::new(PROTECTED_NOTE).weak().small());
        } else if self.confirm == Some(c.pid) {
            ui.label(RichText::new(confirm_text(&c.name, 1)).small());
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new(RichText::new("End").color(RED)))
                    .clicked()
                {
                    intent = Some(Intent::ConfirmEnd);
                }
                if ui.button("Cancel").clicked() {
                    intent = Some(Intent::CancelEnd);
                }
            });
        } else if ui
            .add_enabled(!busy, egui::Button::new("End process"))
            .clicked()
        {
            intent = Some(Intent::AskEnd(c.pid));
        }
        if busy {
            ui.label(RichText::new("Working...").weak().small());
        }
        ui.add_space(GAP - 2.0);
        self.end_result(ui);
        intent
    }

    fn end_result(&self, ui: &mut egui::Ui) {
        if let Some(line) = &self.end_line {
            ui.label(RichText::new(line).small());
        }
    }

    fn apply(&mut self, intent: Intent, ctx: &egui::Context, chosen: Option<&Conn>) {
        match intent {
            Intent::Copy(text) => ctx.copy_text(text),
            Intent::AskEnd(pid) => self.confirm = Some(pid),
            Intent::CancelEnd => self.confirm = None,
            Intent::ConfirmEnd => {
                self.confirm = None;
                if let Some(c) = chosen {
                    self.start_end(ctx, c);
                }
            }
            Intent::Resolve(ip) => {
                if self.resolving.is_some() {
                    return;
                }
                let (tx, rx) = mpsc::channel();
                let ctx = ctx.clone();
                let spawned = std::thread::Builder::new()
                    .name("connection-resolve".into())
                    .spawn(move || {
                        let _ = tx.send(reverse_lookup(ip));
                        ctx.request_repaint();
                    });
                if spawned.is_ok() {
                    self.resolving = Some((ip, rx));
                } else {
                    self.names.insert(ip, None);
                }
            }
        }
    }

    fn start_end(&mut self, ctx: &egui::Context, c: &Conn) {
        if self.end_job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let (pid, name) = (c.pid, c.name.clone());
        let spawned = std::thread::Builder::new()
            .name("connection-end".into())
            .spawn(move || {
                let r = end_process(pid);
                let line = result_line("Ended", &name, pid, r);
                let _ = tx.send((line, r == Err(ActionError::AccessDenied)));
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.end_job = Some(rx);
            self.end_line = None;
        } else {
            self.end_line = Some("Could not start the action".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn conn(pid: u32, name: &str, local: (IpAddr, u16), remote: Option<(IpAddr, u16)>) -> Conn {
        Conn {
            pid,
            name: name.to_string(),
            exe: None,
            proto: Proto::Tcp,
            local,
            remote,
            state: Some(TcpState(5)),
        }
    }

    fn dw(bytes: [u8; 4]) -> u32 {
        u32::from_le_bytes(bytes)
    }

    #[test]
    fn port_conversion_uses_low_word_network_order() {
        // Port 443 = 0x01BB sent as bytes 01 BB; upper bytes are garbage Windows may leave.
        assert_eq!(port_from_dword(dw([0x01, 0xBB, 0x00, 0x00])), 443);
        assert_eq!(port_from_dword(dw([0x01, 0xBB, 0xAB, 0xCD])), 443);
        assert_eq!(port_from_dword(dw([0x00, 0x50, 0x00, 0x00])), 80);
        assert_eq!(port_from_dword(0), 0);
        assert_eq!(port_from_dword(dw([0xFF, 0xFF, 0, 0])), 65535);
    }

    #[test]
    fn address_text_v4_v6() {
        assert_eq!(addr_text((v4(10, 0, 0, 5), 443)), "10.0.0.5:443");
        assert_eq!(addr_text((v4(0, 0, 0, 0), 0)), "0.0.0.0:0");
        assert_eq!(
            addr_text((IpAddr::V6(Ipv6Addr::UNSPECIFIED), 80)),
            "[::]:80"
        );
        assert_eq!(
            addr_text((IpAddr::V6(Ipv6Addr::LOCALHOST), 8080)),
            "[::1]:8080"
        );
        let ip: Ipv6Addr = "2001:db8::1".parse().unwrap();
        assert_eq!(addr_text((IpAddr::V6(ip), 22)), "[2001:db8::1]:22");
    }

    #[test]
    fn parses_tcp4_row() {
        let mut b = 1u32.to_le_bytes().to_vec();
        for w in [
            5u32.to_le_bytes(),
            [192, 168, 1, 10],
            [0x01, 0xBB, 0, 0],
            [93, 184, 216, 34],
            [0x00, 0x50, 0, 0],
            4321u32.to_le_bytes(),
        ] {
            b.extend(w);
        }
        let rows = parse_tcp4(&b);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.local, (v4(192, 168, 1, 10), 443));
        assert_eq!(r.remote, Some((v4(93, 184, 216, 34), 80)));
        assert_eq!(r.state, Some(TcpState(5)));
        assert_eq!(r.pid, 4321);
    }

    #[test]
    fn listening_has_no_remote_and_short_buffers_are_safe() {
        let mut b = 1u32.to_le_bytes().to_vec();
        for w in [
            2u32.to_le_bytes(),
            [0, 0, 0, 0],
            [0x1F, 0x90, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            7u32.to_le_bytes(),
        ] {
            b.extend(w);
        }
        let rows = parse_tcp4(&b);
        assert_eq!(rows[0].local.1, 8080);
        assert!(rows[0].remote.is_none());
        // Count says 2 rows but only one is present: no panic, one row.
        b[0] = 2;
        assert_eq!(parse_tcp4(&b).len(), 1);
        assert!(parse_tcp4(&[]).is_empty());
        assert!(parse_tcp4(&[1, 0]).is_empty());
    }

    #[test]
    fn parses_udp_and_v6_rows() {
        let mut u = 1u32.to_le_bytes().to_vec();
        u.extend([0u8, 0, 0, 0, 0x00, 0x35, 0, 0]);
        u.extend(9u32.to_le_bytes());
        let rows = parse_udp4(&u);
        assert_eq!(rows[0].local, (v4(0, 0, 0, 0), 53));
        assert_eq!(rows[0].proto, Proto::Udp);
        assert!(rows[0].remote.is_none() && rows[0].state.is_none());

        // UDP6: ::1, scope 99 (ignored), port 5353, pid 3.
        let mut u6 = 1u32.to_le_bytes().to_vec();
        let mut addr = [0u8; 16];
        addr[15] = 1;
        u6.extend(addr);
        u6.extend(99u32.to_le_bytes());
        u6.extend([0x14, 0xE9, 0, 0]);
        u6.extend(3u32.to_le_bytes());
        let rows = parse_udp6(&u6);
        assert_eq!(rows[0].local, (IpAddr::V6(Ipv6Addr::LOCALHOST), 5353));
        assert_eq!(rows[0].pid, 3);

        // TCP6 established: ::1:1000 -> ::1:2000, pid 8.
        let mut t6 = 1u32.to_le_bytes().to_vec();
        t6.extend(addr);
        t6.extend(1u32.to_le_bytes());
        t6.extend([0x03, 0xE8, 0, 0]);
        t6.extend(addr);
        t6.extend(2u32.to_le_bytes());
        t6.extend([0x07, 0xD0, 0, 0]);
        t6.extend(5u32.to_le_bytes());
        t6.extend(8u32.to_le_bytes());
        let rows = parse_tcp6(&t6);
        assert_eq!(rows[0].local.1, 1000);
        assert_eq!(rows[0].remote.map(|r| r.1), Some(2000));
        assert_eq!(rows[0].pid, 8);
    }

    #[test]
    fn state_words() {
        assert_eq!(state_text(TcpState(5)), "Connected");
        assert_eq!(state_text(TcpState(2)), "Listening");
        assert_eq!(state_text(TcpState(3)), "Waiting");
        assert_eq!(state_text(TcpState(4)), "Waiting");
        for s in [1, 6, 7, 8, 9, 10, 11, 12] {
            assert_eq!(state_text(TcpState(s)), "Closing");
        }
    }

    #[test]
    fn own_detection_keeps_unspecified_listeners() {
        let lo = (v4(127, 0, 0, 1), 1);
        let any = (v4(0, 0, 0, 0), 80);
        let ext = (v4(8, 8, 8, 8), 443);
        assert!(is_own(&conn(1, "a", lo, Some((v4(127, 5, 5, 5), 2)))));
        assert!(is_own(&conn(1, "a", lo, None)));
        assert!(is_own(&conn(
            1,
            "a",
            (IpAddr::V6(Ipv6Addr::LOCALHOST), 1),
            None
        )));
        assert!(!is_own(&conn(1, "a", any, None)));
        assert!(!is_own(&conn(
            1,
            "a",
            (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 80),
            None
        )));
        assert!(!is_own(&conn(1, "a", (v4(192, 168, 1, 2), 5), Some(ext))));
        assert!(is_loopback("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_loopback(v4(128, 0, 0, 1)));
    }

    fn sample() -> Vec<Conn> {
        let mut listen = conn(3, "svc.exe", (v4(0, 0, 0, 0), 135), None);
        listen.state = Some(TcpState(2));
        let mut udp = conn(4, "dns.exe", (v4(0, 0, 0, 0), 53), None);
        udp.proto = Proto::Udp;
        udp.state = None;
        vec![
            conn(
                1,
                "Chrome.exe",
                (v4(10, 0, 0, 2), 5000),
                Some((v4(1, 1, 1, 1), 443)),
            ),
            conn(
                2,
                "chrome.exe",
                (v4(10, 0, 0, 2), 5001),
                Some((v4(2, 2, 2, 2), 443)),
            ),
            conn(
                5,
                "spotify.exe",
                (v4(10, 0, 0, 2), 5002),
                Some((v4(3, 3, 3, 3), 4070)),
            ),
            conn(
                6,
                "dev.exe",
                (v4(127, 0, 0, 1), 6000),
                Some((v4(127, 0, 0, 1), 6001)),
            ),
            listen,
            udp,
        ]
    }

    #[test]
    fn quick_filters() {
        let rows = sample();
        assert_eq!(quick_filter(&rows, false, false).len(), 6);
        // Connected only drops the listener but keeps UDP (no state).
        assert_eq!(quick_filter(&rows, true, false).len(), 5);
        assert_eq!(quick_filter(&rows, false, true).len(), 5);
        assert_eq!(quick_filter(&rows, true, true).len(), 4);
    }

    #[test]
    fn text_filter() {
        let rows = sample();
        assert_eq!(filter_conns(rows.clone(), "CHROME").len(), 2);
        assert_eq!(filter_conns(rows.clone(), ":443").len(), 2);
        assert_eq!(filter_conns(rows.clone(), "3.3.3").len(), 1);
        assert_eq!(filter_conns(rows.clone(), "5002").len(), 1);
        assert_eq!(filter_conns(rows.clone(), " ").len(), 6);
        assert!(filter_conns(rows, "zzz").is_empty());
    }

    #[test]
    fn sorting() {
        let mut rows = sample();
        sort_conns(&mut rows, SortKey::Process, false);
        assert_eq!(rows[0].name, "Chrome.exe");
        assert_eq!(rows[1].name, "chrome.exe");
        sort_conns(&mut rows, SortKey::Pid, true);
        assert_eq!(rows[0].pid, 6);
        sort_conns(&mut rows, SortKey::Proto, false);
        assert_eq!(rows.last().unwrap().proto, Proto::Udp);
        sort_conns(&mut rows, SortKey::Local, false);
        assert_eq!(rows[0].local.0, v4(0, 0, 0, 0));
        assert_eq!(rows[0].local.1, 53);
        sort_conns(&mut rows, SortKey::Remote, false);
        assert!(rows[0].remote.is_none());
        sort_conns(&mut rows, SortKey::State, false);
        assert_eq!(rows[0].state_label(), "");
    }

    #[test]
    fn top_five_summary() {
        let rows = sample();
        let top = top_processes(&rows, 2);
        assert_eq!(top[0], ("Chrome.exe".to_string(), 2));
        assert_eq!(top.len(), 2);
        assert_eq!(top[1].1, 1);
        assert_eq!(summary_text(&top[..1]), "Chrome.exe 2");
        assert_eq!(summary_text(&[]), "");
        assert_eq!(top_processes(&rows, 5).len(), 5);
    }

    #[test]
    fn copy_text_and_naming() {
        let rows = sample();
        assert_eq!(
            copy_row(&rows[0]),
            "Chrome.exe, pid 1, TCP, local 10.0.0.2:5000, remote 1.1.1.1:443, Connected"
        );
        assert_eq!(
            copy_row(&rows[5]),
            "dns.exe, pid 4, UDP, local 0.0.0.0:53, remote -"
        );
        let mut names = HashMap::new();
        names.insert(7u32, ("x.exe".to_string(), Some("C:\\x.exe".to_string())));
        let raw = vec![
            udp_raw(7, (v4(0, 0, 0, 0), 1)),
            udp_raw(0, (v4(0, 0, 0, 0), 2)),
            udp_raw(99, (v4(0, 0, 0, 0), 3)),
        ];
        let named = name_rows(raw, &names);
        assert_eq!(named[0].name, "x.exe");
        assert_eq!(named[0].exe.as_deref(), Some("C:\\x.exe"));
        assert_eq!(named[1].name, "System");
        assert_eq!(named[2].name, "Unknown");
        assert_eq!(resolved_text(&None), "No name found");
        assert_eq!(resolved_text(&Some("h.example".into())), "h.example");
    }

    #[test]
    fn sockaddr_layout() {
        let (b, len) = sockaddr(v4(1, 2, 3, 4));
        assert_eq!(len, 16);
        assert_eq!(&b.0[4..8], &[1, 2, 3, 4]);
        let (b, len) = sockaddr(IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(len, 28);
        assert_eq!(b.0[23], 1);
    }

    #[test]
    fn reading_the_tables_works() {
        // Read-only call; an empty list is fine, an error is not.
        let rows = read_all().expect("tables readable");
        assert!(rows.len() < 1_000_000);
    }
}
