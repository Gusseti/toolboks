//! Windows Event Log access behind a trait, plus crash dump header reading.
//!
//! The real source uses the Event Log API (EvtQuery / EvtNext / EvtRender). Everything that can be
//! pure (XPath building, time conversion, XML parsing, merging, dump header parsing) is a plain
//! function with unit tests. Reading the log is read-only.
#![allow(dead_code)]

use std::io::Read;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::EventLog::{
    EvtClose, EvtFormatMessage, EvtFormatMessageEvent, EvtNext, EvtOpenPublisherMetadata, EvtQuery,
    EvtQueryChannelPath, EvtQueryForwardDirection, EvtQueryReverseDirection, EvtRender,
    EvtRenderEventXml, EVT_HANDLE,
};

use super::model::{ChannelStatus, Event};

/// Marker for `Query::levels`: instead of a level filter, select the crash marker events
/// (Kernel-Power 41, EventLog 6008, WER-SystemErrorReporting 1001) at any level.
pub const CRASH_MARKERS: u8 = 255;

#[derive(Clone, Debug)]
pub struct Query {
    pub channel: String,
    /// Inclusive lower bound, ms since 1970 (UTC).
    pub since_ms: i64,
    /// Exclusive upper bound; None = now.
    pub until_ms: Option<i64>,
    /// Only these levels (1 Critical .. 5 Verbose); empty = all. A `CRASH_MARKERS` entry selects
    /// the crash marker event ids instead (any level).
    pub levels: Vec<u8>,
    pub max: usize,
    /// Newest first when true.
    pub newest_first: bool,
}

pub trait EventSource {
    fn query(&self, q: &Query) -> Result<Vec<Event>, String>;
    fn channel_status(&self, channel: &str) -> ChannelStatus;
}

/// The real Windows Event Log (EvtQuery / EvtNext / EvtRender). Calls may be slow: use a worker
/// thread, never the UI thread.
pub struct WinEvents;

/// Closes an Event Log handle on drop.
struct Handle(EVT_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if self.0 .0 != 0 {
            // SAFETY: the handle came from the Event Log API and is closed exactly once.
            unsafe {
                let _ = EvtClose(self.0);
            }
        }
    }
}

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_TIMEOUT: u32 = 258;
const ERROR_NO_MORE_ITEMS: u32 = 259;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const ERROR_EVT_INVALID_CHANNEL_PATH: u32 = 15000;
const ERROR_EVT_INVALID_QUERY: u32 = 15001;
const ERROR_EVT_CHANNEL_NOT_FOUND: u32 = 15007;

fn win32_code(e: &windows::core::Error) -> u32 {
    let c = e.code().0 as u32;
    // HRESULT_FROM_WIN32 keeps the code in the low 16 bits.
    if c & 0xFFFF_0000 == 0x8007_0000 {
        c & 0xFFFF
    } else {
        c
    }
}

fn plain_error(channel: &str, e: &windows::core::Error) -> String {
    match win32_code(e) {
        ERROR_ACCESS_DENIED => {
            format!("Windows did not allow reading the {channel} log without administrator rights")
        }
        ERROR_EVT_CHANNEL_NOT_FOUND | ERROR_EVT_INVALID_CHANNEL_PATH => {
            format!("The {channel} log does not exist on this PC")
        }
        ERROR_EVT_INVALID_QUERY => format!("Windows rejected the query for the {channel} log"),
        c => format!("Windows could not read the {channel} log (error {c})"),
    }
}

/// Renders one event handle to its XML text.
fn render_xml(ev: EVT_HANDLE) -> Option<String> {
    let mut used = 0u32;
    let mut props = 0u32;
    // SAFETY: size-probe call with no buffer; the API reports the needed size in bytes.
    let first = unsafe {
        EvtRender(
            None,
            ev,
            EvtRenderEventXml.0,
            0,
            None,
            &mut used,
            &mut props,
        )
    };
    if let Err(e) = first {
        if win32_code(&e) != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
    }
    let mut buf = vec![0u16; (used as usize) / 2 + 2];
    let bytes = (buf.len() * 2) as u32;
    // SAFETY: the buffer is `bytes` long and lives across the call.
    let r = unsafe {
        EvtRender(
            None,
            ev,
            EvtRenderEventXml.0,
            bytes,
            Some(buf.as_mut_ptr().cast()),
            &mut used,
            &mut props,
        )
    };
    r.ok()?;
    let chars = ((used as usize) / 2).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..chars]);
    Some(text.trim_end_matches('\0').to_string())
}

fn open_query(channel: &str, xpath: &str, newest_first: bool) -> Result<Handle, String> {
    let dir = if newest_first {
        EvtQueryReverseDirection.0
    } else {
        EvtQueryForwardDirection.0
    };
    // SAFETY: both strings outlive the call; the result is wrapped for closing.
    let h = unsafe {
        EvtQuery(
            None,
            &HSTRING::from(channel),
            &HSTRING::from(xpath),
            EvtQueryChannelPath.0 | dir,
        )
    };
    match h {
        Ok(h) => Ok(Handle(h)),
        Err(e) => Err(plain_error(channel, &e)),
    }
}

impl EventSource for WinEvents {
    fn query(&self, q: &Query) -> Result<Vec<Event>, String> {
        let mut out = Vec::new();
        if q.max == 0 {
            return Ok(out);
        }
        let results = open_query(&q.channel, &build_xpath(q), q.newest_first)?;
        let mut batch = [0isize; 64];
        'outer: loop {
            let want = (q.max - out.len()).min(batch.len());
            let mut returned = 0u32;
            // SAFETY: `batch[..want]` is valid for writes; `returned` receives the count.
            let r = unsafe { EvtNext(results.0, &mut batch[..want], u32::MAX, 0, &mut returned) };
            let got = returned as usize;
            let handles: Vec<Handle> = batch[..got.min(want)]
                .iter()
                .map(|h| Handle(EVT_HANDLE(*h)))
                .collect();
            for h in &handles {
                if let Some(xml) = render_xml(h.0) {
                    if let Some(mut ev) = parse_event_xml(&xml) {
                        if ev.channel.is_empty() {
                            ev.channel = q.channel.clone();
                        }
                        out.push(ev);
                        if out.len() >= q.max {
                            break 'outer;
                        }
                    }
                }
            }
            if let Err(e) = r {
                match win32_code(&e) {
                    ERROR_NO_MORE_ITEMS | ERROR_TIMEOUT => break,
                    _ => return Err(plain_error(&q.channel, &e)),
                }
            }
            if got == 0 {
                break;
            }
        }
        Ok(out)
    }

    fn channel_status(&self, channel: &str) -> ChannelStatus {
        let q = Query {
            channel: channel.to_string(),
            since_ms: 0,
            until_ms: None,
            levels: Vec::new(),
            max: 1,
            newest_first: false,
        };
        match self.query(&q) {
            Ok(v) => ChannelStatus {
                name: channel.to_string(),
                readable: true,
                error: None,
                oldest_ms: v.first().map(|e| e.time_ms),
            },
            Err(e) => ChannelStatus {
                name: channel.to_string(),
                readable: false,
                error: Some(e),
                oldest_ms: None,
            },
        }
    }
}

/// Localised message text of one event (EvtFormatMessage), on demand only.
pub fn message_of(event: &Event) -> Option<String> {
    if event.channel.is_empty() || event.provider.is_empty() {
        return None;
    }
    let xpath = format!("*[System[EventRecordID={}]]", event.record_id);
    let results = open_query(&event.channel, &xpath, false).ok()?;
    let mut h = [0isize; 1];
    let mut returned = 0u32;
    // SAFETY: one-element buffer, count returned in `returned`.
    unsafe { EvtNext(results.0, &mut h, 5000, 0, &mut returned) }.ok()?;
    if returned == 0 {
        return None;
    }
    let ev = Handle(EVT_HANDLE(h[0]));
    // SAFETY: provider name and null log path are valid for the call.
    let meta = unsafe {
        EvtOpenPublisherMetadata(
            None,
            &HSTRING::from(event.provider.as_str()),
            PCWSTR::null(),
            0,
            0,
        )
    }
    .ok()?;
    let meta = Handle(meta);
    let mut used = 0u32;
    // SAFETY: size probe without a buffer; `used` receives the needed length in characters.
    let first = unsafe {
        EvtFormatMessage(
            Some(meta.0),
            Some(ev.0),
            0,
            None,
            EvtFormatMessageEvent.0,
            None,
            &mut used,
        )
    };
    if let Err(e) = first {
        if win32_code(&e) != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
    }
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u16; used as usize + 1];
    let mut used2 = 0u32;
    // SAFETY: the buffer holds `buf.len()` characters.
    unsafe {
        EvtFormatMessage(
            Some(meta.0),
            Some(ev.0),
            0,
            None,
            EvtFormatMessageEvent.0,
            Some(&mut buf),
            &mut used2,
        )
    }
    .ok()?;
    let chars = (used2 as usize).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..chars]);
    let text = text.trim_end_matches('\0').trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Channels Event Watch reads, in order.
pub const CHANNELS: [&str; 2] = ["System", "Application"];

const LOAD_MAX_PER_CHANNEL: usize = 20_000;
const LOAD_MAX_MARKERS: usize = 5_000;

/// Loads System and Application since `since_ms` (all crash markers plus warnings and errors),
/// merged and sorted oldest first, plus one status per channel.
pub fn load(src: &dyn EventSource, since_ms: i64) -> (Vec<Event>, Vec<ChannelStatus>) {
    let mut all: Vec<Event> = Vec::new();
    let mut statuses = Vec::new();
    for channel in CHANNELS {
        let levels = Query {
            channel: channel.to_string(),
            since_ms,
            until_ms: None,
            levels: vec![1, 2, 3],
            max: LOAD_MAX_PER_CHANNEL,
            newest_first: true,
        };
        let markers = Query {
            levels: vec![CRASH_MARKERS],
            max: LOAD_MAX_MARKERS,
            ..levels.clone()
        };
        if let Ok(v) = src.query(&levels) {
            all.extend(v);
        }
        // The crash markers live in the System log only (Application has unrelated 1001s).
        if channel == "System" {
            if let Ok(v) = src.query(&markers) {
                all.extend(v);
            }
        }
        statuses.push(src.channel_status(channel));
    }
    all.sort_by(|a, b| (a.channel.as_str(), a.record_id).cmp(&(b.channel.as_str(), b.record_id)));
    all.dedup_by(|a, b| a.channel == b.channel && a.record_id == b.record_id);
    all.sort_by(|a, b| {
        (a.time_ms, a.channel.as_str(), a.record_id).cmp(&(
            b.time_ms,
            b.channel.as_str(),
            b.record_id,
        ))
    });
    (all, statuses)
}

/// The XPath for a query: time range, then levels or crash marker ids.
pub fn build_xpath(q: &Query) -> String {
    let mut conds: Vec<String> = Vec::new();
    let markers = q.levels.contains(&CRASH_MARKERS);
    if markers {
        conds.push(
            "((Provider[@Name='Microsoft-Windows-Kernel-Power'] and EventID=41) or (Provider[@Name='EventLog'] and EventID=6008) or (Provider[@Name='Microsoft-Windows-WER-SystemErrorReporting'] and EventID=1001))"
                .to_string(),
        );
    } else if !q.levels.is_empty() {
        let l: Vec<String> = q.levels.iter().map(|l| format!("Level={l}")).collect();
        conds.push(format!("({})", l.join(" or ")));
    }
    conds.push(format!(
        "TimeCreated[@SystemTime>='{}']",
        iso_time(q.since_ms)
    ));
    if let Some(u) = q.until_ms {
        conds.push(format!("TimeCreated[@SystemTime<'{}']", iso_time(u)));
    }
    format!("*[System[{}]]", conds.join(" and "))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// ms since 1970 as `2026-10-29T21:45:49.123Z`.
pub fn iso_time(ms: i64) -> String {
    let ms = ms.max(0);
    let days = ms / 86_400_000;
    let rem = ms % 86_400_000;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// Parses `2026-10-29T21:45:49.1234567Z` (fraction optional, up to 7 digits) to ms since 1970.
pub fn parse_iso_time(s: &str) -> Option<i64> {
    let s = s.trim().strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let m: i64 = dp.next()?.parse().ok()?;
    let d: i64 = dp.next()?.parse().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (hms, frac) = match time.split_once('.') {
        Some((a, b)) => (a, b),
        None => (time, ""),
    };
    let mut tp = hms.split(':');
    let hh: i64 = tp.next()?.parse().ok()?;
    let mm: i64 = tp.next()?.parse().ok()?;
    let ss: i64 = tp.next()?.parse().ok()?;
    if tp.next().is_some() || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    if frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut ms = 0i64;
    for (i, b) in frac.bytes().take(3).enumerate() {
        ms += i64::from(b - b'0') * [100, 10, 1][i];
    }
    Some(days_from_civil(y, m, d) * 86_400_000 + hh * 3_600_000 + mm * 60_000 + ss * 1000 + ms)
}

// ---------------------------------------------------------------------------------------------
// Event XML parsing

enum Tok<'a> {
    Open(&'a str, Vec<(&'a str, String)>),
    Close(&'a str),
    Text(&'a str),
}

fn parse_attrs(mut s: &str) -> Vec<(&str, String)> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start();
        let Some(eq) = s.find('=') else { break };
        let key = s[..eq].trim();
        let rest = s[eq + 1..].trim_start();
        let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let body = &rest[1..];
        let Some(end) = body.find(q) else { break };
        out.push((key, unescape(&body[..end])));
        s = &body[end + 1..];
    }
    out
}

fn tokenize(xml: &str) -> Vec<Tok<'_>> {
    let mut toks = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        if lt > 0 {
            toks.push(Tok::Text(&rest[..lt]));
        }
        rest = &rest[lt..];
        if rest.starts_with("<?") || rest.starts_with("<!--") {
            let end = if rest.starts_with("<?") { "?>" } else { "-->" };
            match rest.find(end) {
                Some(p) => rest = &rest[p + end.len()..],
                None => return toks,
            }
            continue;
        }
        // Find the closing '>' outside quotes.
        let mut quote: Option<char> = None;
        let mut close = None;
        for (i, c) in rest.char_indices().skip(1) {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '>' => {
                    close = Some(i);
                    break;
                }
                None => {}
            }
        }
        let Some(close) = close else { return toks };
        let inner = &rest[1..close];
        rest = &rest[close + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            toks.push(Tok::Close(name.trim()));
            continue;
        }
        let empty = inner.ends_with('/');
        let inner = inner.strip_suffix('/').unwrap_or(inner);
        let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
        let name = &inner[..name_end];
        toks.push(Tok::Open(name, parse_attrs(&inner[name_end..])));
        if empty {
            toks.push(Tok::Close(name));
        }
    }
    if !rest.is_empty() {
        toks.push(Tok::Text(rest));
    }
    toks
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let decoded = rest.find(';').filter(|e| *e <= 10).and_then(|e| {
            let ent = &rest[1..e];
            let ch = match ent {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => ent.strip_prefix('#').and_then(|n| {
                    let v = match n.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok(),
                        None => n.parse().ok(),
                    };
                    v.and_then(char::from_u32)
                }),
            };
            ch.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr<'a>(attrs: &'a [(&str, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.as_str())
}

/// Parses the XML of one event (as produced by EvtRender) into an `Event`. `None` if the record
/// id, time or provider is missing. `message` is left empty.
pub fn parse_event_xml(xml: &str) -> Option<Event> {
    let mut record_id: Option<u64> = None;
    let mut time_ms: Option<i64> = None;
    let mut provider: Option<String> = None;
    let mut channel = String::new();
    let mut id: u32 = 0;
    let mut level: u8 = 0;
    let mut data: Vec<(String, String)> = Vec::new();

    let mut in_system = false;
    let mut in_data = false;
    let mut in_user = false;
    let mut pending: Option<String> = None;
    let mut text = String::new();

    for tok in tokenize(xml) {
        match tok {
            Tok::Open(name, attrs) => {
                text.clear();
                match name {
                    "System" => in_system = true,
                    "EventData" => in_data = true,
                    "UserData" => in_user = true,
                    "Provider" if in_system => {
                        provider = attr(&attrs, "Name").map(str::to_string);
                    }
                    "TimeCreated" if in_system => {
                        time_ms = attr(&attrs, "SystemTime").and_then(parse_iso_time);
                    }
                    "Data" if in_data || in_user => {
                        pending = Some(attr(&attrs, "Name").unwrap_or("").to_string());
                    }
                    _ if in_user && !in_data => pending = Some(name.to_string()),
                    _ => {}
                }
            }
            Tok::Text(t) => text.push_str(&unescape(t)),
            Tok::Close(name) => {
                match name {
                    "System" => in_system = false,
                    "EventData" => in_data = false,
                    "UserData" => in_user = false,
                    "EventID" if in_system => id = text.trim().parse().unwrap_or(0),
                    "Level" if in_system => level = text.trim().parse().unwrap_or(0),
                    "EventRecordID" if in_system => record_id = text.trim().parse().ok(),
                    "Channel" if in_system => channel = text.trim().to_string(),
                    _ => {}
                }
                if let Some(n) = pending.take() {
                    if in_data || in_user {
                        let n = if n.is_empty() {
                            format!("Data{}", data.len() + 1)
                        } else {
                            n
                        };
                        data.push((n, std::mem::take(&mut text)));
                    }
                }
                text.clear();
            }
        }
    }
    Some(Event {
        record_id: record_id?,
        time_ms: time_ms?,
        channel,
        provider: provider?,
        id,
        level,
        data,
        message: None,
    })
}

// ---------------------------------------------------------------------------------------------
// Crash dump header

/// The first 4 KB of a kernel/minidump header: bug check code, four parameters, time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpHeader {
    pub code: u32,
    pub params: [u64; 4],
    /// ms since 1970 from the header's FILETIME; None if zero.
    pub time_ms: Option<i64>,
}

fn le_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap_or([0; 4]))
}

fn le_u64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap_or([0; 8]))
}

/// Parses a `PAGE`/`DU64` header (BugCheckCode u32 at 0x38, parameters u64 at 0x40/0x48/0x50/0x58,
/// SystemTime FILETIME at 0xFA8). `None` if the signature does not match or the slice is short.
pub fn parse_header(bytes: &[u8]) -> Option<DumpHeader> {
    if bytes.len() < 0xFB0 || &bytes[0..4] != b"PAGE" || &bytes[4..8] != b"DU64" {
        return None;
    }
    let ft = le_u64(bytes, 0xFA8);
    let time_ms = if ft == 0 {
        None
    } else {
        (ft / 10_000)
            .checked_sub(11_644_473_600_000)
            .map(|v| v as i64)
    };
    Some(DumpHeader {
        code: le_u32(bytes, 0x38),
        params: [
            le_u64(bytes, 0x40),
            le_u64(bytes, 0x48),
            le_u64(bytes, 0x50),
            le_u64(bytes, 0x58),
        ],
        time_ms,
    })
}

/// Reads the first 0x1000 bytes of a dump file and parses them (read-only; missing or unreadable
/// files are an `Err` with a plain reason).
pub fn read_dump_header(path: &str) -> Result<DumpHeader, String> {
    let map = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => "The dump file is missing".to_string(),
        std::io::ErrorKind::PermissionDenied => {
            "Windows did not allow reading the dump file".to_string()
        }
        _ => "Windows could not read the dump file".to_string(),
    };
    let file = std::fs::File::open(path).map_err(map)?;
    let mut buf = Vec::with_capacity(0x1000);
    file.take(0x1000).read_to_end(&mut buf).map_err(map)?;
    parse_header(&buf).ok_or_else(|| "This is not a full Windows crash dump".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KP41: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-Kernel-Power' Guid='{331C3B3A-2005-44C2-AC5E-77220C37D6B4}'/><EventID>41</EventID><Version>10</Version><Level>1</Level><Task>63</Task><Opcode>0</Opcode><Keywords>0x8000400000000002</Keywords><TimeCreated SystemTime='2026-10-29T21:45:49.1234567Z'/><EventRecordID>41234</EventRecordID><Correlation/><Execution ProcessID='4' ThreadID='8'/><Channel>System</Channel><Computer>PC</Computer><Security UserID='S-1-5-18'/></System><EventData><Data Name='BugcheckCode'>313</Data><Data Name='BugcheckParameter1'>0x1</Data><Data Name='SleepInProgress'>0</Data><Data Name='PowerButtonTimestamp'>133900000000000000</Data></EventData></Event>"#;

    const WER1001: &str = r#"<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event"><System><Provider Name="Microsoft-Windows-WER-SystemErrorReporting" Guid="{ABCE23E7-DE45-4366-8631-84FA6C525952}" EventSourceName="BugCheck"/><EventID Qualifiers="16384">1001</EventID><Level>2</Level><TimeCreated SystemTime="2026-10-29T21:46:10.0000000Z"/><EventRecordID>41240</EventRecordID><Channel>System</Channel></System><EventData><Data>0x00000139 (0x0000000000000003, 0xffffa000, 0xffffa100, 0x0000000000000000)</Data><Data>C:\Windows\MEMORY.DMP</Data><Data>a1b2 &amp; &lt;c&gt; &quot;q&quot; &apos;s&apos;</Data></EventData></Event>"#;

    const DISK153: &str = r#"<Event xmlns='x'><System><Provider Name='disk'/><EventID Qualifiers='32772'>153</EventID><Level>3</Level><TimeCreated SystemTime='2026-10-28T08:00:00.5Z'/><EventRecordID>9</EventRecordID><Channel>System</Channel></System><EventData><Data>\Device\Harddisk1\DR1</Data><Data Name='x'/><Binary>0000</Binary></EventData></Event>"#;

    const WHEA17: &str = r#"<Event xmlns='x'><System><Provider Name='Microsoft-Windows-WHEA-Logger' Guid='{C26C4F3C}'/><EventID>17</EventID><Level>3</Level><TimeCreated SystemTime='2026-10-27T01:02:03.999Z'/><EventRecordID>77</EventRecordID><Channel>System</Channel></System><UserData><ErrorRecord xmlns='y'><Severity>Corrected</Severity><Source>PCI Express</Source></ErrorRecord></UserData></Event>"#;

    #[test]
    fn parses_kernel_power_41() {
        let e = parse_event_xml(KP41).unwrap();
        assert_eq!(e.record_id, 41234);
        assert_eq!(e.provider, "Microsoft-Windows-Kernel-Power");
        assert_eq!(e.id, 41);
        assert_eq!(e.level, 1);
        assert_eq!(e.channel, "System");
        assert_eq!(
            e.time_ms,
            parse_iso_time("2026-10-29T21:45:49.123Z").unwrap()
        );
        assert_eq!(e.value("BugcheckCode"), Some("313"));
        assert_eq!(e.value("PowerButtonTimestamp"), Some("133900000000000000"));
        assert_eq!(e.data.len(), 4);
        assert_eq!(e.data[0].0, "BugcheckCode");
    }

    #[test]
    fn parses_wer_1001_with_qualifiers_unnamed_and_escapes() {
        let e = parse_event_xml(WER1001).unwrap();
        assert_eq!(e.id, 1001);
        assert_eq!(e.data[0].0, "Data1");
        assert!(e.data[0].1.starts_with("0x00000139 ("));
        assert_eq!(
            e.data[1],
            ("Data2".to_string(), r"C:\Windows\MEMORY.DMP".to_string())
        );
        assert_eq!(e.data[2].1, "a1b2 & <c> \"q\" 's'");
    }

    #[test]
    fn parses_disk_153_with_empty_and_binary() {
        let e = parse_event_xml(DISK153).unwrap();
        assert_eq!(e.provider, "disk");
        assert_eq!(e.id, 153);
        assert_eq!(e.level, 3);
        assert_eq!(
            e.data[0],
            ("Data1".to_string(), r"\Device\Harddisk1\DR1".to_string())
        );
        assert_eq!(e.data[1], ("x".to_string(), String::new()));
        assert_eq!(e.data.len(), 2);
        assert_eq!(e.time_ms % 1000, 500);
    }

    #[test]
    fn parses_whea_17_user_data() {
        let e = parse_event_xml(WHEA17).unwrap();
        assert_eq!(e.id, 17);
        assert_eq!(e.value("Severity"), Some("Corrected"));
        assert_eq!(e.value("Source"), Some("PCI Express"));
        assert_eq!(e.data.len(), 2);
    }

    #[test]
    fn rejects_incomplete_xml() {
        assert!(parse_event_xml("").is_none());
        assert!(parse_event_xml("<Event><System></System></Event>").is_none());
        assert!(parse_event_xml("<Event><System><Provider Name='a'/>").is_none());
    }

    #[test]
    fn iso_time_round_trips() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(1_000), "1970-01-01T00:00:01.000Z");
        let ms = parse_iso_time("2026-10-29T21:45:49.1234567Z").unwrap();
        assert_eq!(iso_time(ms), "2026-10-29T21:45:49.123Z");
        assert_eq!(parse_iso_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso_time("2024-02-29T23:59:59.9Z")
                .map(iso_time)
                .as_deref(),
            Some("2024-02-29T23:59:59.900Z")
        );
        assert_eq!(parse_iso_time("nonsense"), None);
        assert_eq!(parse_iso_time("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_iso_time("2026-10-29T21:45:49.12x4Z"), None);
    }

    #[test]
    fn xpath_building() {
        let mut q = Query {
            channel: "System".into(),
            since_ms: 0,
            until_ms: None,
            levels: vec![],
            max: 10,
            newest_first: true,
        };
        assert_eq!(
            build_xpath(&q),
            "*[System[TimeCreated[@SystemTime>='1970-01-01T00:00:00.000Z']]]"
        );
        q.levels = vec![2, 3];
        q.until_ms = Some(1000);
        assert_eq!(
            build_xpath(&q),
            "*[System[(Level=2 or Level=3) and TimeCreated[@SystemTime>='1970-01-01T00:00:00.000Z'] and TimeCreated[@SystemTime<'1970-01-01T00:00:01.000Z']]]"
        );
        q.levels = vec![CRASH_MARKERS];
        q.until_ms = None;
        assert!(build_xpath(&q).contains(
            "Provider[@Name='Microsoft-Windows-WER-SystemErrorReporting'] and EventID=1001"
        ));
        assert!(!build_xpath(&q).contains("Level="));
    }

    struct FakeSource {
        events: Vec<Event>,
    }

    impl EventSource for FakeSource {
        fn query(&self, q: &Query) -> Result<Vec<Event>, String> {
            if q.channel == "Application" && self.events.iter().all(|e| e.channel != "Application")
            {
                return Err("nope".into());
            }
            let mut v: Vec<Event> = self
                .events
                .iter()
                .filter(|e| e.channel == q.channel && e.time_ms >= q.since_ms)
                .filter(|e| {
                    if q.levels.contains(&CRASH_MARKERS) {
                        [41, 6008, 1001].contains(&e.id)
                    } else {
                        q.levels.contains(&e.level)
                    }
                })
                .cloned()
                .collect();
            v.sort_by_key(|e| std::cmp::Reverse(e.time_ms));
            Ok(v)
        }
        fn channel_status(&self, channel: &str) -> ChannelStatus {
            ChannelStatus {
                name: channel.to_string(),
                readable: true,
                error: None,
                oldest_ms: None,
            }
        }
    }

    fn ev(channel: &str, record_id: u64, time_ms: i64, id: u32, level: u8) -> Event {
        Event {
            record_id,
            time_ms,
            channel: channel.into(),
            provider: "p".into(),
            id,
            level,
            data: vec![],
            message: None,
        }
    }

    #[test]
    fn load_merges_dedupes_and_sorts() {
        let src = FakeSource {
            events: vec![
                ev("System", 3, 300, 41, 2),     // in both queries
                ev("System", 1, 100, 7, 3),      // levels only
                ev("System", 2, 200, 1001, 4),   // marker only
                ev("System", 9, 50, 7, 4),       // neither (info, not marker)
                ev("System", 8, 10, 7, 1),       // before since
                ev("Application", 3, 250, 5, 2), // same record id, other channel
            ],
        };
        let (events, statuses) = load(&src, 100);
        let keys: Vec<(&str, u64)> = events
            .iter()
            .map(|e| (e.channel.as_str(), e.record_id))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("System", 1),
                ("System", 2),
                ("Application", 3),
                ("System", 3)
            ]
        );
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].name, "System");
        assert_eq!(statuses[1].name, "Application");
    }

    #[test]
    fn load_survives_failing_channel() {
        let src = FakeSource {
            events: vec![ev("System", 1, 100, 41, 1)],
        };
        let (events, statuses) = load(&src, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(statuses.len(), 2);
    }

    fn header_bytes() -> Vec<u8> {
        let mut b = vec![0u8; 0x1000];
        b[0..4].copy_from_slice(b"PAGE");
        b[4..8].copy_from_slice(b"DU64");
        b[0x38..0x3C].copy_from_slice(&0x139u32.to_le_bytes());
        for (i, p) in [3u64, 0xffff_a000_0000_0000, 0x10, 0].iter().enumerate() {
            b[0x40 + i * 8..0x48 + i * 8].copy_from_slice(&p.to_le_bytes());
        }
        // 2026-10-29T21:45:49Z as FILETIME.
        let ms = parse_iso_time("2026-10-29T21:45:49Z").unwrap();
        let ft = (ms as u64 + 11_644_473_600_000) * 10_000;
        b[0xFA8..0xFB0].copy_from_slice(&ft.to_le_bytes());
        b
    }

    #[test]
    fn parses_synthetic_header() {
        let h = parse_header(&header_bytes()).unwrap();
        assert_eq!(h.code, 0x139);
        assert_eq!(h.params, [3, 0xffff_a000_0000_0000, 0x10, 0]);
        assert_eq!(h.time_ms, parse_iso_time("2026-10-29T21:45:49Z"));
    }

    #[test]
    fn header_zero_time_is_none() {
        let mut b = header_bytes();
        b[0xFA8..0xFB0].fill(0);
        assert_eq!(parse_header(&b).unwrap().time_ms, None);
    }

    #[test]
    fn header_rejects_short_and_wrong_signature() {
        assert!(parse_header(&header_bytes()[..0x100]).is_none());
        assert!(parse_header(&[]).is_none());
        let mut b = header_bytes();
        b[0..4].copy_from_slice(b"MDMP");
        assert!(parse_header(&b).is_none());
        let mut b = header_bytes();
        b[4..8].copy_from_slice(b"DU32");
        assert!(parse_header(&b).is_none());
    }

    #[test]
    fn missing_dump_file_is_plain_error() {
        assert_eq!(
            read_dump_header(r"C:\definitely\not\here\MEMORY.DMP").unwrap_err(),
            "The dump file is missing"
        );
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    #[test]
    #[ignore]
    fn real_system_log_smoke() {
        let q = Query {
            channel: "System".into(),
            since_ms: now_ms() - 7 * 86_400_000,
            until_ms: None,
            levels: vec![],
            max: 50,
            newest_first: true,
        };
        let r = WinEvents.query(&q);
        let v = r.expect("System log should be readable");
        println!("count = {}", v.len());
        if let Some(e) = v.first() {
            println!(
                "first: {} {} {} data={}",
                e.provider,
                e.id,
                iso_time(e.time_ms),
                e.data.len()
            );
        }
        for c in CHANNELS {
            println!("{:?}", WinEvents.channel_status(c));
        }
    }

    #[test]
    #[ignore]
    fn real_crash_markers_smoke() {
        let q = Query {
            channel: "System".into(),
            since_ms: now_ms() - 90 * 86_400_000,
            until_ms: None,
            levels: vec![CRASH_MARKERS],
            max: 20,
            newest_first: true,
        };
        let v = WinEvents.query(&q).expect("System log should be readable");
        println!("markers = {}", v.len());
        for e in v
            .iter()
            .filter(|e| (e.provider.contains("Kernel-Power") && e.id == 41) || e.id == 1001)
        {
            println!("{} {} {}", e.provider, e.id, iso_time(e.time_ms));
            for (k, val) in &e.data {
                println!("    {k} = {val}");
            }
        }
    }
}
