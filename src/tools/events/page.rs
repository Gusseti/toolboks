//! The Event Watch pages (Live, Crashes, Summary) and the pure helpers behind them.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Vec2};
use egui_extras::{Column, TableBuilder};

use super::analysis;
use super::bugchecks::{self, Cause};
use super::catalog;
use super::model::{
    Category, ChannelStatus, Crash, CrashKind, DigestLine, Event, Group, Info, Severity,
};
use super::source::{self, DumpHeader, EventSource, Query, WinEvents};
use crate::ui::{self as look, AMBER, GAP, MUTED, RED};

const DAY_MS: i64 = 86_400_000;
const LOOKBACK_MS: i64 = 30 * DAY_MS;
const POLL_EVERY: Duration = Duration::from_secs(2);
const ROW_H: f32 = 22.0;
const INSPECTOR_W: f32 = 210.0;
const INSPECTOR_PAD: f32 = 8.0;
const PILL_W: f32 = 104.0;
const HARMLESS: Color32 = Color32::from_rgb(0x6f, 0xa8, 0x85);

type Key = (String, u64);

// ---------------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------------

/// `2026-10-30 14:03:22` in UTC.
fn ms_to_text(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn sev_index(s: Severity) -> usize {
    match s {
        Severity::NeedsAttention => 0,
        Severity::WorthWatching => 1,
        Severity::Harmless => 2,
        Severity::Info => 3,
    }
}

fn sev_label(s: Severity) -> &'static str {
    match s {
        Severity::NeedsAttention => "Needs attention",
        Severity::WorthWatching => "Worth watching",
        Severity::Harmless => "Probably harmless",
        Severity::Info => "Info",
    }
}

fn sev_color(s: Severity) -> Color32 {
    match s {
        Severity::NeedsAttention => RED,
        Severity::WorthWatching => AMBER,
        Severity::Harmless => HARMLESS,
        Severity::Info => MUTED,
    }
}

fn category_text(c: Category) -> &'static str {
    match c {
        Category::Disk => "Disk",
        Category::Graphics => "Graphics",
        Category::Hardware => "Hardware",
        Category::Memory => "Memory",
        Category::Power => "Power",
        Category::Filesystem => "Files",
        Category::Driver => "Driver",
        Category::Software => "Software",
        Category::System => "System",
        Category::Other => "Other",
    }
}

fn level_text(level: u8) -> String {
    match level {
        0 => "Always logged".to_string(),
        1 => "Critical".to_string(),
        2 => "Error".to_string(),
        3 => "Warning".to_string(),
        4 => "Information".to_string(),
        5 => "Verbose".to_string(),
        n => format!("Level {n}"),
    }
}

/// Is a row of this severity shown? `show` is in the order of the four severities.
fn severity_shown(s: Severity, show: &[bool; 4]) -> bool {
    show[sev_index(s)]
}

/// Every word of `filter` must appear in the provider, the id, the headline or the subject.
fn matches_filter(
    filter: &str,
    provider: &str,
    id: u32,
    headline: &str,
    subject: Option<&str>,
) -> bool {
    let hay = format!(
        "{} {} {} {}",
        provider,
        id,
        headline,
        subject.unwrap_or_default()
    )
    .to_lowercase();
    filter
        .split_whitespace()
        .all(|w| hay.contains(&w.to_lowercase()))
}

/// "x143 Disk 0: headline"; a single event just shows the headline.
fn row_headline(headline: &str, count: usize, subject: Option<&str>) -> String {
    let mut out = String::new();
    if count > 1 {
        out.push_str(&format!("x{count} "));
    }
    if let Some(s) = subject.filter(|s| !s.is_empty()) {
        out.push_str(s);
        out.push_str(": ");
    }
    out.push_str(headline);
    out
}

/// What the table and the inspector work on: one event, or a group of repeated ones.
#[derive(Clone, Debug)]
enum Sel {
    Event(Event, Info),
    Group(Group),
}

impl Sel {
    fn event(&self) -> &Event {
        match self {
            Sel::Event(e, _) => e,
            Sel::Group(g) => &g.sample,
        }
    }
    fn info(&self) -> &Info {
        match self {
            Sel::Event(_, i) => i,
            Sel::Group(g) => &g.info,
        }
    }
    fn key(&self) -> (&str, u64, bool) {
        let e = self.event();
        (&e.channel, e.record_id, matches!(self, Sel::Group(_)))
    }
}

#[derive(Clone, Debug)]
struct Row {
    time_ms: i64,
    severity: Severity,
    category: Category,
    headline: String,
    provider: String,
    id: u32,
    sel: Sel,
}

/// Rows of the Live table, newest first, after the severity and text filters.
fn build_rows(
    events: &[Event],
    groups: &[Group],
    grouped: bool,
    filter: &str,
    show: &[bool; 4],
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    if grouped {
        for g in groups {
            if !severity_shown(g.info.severity, show) {
                continue;
            }
            let headline = row_headline(g.info.headline, g.count, g.subject.as_deref());
            if !matches_filter(filter, &g.provider, g.id, &headline, g.subject.as_deref()) {
                continue;
            }
            rows.push(Row {
                time_ms: g.last_ms,
                severity: g.info.severity,
                category: g.info.category,
                headline,
                provider: g.provider.clone(),
                id: g.id,
                sel: Sel::Group(g.clone()),
            });
        }
    } else {
        for e in events {
            let info = catalog::classify(e);
            if !severity_shown(info.severity, show) {
                continue;
            }
            if !matches_filter(filter, &e.provider, e.id, info.headline, None) {
                continue;
            }
            rows.push(Row {
                time_ms: e.time_ms,
                severity: info.severity,
                category: info.category,
                headline: info.headline.to_string(),
                provider: e.provider.clone(),
                id: e.id,
                sel: Sel::Event(e.clone(), info),
            });
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.time_ms));
    rows
}

/// "-3 min 20 s" for an event `event_ms` that happened before `stopped_ms`.
fn minutes_before(stopped_ms: i64, event_ms: i64) -> String {
    let secs = ((stopped_ms - event_ms) / 1000).max(0);
    let (m, s) = (secs / 60, secs % 60);
    match (m, s) {
        (0, s) => format!("-{s} s"),
        (m, 0) => format!("-{m} min"),
        (m, s) => format!("-{m} min {s} s"),
    }
}

fn crash_title(kind: &CrashKind) -> String {
    match kind {
        CrashKind::BlueScreen { code, .. } => {
            let name = bugchecks::lookup(*code).map(|b| b.name).unwrap_or_default();
            let text = format!("Bluescreen 0x{code:08X} {name}");
            text.trim_end().to_string()
        }
        CrashKind::ForcedRestart => "Forced restart (power button held)".to_string(),
        CrashKind::PowerLossOrHang => "Power loss or freeze".to_string(),
        CrashKind::Unknown => "Unknown restart".to_string(),
    }
}

fn cause_text(c: Cause) -> &'static str {
    match c {
        Cause::Hardware => "Often a hardware problem, but not always.",
        Cause::Driver => "Usually a driver problem, but other causes are possible.",
        Cause::Software => "Often caused by software, though not always.",
        Cause::Unknown => "The cause is hard to tell from the code alone.",
    }
}

/// The plain sentence under the crash title.
fn crash_detail(kind: &CrashKind) -> Option<String> {
    match kind {
        CrashKind::BlueScreen { code, .. } => Some(match bugchecks::lookup(*code) {
            Some(b) => format!("{} {}", b.sentence, cause_text(b.cause)),
            None => "Windows has no plain description for this stop code yet.".to_string(),
        }),
        CrashKind::ForcedRestart => Some(
            "The power button was held until the PC switched off, so Windows could not write a report."
                .to_string(),
        ),
        CrashKind::PowerLossOrHang => Some(
            "Windows lost power or froze without writing a report. A power cut, a failing power supply, overheating or a hard freeze can all look like this."
                .to_string(),
        ),
        CrashKind::Unknown => None,
    }
}

fn dump_line(kind: &CrashKind, path: Option<&str>, exists: bool) -> Option<String> {
    match path {
        Some(p) if exists => Some(format!("Windows recorded a dump at {p}")),
        Some(p) => Some(format!(
            "Windows recorded a dump at {p}. The dump file is missing."
        )),
        None if matches!(kind, CrashKind::BlueScreen { .. }) => {
            Some("Windows did not record a dump for this crash.".to_string())
        }
        None => None,
    }
}

fn dump_header_text(h: &DumpHeader, kind: &CrashKind) -> String {
    let mut t = format!(
        "Dump header: stop code 0x{:08X}, parameters {:#x} {:#x} {:#x} {:#x}",
        h.code, h.params[0], h.params[1], h.params[2], h.params[3]
    );
    if let Some(ms) = h.time_ms {
        t.push_str(&format!(", written {} UTC", ms_to_text(ms)));
    }
    t.push('.');
    if let CrashKind::BlueScreen { code, .. } = kind {
        t.push_str(if *code == h.code {
            " This matches the event."
        } else {
            " This does not match the event."
        });
    }
    t
}

/// "The log covers from ..." from the date every readable channel reaches back to (never earlier
/// than what was asked for).
fn coverage_line(statuses: &[ChannelStatus], since_ms: i64) -> Option<String> {
    let oldest = statuses
        .iter()
        .filter(|s| s.readable)
        .filter_map(|s| s.oldest_ms)
        .max()?;
    let text = ms_to_text(oldest.max(since_ms));
    Some(format!("The log covers from {} UTC.", &text[..16]))
}

fn unreadable_warnings(statuses: &[ChannelStatus]) -> Vec<String> {
    statuses
        .iter()
        .filter(|s| !s.readable)
        .map(|s| format!("Windows did not let toolboks read the {} log.", s.name))
        .collect()
}

fn no_crashes_text(statuses: &[ChannelStatus]) -> String {
    if statuses.is_empty() || statuses.iter().any(|s| !s.readable) {
        "No crashes found in the part of the log toolboks could read.".to_string()
    } else {
        "No crashes found in the last 30 days.".to_string()
    }
}

fn count_text(rows: usize, events: usize, grouped: bool) -> String {
    if grouped {
        format!("{rows} groups, {events} events")
    } else {
        format!("{rows} events")
    }
}

/// Adds `new` events that are not known yet (same channel and record id); keeps the list sorted
/// oldest first. Returns how many were added.
fn merge_events(events: &mut Vec<Event>, new: Vec<Event>) -> usize {
    let mut seen: HashSet<(String, u64)> = events
        .iter()
        .map(|e| (e.channel.clone(), e.record_id))
        .collect();
    let before = events.len();
    for e in new {
        if seen.insert((e.channel.clone(), e.record_id)) {
            events.push(e);
        }
    }
    let added = events.len() - before;
    if added > 0 {
        events.sort_by_key(|e| e.time_ms);
    }
    added
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}...")
    }
}

// --- Redaction ---------------------------------------------------------------------------------

/// Replaces the Windows user name and profile paths (`C:\Users\<name>`) with `<user>`.
fn redact(text: &str) -> String {
    redact_for(text, std::env::var("USERNAME").ok().as_deref())
}

fn starts_with_ci(chars: &[char], at: usize, needle: &[char]) -> bool {
    chars.len() >= at + needle.len()
        && chars[at..at + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
}

fn redact_for(text: &str, user: Option<&str>) -> String {
    let user: Option<Vec<char>> = user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| u.chars().collect());
    let chars: Vec<char> = text.chars().collect();
    let users_dir: Vec<char> = ":\\users\\".chars().collect();
    let is_name_end = |c: char| {
        matches!(
            c,
            '\\' | '/' | '"' | '\'' | '<' | '>' | '|' | '*' | '?' | ',' | ';' | '\n' | '\r' | '\t'
        )
    };

    // Pass 1: profile paths.
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() && starts_with_ci(&chars, i + 1, &users_dir) {
            let start = i + 1 + users_dir.len();
            let mut end = start;
            let known = user.as_ref().filter(|u| {
                starts_with_ci(&chars, start, u)
                    && chars
                        .get(start + u.len())
                        .is_none_or(|c| is_name_end(*c) || c.is_whitespace())
            });
            if let Some(u) = known {
                end = start + u.len();
            } else {
                while end < chars.len() && !is_name_end(chars[end]) && !chars[end].is_whitespace() {
                    end += 1;
                }
            }
            let name: String = chars[start..end].iter().collect();
            let shared = ["public", "default", "all"]
                .iter()
                .any(|s| name.eq_ignore_ascii_case(s));
            if end > start && !shared {
                out.extend("<user>".chars());
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    // Pass 2: the bare user name as a whole word.
    let Some(u) = user.filter(|u| u.len() >= 3) else {
        return out.into_iter().collect();
    };
    let chars = out;
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut res = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let before_ok = i == 0 || !word(chars[i - 1]);
        let after_ok = chars.get(i + u.len()).is_none_or(|c| !word(*c));
        if before_ok && after_ok && starts_with_ci(&chars, i, &u) {
            res.push_str("<user>");
            i += u.len();
        } else {
            res.push(chars[i]);
            i += 1;
        }
    }
    res
}

// --- Reports -----------------------------------------------------------------------------------

fn event_lines(e: &Event, out: &mut String) {
    out.push_str(&format!(
        "Provider: {}\nEvent ID: {}\nLevel: {}\nChannel: {}\nTime (UTC): {}\n",
        e.provider,
        e.id,
        level_text(e.level),
        e.channel,
        ms_to_text(e.time_ms)
    ));
    if !e.data.is_empty() {
        out.push_str("Event data:\n");
        for (k, v) in &e.data {
            out.push_str(&format!("  {k}: {}\n", shorten(v, 500)));
        }
    }
}

fn event_report(version: &str, sel: &Sel, message: Option<&str>, keep_names: bool) -> String {
    let info = sel.info();
    let mut t = format!(
        "toolboks {version} - Event Watch report\n\n{}\nSeverity: {}\n",
        info.headline,
        sev_label(info.severity)
    );
    if let Sel::Group(g) = sel {
        t.push_str(&format!(
            "Repeated {} times between {} and {} UTC\n",
            g.count,
            ms_to_text(g.first_ms),
            ms_to_text(g.last_ms)
        ));
        if let Some(s) = &g.subject {
            t.push_str(&format!("About: {s}\n"));
        }
    }
    if !info.hint.is_empty() {
        t.push_str(&format!("Hint: {}\n", info.hint));
    }
    t.push('\n');
    event_lines(sel.event(), &mut t);
    if let Some(m) = message {
        t.push_str(&format!("\nWindows' own message:\n{m}\n"));
    }
    if keep_names {
        t
    } else {
        redact(&t)
    }
}

fn crash_report(
    version: &str,
    crash: &Crash,
    dump_exists: bool,
    header: Option<&Result<DumpHeader, String>>,
    keep_names: bool,
) -> String {
    let mut t = format!(
        "toolboks {version} - Event Watch crash report\n\n{}\nStopped (best estimate): {} UTC\nNoticed at next start: {} UTC\n",
        crash_title(&crash.kind),
        ms_to_text(crash.stopped_ms),
        ms_to_text(crash.noticed_ms)
    );
    if let Some(d) = crash_detail(&crash.kind) {
        t.push_str(&format!("{d}\n"));
    }
    if let Some(d) = dump_line(&crash.kind, crash.dump_path.as_deref(), dump_exists) {
        t.push_str(&format!("{d}\n"));
    }
    if let Some(Ok(h)) = header {
        t.push_str(&format!("{}\n", dump_header_text(h, &crash.kind)));
    }
    t.push_str("\nEvents in the 10 minutes before (oldest first):\n");
    if crash.before.is_empty() {
        t.push_str("  none logged\n");
    }
    for e in &crash.before {
        let info = catalog::classify(e);
        t.push_str(&format!(
            "  {}  [{}]  {}  ({} {})\n",
            minutes_before(crash.stopped_ms, e.time_ms),
            sev_label(info.severity),
            info.headline,
            e.provider,
            e.id
        ));
    }
    if keep_names {
        t
    } else {
        redact(&t)
    }
}

// ---------------------------------------------------------------------------------------------
// State and worker threads
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Analysis {
    groups: Vec<Group>,
    crashes: Vec<Crash>,
    digest: Vec<DigestLine>,
}

fn analyze(events: &[Event]) -> Analysis {
    let groups = analysis::group(events);
    let crashes = analysis::detect_crashes(events, analysis::DEFAULT_WINDOW_MS);
    let digest = analysis::digest(&groups, &crashes);
    Analysis {
        groups,
        crashes,
        digest,
    }
}

enum Msg {
    Loaded {
        events: Vec<Event>,
        statuses: Vec<ChannelStatus>,
        since_ms: i64,
        analysis: Analysis,
    },
    Polled(Vec<Event>),
    Analyzed(Analysis, usize),
    Message(Key, Option<String>),
    Dump(i64, Result<DumpHeader, String>),
}

enum CrashAction {
    Select(Event),
    Copy(String),
    ReadDump(i64, String),
}

pub struct Page {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    started: bool,
    loading: bool,
    loaded: bool,
    poll_busy: bool,
    last_poll: Instant,
    analysis_busy: bool,

    events: Vec<Event>,
    statuses: Vec<ChannelStatus>,
    since_ms: i64,
    analysis: Analysis,

    filter: String,
    show: [bool; 4],
    grouped: bool,
    paused: bool,
    rows: Vec<Row>,
    rows_dirty: bool,
    sel: Option<Sel>,

    keep_names: bool,
    messages: HashMap<Key, Option<String>>,
    message_pending: HashSet<Key>,
    dump_exists: HashMap<i64, bool>,
    dump_headers: HashMap<i64, Result<DumpHeader, String>>,
    dump_pending: HashSet<i64>,
}

impl Page {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            started: false,
            loading: false,
            loaded: false,
            poll_busy: false,
            last_poll: Instant::now(),
            analysis_busy: false,
            events: Vec::new(),
            statuses: Vec::new(),
            since_ms: 0,
            analysis: Analysis::default(),
            filter: String::new(),
            show: [true, true, false, false],
            grouped: true,
            paused: false,
            rows: Vec::new(),
            rows_dirty: true,
            sel: None,
            keep_names: false,
            messages: HashMap::new(),
            message_pending: HashSet::new(),
            dump_exists: HashMap::new(),
            dump_headers: HashMap::new(),
            dump_pending: HashSet::new(),
        }
    }

    // --- worker threads ---

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let since_ms = now_ms() - LOOKBACK_MS;
            let (events, statuses) = source::load(&WinEvents, since_ms);
            let analysis = analyze(&events);
            let _ = tx.send(Msg::Loaded {
                events,
                statuses,
                since_ms,
                analysis,
            });
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_busy = true;
        self.last_poll = Instant::now();
        let newest = self.events.iter().map(|e| e.time_ms).max().unwrap_or(0);
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut found = Vec::new();
            for channel in source::CHANNELS {
                let q = Query {
                    channel: channel.to_string(),
                    since_ms: newest,
                    until_ms: None,
                    levels: vec![1, 2, 3],
                    max: 200,
                    newest_first: true,
                };
                if let Ok(mut events) = WinEvents.query(&q) {
                    found.append(&mut events);
                }
            }
            let _ = tx.send(Msg::Polled(found));
            ctx.request_repaint();
        });
    }

    fn reanalyze(&mut self, ctx: &egui::Context) {
        if self.analysis_busy {
            return;
        }
        self.analysis_busy = true;
        let events = self.events.clone();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let n = events.len();
            let _ = tx.send(Msg::Analyzed(analyze(&events), n));
            ctx.request_repaint();
        });
    }

    fn ask_message(&mut self, event: &Event, ctx: &egui::Context) {
        let key = (event.channel.clone(), event.record_id);
        if !self.message_pending.insert(key.clone()) {
            return;
        }
        let (tx, ctx, event) = (self.tx.clone(), ctx.clone(), event.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Message(key, source::message_of(&event)));
            ctx.request_repaint();
        });
    }

    fn read_dump(&mut self, id: i64, path: String, ctx: &egui::Context) {
        if !self.dump_pending.insert(id) {
            return;
        }
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Dump(id, source::read_dump_header(&path)));
            ctx.request_repaint();
        });
    }

    fn data_changed(&mut self) {
        if !self.paused {
            self.rows_dirty = true;
        }
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded {
                    events,
                    statuses,
                    since_ms,
                    analysis,
                } => {
                    self.events = events;
                    self.statuses = statuses;
                    self.since_ms = since_ms;
                    self.analysis = analysis;
                    self.dump_exists.clear();
                    self.loading = false;
                    self.loaded = true;
                    self.last_poll = Instant::now();
                    self.data_changed();
                }
                Msg::Polled(new) => {
                    self.poll_busy = false;
                    if !self.loading && merge_events(&mut self.events, new) > 0 {
                        self.reanalyze(ctx);
                    }
                }
                Msg::Analyzed(analysis, n) => {
                    self.analysis_busy = false;
                    self.analysis = analysis;
                    self.data_changed();
                    if n != self.events.len() {
                        self.reanalyze(ctx);
                    }
                }
                Msg::Message(key, text) => {
                    self.message_pending.remove(&key);
                    self.messages.insert(key, text);
                }
                Msg::Dump(id, result) => {
                    self.dump_pending.remove(&id);
                    self.dump_headers.insert(id, result);
                }
            }
        }
    }

    // --- drawing ---

    pub fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        if !self.started {
            self.started = true;
            self.refresh(&ctx);
        }
        // The light poll only runs while the Live page is on screen: this is only called then.
        if page == 0 && self.loaded && !self.loading && !self.poll_busy {
            if self.last_poll.elapsed() >= POLL_EVERY {
                self.poll(&ctx);
            }
            ctx.request_repaint_after(POLL_EVERY);
        }
        if page == 0 && self.rows_dirty {
            self.rows_dirty = false;
            self.rows = build_rows(
                &self.events,
                &self.analysis.groups,
                self.grouped,
                &self.filter,
                &self.show,
            );
        }

        if page < 2 {
            let frame = egui::Frame::NONE
                .fill(ui.visuals().extreme_bg_color)
                .inner_margin(INSPECTOR_PAD);
            let panel = egui::Panel::right("events_inspector")
                .resizable(false)
                .exact_size(INSPECTOR_W)
                .frame(frame)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.inspector(ui, &ctx));
                });
            let rect = panel.response.rect;
            ui.painter().vline(
                rect.left(),
                rect.y_range(),
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
        }

        let side = egui::Frame::NONE.inner_margin(look::CARD_PAD);
        egui::CentralPanel::default().frame(side).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            self.header(ui, &ctx);
            match page {
                0 => self.live_page(ui),
                1 => self.crashes_page(ui, &ctx),
                _ => self.summary_page(ui),
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if self.loading {
                ui.spinner();
                ui.label("Reading Windows event logs...");
            } else if ui.button("Refresh").clicked() {
                self.refresh(ctx);
            }
            if let Some(line) = coverage_line(&self.statuses, self.since_ms) {
                ui.label(RichText::new(line).small().weak());
            }
        });
        for w in unreadable_warnings(&self.statuses) {
            ui.label(RichText::new(w).small().color(AMBER));
        }
    }

    fn live_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Filter by provider, id or text")
                        .desired_width(220.0),
                )
                .changed();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let events: usize = self
                    .rows
                    .iter()
                    .map(|r| match &r.sel {
                        Sel::Group(g) => g.count,
                        Sel::Event(..) => 1,
                    })
                    .sum();
                ui.label(
                    RichText::new(count_text(self.rows.len(), events, self.grouped))
                        .small()
                        .weak(),
                );
            });
        });
        ui.horizontal_wrapped(|ui| {
            for (i, s) in [
                Severity::NeedsAttention,
                Severity::WorthWatching,
                Severity::Harmless,
                Severity::Info,
            ]
            .into_iter()
            .enumerate()
            {
                changed |= ui.checkbox(&mut self.show[i], sev_label(s)).changed();
            }
            changed |= ui
                .checkbox(&mut self.grouped, "Group repeated events")
                .changed();
            ui.add_space(GAP);
            if look::toggle_switch(ui, &mut self.paused).changed() && !self.paused {
                changed = true;
            }
            ui.label("Pause");
        });
        if changed {
            self.rows_dirty = true;
        }
        ui.add_space(2.0);

        if self.rows.is_empty() {
            if self.loaded && !self.loading {
                ui.label(
                    RichText::new("Nothing to show with these filters.")
                        .small()
                        .weak(),
                );
            }
            return;
        }
        self.table(ui);
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<usize> = None;
        let selected = self.sel.as_ref().map(Sel::key);
        let rows = &self.rows;
        TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .column(Column::exact(144.0))
            .column(Column::exact(PILL_W + 8.0))
            .column(Column::exact(64.0))
            .column(Column::remainder().at_least(120.0).clip(true))
            .column(Column::exact(190.0).clip(true))
            .header(ROW_H, |mut header| {
                for title in [
                    "Time (UTC)",
                    "Severity",
                    "Category",
                    "What happened",
                    "Source",
                ] {
                    header.col(|ui| {
                        ui.label(RichText::new(title).small().strong());
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_H, rows.len(), |mut row| {
                    let r = &rows[row.index()];
                    row.set_selected(selected == Some(r.sel.key()));
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(ms_to_text(r.time_ms)).monospace())
                                .selectable(false),
                        );
                    });
                    row.col(|ui| sev_pill(ui, r.severity, PILL_W));
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(category_text(r.category))
                                .selectable(false)
                                .truncate(),
                        );
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&r.headline).selectable(false).truncate());
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{} {}", r.provider, r.id))
                                    .small()
                                    .weak(),
                            )
                            .selectable(false)
                            .truncate(),
                        );
                    });
                    if row.response().clicked() {
                        clicked = Some(row.index());
                    }
                });
            });
        if let Some(i) = clicked {
            self.sel = Some(self.rows[i].sel.clone());
        }
    }

    fn crashes_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !self.loaded {
            return;
        }
        if self.analysis.crashes.is_empty() {
            ui.label(no_crashes_text(&self.statuses));
            return;
        }
        let crashes = std::mem::take(&mut self.analysis.crashes);
        for c in &crashes {
            if let Some(p) = &c.dump_path {
                self.dump_exists
                    .entry(c.noticed_ms)
                    .or_insert_with(|| Path::new(p).exists());
            }
        }
        let mut actions: Vec<CrashAction> = Vec::new();
        let version = env!("CARGO_PKG_VERSION");
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for crash in &crashes {
                    let id = crash.noticed_ms;
                    let exists = self.dump_exists.get(&id).copied().unwrap_or(false);
                    let header = self.dump_headers.get(&id);
                    look::card(ui, ui.available_width(), 0.0, |ui| {
                        ui.label(RichText::new(crash_title(&crash.kind)).strong().size(13.0));
                        ui.label(
                            RichText::new(format!(
                                "Stopped around {} UTC",
                                ms_to_text(crash.stopped_ms)
                            ))
                            .small()
                            .weak(),
                        );
                        if let Some(d) = crash_detail(&crash.kind) {
                            ui.label(d);
                        }
                        if let Some(d) = dump_line(&crash.kind, crash.dump_path.as_deref(), exists)
                        {
                            ui.label(RichText::new(d).small().weak());
                        }
                        match header {
                            Some(Ok(h)) => {
                                ui.label(RichText::new(dump_header_text(h, &crash.kind)).small());
                            }
                            Some(Err(e)) => {
                                ui.label(
                                    RichText::new(format!("Could not read the dump header: {e}"))
                                        .small()
                                        .color(AMBER),
                                );
                            }
                            None => {}
                        }
                        ui.horizontal(|ui| {
                            if let (true, Some(p)) = (exists, &crash.dump_path) {
                                let busy = self.dump_pending.contains(&id);
                                if ui
                                    .add_enabled(!busy, egui::Button::new("Read dump header"))
                                    .clicked()
                                {
                                    actions.push(CrashAction::ReadDump(id, p.clone()));
                                }
                            }
                            if ui.button("Copy crash report").clicked() {
                                actions.push(CrashAction::Copy(crash_report(
                                    version,
                                    crash,
                                    exists,
                                    header,
                                    self.keep_names,
                                )));
                            }
                        });
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("What happened in the 10 minutes before")
                                .small()
                                .strong(),
                        );
                        if crash.before.is_empty() {
                            ui.label(
                                RichText::new("No warnings or errors were logged in that time.")
                                    .small()
                                    .weak(),
                            );
                        }
                        for e in &crash.before {
                            let info = catalog::classify(e);
                            let is_sel = self.sel.as_ref().is_some_and(|s| {
                                matches!(s, Sel::Event(..))
                                    && s.event().channel == e.channel
                                    && s.event().record_id == e.record_id
                            });
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    Vec2::new(78.0, 18.0),
                                    egui::Label::new(
                                        RichText::new(minutes_before(crash.stopped_ms, e.time_ms))
                                            .monospace()
                                            .small()
                                            .weak(),
                                    ),
                                );
                                sev_pill(ui, info.severity, PILL_W);
                                if ui.selectable_label(is_sel, info.headline).clicked() {
                                    actions.push(CrashAction::Select(e.clone()));
                                }
                            });
                        }
                    });
                    ui.add_space(GAP);
                }
            });
        self.analysis.crashes = crashes;
        for a in actions {
            match a {
                CrashAction::Select(e) => {
                    let info = catalog::classify(&e);
                    self.sel = Some(Sel::Event(e, info));
                }
                CrashAction::Copy(text) => ctx.copy_text(text),
                CrashAction::ReadDump(id, path) => self.read_dump(id, path, ctx),
            }
        }
    }

    fn summary_page(&mut self, ui: &mut egui::Ui) {
        if !self.loaded {
            return;
        }
        if self.analysis.digest.is_empty() {
            let text = if self.statuses.is_empty() || self.statuses.iter().any(|s| !s.readable) {
                "No summary is available because part of the log could not be read."
            } else {
                "Nothing notable was found in the last 30 days."
            };
            ui.label(text);
        }
        for line in &self.analysis.digest {
            ui.horizontal_top(|ui| {
                sev_pill(ui, line.severity, PILL_W);
                ui.add(egui::Label::new(&line.text).wrap());
            });
        }
        ui.add_space(GAP);
        ui.label(
            RichText::new("What this can and cannot tell you")
                .small()
                .strong(),
        );
        ui.label(
            RichText::new(
                "Hints, not diagnoses. Crashes can have causes that leave no trace in the log.",
            )
            .small()
            .weak(),
        );
    }

    fn inspector(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let Some(sel) = self.sel.clone() else {
            ui.label(
                RichText::new("Select an event to see its details.")
                    .small()
                    .weak(),
            );
            return;
        };
        let (e, info) = (sel.event(), sel.info());
        ui.label(RichText::new(info.headline).strong());
        sev_pill(ui, info.severity, 0.0);
        let kv = |ui: &mut egui::Ui, k: &str, v: &str| {
            ui.add(egui::Label::new(RichText::new(format!("{k}: {v}")).small()).wrap());
        };
        if let Sel::Group(g) = &sel {
            kv(ui, "Repeated", &format!("{} times", g.count));
            kv(ui, "First", &format!("{} UTC", ms_to_text(g.first_ms)));
            kv(ui, "Last", &format!("{} UTC", ms_to_text(g.last_ms)));
            if let Some(s) = &g.subject {
                kv(ui, "About", s);
            }
        }
        kv(ui, "Provider", &e.provider);
        kv(ui, "Event ID", &e.id.to_string());
        kv(ui, "Level", &level_text(e.level));
        kv(ui, "Channel", &e.channel);
        kv(ui, "Time (UTC)", &ms_to_text(e.time_ms));
        if !info.hint.is_empty() {
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(info.hint).small().weak()).wrap());
        }
        if !e.data.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new("Event data").small().strong());
            for (k, v) in e.data.iter().take(40) {
                kv(ui, k, &shorten(v, 300));
            }
        }

        ui.add_space(4.0);
        let key = (e.channel.clone(), e.record_id);
        let message = self.messages.get(&key).cloned();
        match &message {
            Some(Some(m)) => {
                ui.label(RichText::new("Windows' own message").small().strong());
                ui.add(egui::Label::new(RichText::new(shorten(m, 1500)).small()).wrap());
            }
            Some(None) => {
                ui.label(
                    RichText::new("Windows has no message text for this event.")
                        .small()
                        .weak(),
                );
            }
            None if self.message_pending.contains(&key) => {
                ui.label(RichText::new("Asking Windows...").small().weak());
            }
            None => {
                if ui.button("Show Windows' own message").clicked() {
                    self.ask_message(e, ctx);
                }
            }
        }

        ui.add_space(4.0);
        ui.checkbox(&mut self.keep_names, "Keep names and paths");
        if ui.button("Copy report").clicked() {
            let text = event_report(
                env!("CARGO_PKG_VERSION"),
                &sel,
                message.as_ref().and_then(|m| m.as_deref()),
                self.keep_names,
            );
            ctx.copy_text(text);
        }
    }
}

/// A small rounded label tinted with the severity color; `width` 0 means as wide as the text.
fn sev_pill(ui: &mut egui::Ui, s: Severity, width: f32) {
    let color = sev_color(s);
    let font = egui::FontId::proportional(10.5);
    let galley = ui
        .painter()
        .layout_no_wrap(sev_label(s).to_owned(), font, color);
    let w = if width > 0.0 {
        width
    } else {
        galley.size().x + 14.0
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, 8.0, color.gamma_multiply(0.18));
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, sev_label(s)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(channel: &str, rec: u64, t: i64, provider: &str, id: u32) -> Event {
        Event {
            record_id: rec,
            time_ms: t,
            channel: channel.to_string(),
            provider: provider.to_string(),
            id,
            level: 2,
            data: Vec::new(),
            message: None,
        }
    }

    fn info(sev: Severity, headline: &'static str) -> Info {
        Info {
            category: Category::Disk,
            severity: sev,
            headline,
            hint: "",
            known: true,
        }
    }

    fn group(count: usize, subject: Option<&str>, sev: Severity, last: i64) -> Group {
        Group {
            provider: "disk".into(),
            id: 153,
            subject: subject.map(str::to_string),
            count,
            first_ms: 0,
            last_ms: last,
            sample: ev("System", 1, last, "disk", 153),
            info: info(sev, "Disk retried a read."),
        }
    }

    #[test]
    fn formats_utc_time() {
        assert_eq!(ms_to_text(0), "1970-01-01 00:00:00");
        assert_eq!(ms_to_text(1_700_000_000_000), "2023-11-14 22:13:20");
        assert_eq!(ms_to_text(951_782_400_000), "2000-02-29 00:00:00");
    }

    #[test]
    fn severity_filter_follows_flags() {
        let show = [true, true, false, false];
        assert!(severity_shown(Severity::NeedsAttention, &show));
        assert!(severity_shown(Severity::WorthWatching, &show));
        assert!(!severity_shown(Severity::Harmless, &show));
        assert!(!severity_shown(Severity::Info, &show));
    }

    #[test]
    fn text_filter_matches_every_word() {
        assert!(matches_filter("", "disk", 7, "x", None));
        assert!(matches_filter("DISK 153", "disk", 153, "Retried", None));
        assert!(matches_filter(
            "retried",
            "disk",
            153,
            "Disk retried a read",
            None
        ));
        assert!(matches_filter("disk 0", "x", 1, "y", Some("Disk 0")));
        assert!(!matches_filter("disk 154", "disk", 153, "Retried", None));
    }

    #[test]
    fn headline_gets_count_and_subject() {
        assert_eq!(
            row_headline("Retried.", 143, Some("Disk 0")),
            "x143 Disk 0: Retried."
        );
        assert_eq!(row_headline("Retried.", 1, None), "Retried.");
        assert_eq!(row_headline("Retried.", 2, None), "x2 Retried.");
    }

    #[test]
    fn grouped_rows_use_groups_and_sort_newest_first() {
        let groups = vec![
            group(3, Some("Disk 0"), Severity::NeedsAttention, 100),
            group(1, None, Severity::Harmless, 300),
            group(5, None, Severity::WorthWatching, 200),
        ];
        let show = [true, true, false, false];
        let rows = build_rows(&[], &groups, true, "", &show);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].time_ms, 200);
        assert_eq!(rows[1].headline, "x3 Disk 0: Disk retried a read.");
        let rows = build_rows(&[], &groups, true, "disk 0", &show);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn raw_rows_are_one_per_event() {
        // The skeleton catalog may classify everything the same; only the shape is checked.
        let events = vec![ev("System", 1, 10, "a", 1), ev("System", 2, 20, "a", 1)];
        let rows = build_rows(&events, &[], false, "", &[true; 4]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].time_ms, 20);
    }

    #[test]
    fn minutes_before_text() {
        assert_eq!(
            minutes_before(1_000_000, 1_000_000 - 200_000),
            "-3 min 20 s"
        );
        assert_eq!(minutes_before(1_000_000, 1_000_000 - 120_000), "-2 min");
        assert_eq!(minutes_before(1_000_000, 1_000_000 - 5_000), "-5 s");
        assert_eq!(minutes_before(1_000_000, 1_000_000 + 5_000), "-0 s");
    }

    #[test]
    fn crash_titles() {
        assert_eq!(
            crash_title(&CrashKind::ForcedRestart),
            "Forced restart (power button held)"
        );
        assert_eq!(
            crash_title(&CrashKind::PowerLossOrHang),
            "Power loss or freeze"
        );
        assert_eq!(crash_title(&CrashKind::Unknown), "Unknown restart");
        let t = crash_title(&CrashKind::BlueScreen {
            code: 0x139,
            params: [0; 4],
        });
        assert!(t.starts_with("Bluescreen 0x00000139"));
    }

    #[test]
    fn dump_lines() {
        let bsod = CrashKind::BlueScreen {
            code: 1,
            params: [0; 4],
        };
        assert_eq!(
            dump_line(&bsod, Some("C:\\Windows\\MEMORY.DMP"), true).unwrap(),
            "Windows recorded a dump at C:\\Windows\\MEMORY.DMP"
        );
        assert!(dump_line(&bsod, Some("C:\\x.dmp"), false)
            .unwrap()
            .ends_with("The dump file is missing."));
        assert!(dump_line(&CrashKind::Unknown, None, false).is_none());
    }

    #[test]
    fn coverage_and_warnings() {
        let ok = ChannelStatus {
            name: "System".into(),
            readable: true,
            error: None,
            oldest_ms: Some(1_700_000_000_000),
        };
        let bad = ChannelStatus {
            name: "Application".into(),
            readable: false,
            error: Some("denied".into()),
            oldest_ms: None,
        };
        assert_eq!(
            coverage_line(std::slice::from_ref(&ok), 0).unwrap(),
            "The log covers from 2023-11-14 22:13 UTC."
        );
        // Never earlier than what was asked for.
        assert!(coverage_line(std::slice::from_ref(&ok), 1_800_000_000_000)
            .unwrap()
            .contains("2027"));
        assert!(coverage_line(std::slice::from_ref(&bad), 0).is_none());
        assert_eq!(
            unreadable_warnings(&[ok.clone(), bad.clone()]),
            vec!["Windows did not let toolboks read the Application log."]
        );
        assert!(no_crashes_text(std::slice::from_ref(&ok)).contains("last 30 days"));
        assert!(!no_crashes_text(&[ok, bad]).contains("last 30 days"));
        assert!(!no_crashes_text(&[]).contains("last 30 days"));
    }

    #[test]
    fn merge_dedupes_and_sorts() {
        let mut events = vec![ev("System", 1, 10, "a", 1), ev("System", 3, 30, "a", 1)];
        let added = merge_events(
            &mut events,
            vec![
                ev("System", 3, 30, "a", 1),
                ev("Application", 3, 25, "a", 1),
                ev("System", 4, 40, "a", 1),
            ],
        );
        assert_eq!(added, 2);
        let times: Vec<i64> = events.iter().map(|e| e.time_ms).collect();
        assert_eq!(times, vec![10, 25, 30, 40]);
        assert_eq!(
            merge_events(&mut events, vec![ev("System", 1, 10, "a", 1)]),
            0
        );
    }

    #[test]
    fn redacts_user_name_and_profile_paths() {
        assert_eq!(
            redact_for("C:\\Users\\Bob\\AppData\\x.dmp failed for bob", Some("bob")),
            "<user>\\AppData\\x.dmp failed for <user>"
        );
        assert_eq!(
            redact_for("D:\\Users\\John Smith\\Desktop\\a", Some("John Smith")),
            "<user>\\Desktop\\a"
        );
        assert_eq!(redact_for("c:\\users\\alice\\f", None), "<user>\\f");
        assert_eq!(
            redact_for("C:\\Users\\Public\\Documents", Some("bob")),
            "C:\\Users\\Public\\Documents"
        );
        assert_eq!(
            redact_for("bobby and bob_x", Some("bob")),
            "bobby and bob_x"
        );
        assert_eq!(redact_for("nothing here", Some("bob")), "nothing here");
    }

    #[test]
    fn reports_redact_by_default() {
        let mut e = ev("System", 9, 1_700_000_000_000, "Prov", 7);
        e.data.push(("Path".into(), "C:\\Users\\zed\\a.dmp".into()));
        let sel = Sel::Event(e, info(Severity::WorthWatching, "Something happened."));
        let text = event_report("1.0", &sel, Some("hello"), false);
        assert!(text.contains("toolboks 1.0"));
        assert!(text.contains("Something happened."));
        assert!(text.contains("<user>\\a.dmp"));
        assert!(text.contains("Windows' own message"));
        let kept = event_report("1.0", &sel, None, true);
        assert!(kept.contains("C:\\Users\\zed\\a.dmp"));
    }

    #[test]
    fn crash_report_has_timeline() {
        let crash = Crash {
            noticed_ms: 2_000_000,
            stopped_ms: 1_000_000,
            kind: CrashKind::PowerLossOrHang,
            dump_path: None,
            before: vec![ev("System", 1, 800_000, "Prov", 7)],
        };
        let t = crash_report("1.0", &crash, false, None, true);
        assert!(t.contains("Power loss or freeze"));
        assert!(t.contains("-3 min 20 s"));
    }
}
