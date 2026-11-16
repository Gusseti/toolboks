//! Health on Home (#54): the findings the add-ons report, validated, kept and ranked.
//!
//! Findings come from add-ons, so they are untrusted input. The host:
//! - files every finding under the id of the connection it came from (there is no source field
//!   on the wire), so an add-on cannot speak for another;
//! - keeps at most [`MAX_FINDINGS`] per add-on, each replacing the add-on's whole set;
//! - drops a finding whose id breaks the pattern or repeats, whose title is empty after
//!   cleaning, or whose `OpenPage` route leads outside the add-on's own catalog pages;
//! - cleans every text like status items (control characters removed, trimmed, cut to the
//!   limit), takes a `measured_at` in the future as the receive time and clamps
//!   `stale_after_secs`;
//! - cuts `why` and the evidence labels to their limits, clamps evidence numbers to 0..=1 (a
//!   number that is not one becomes 0, a series keeps its newest values), and drops evidence
//!   without labels (the labels carry the numbers; colour is never the only signal) or with a
//!   bar that is not a number.
//!
//! **Memory only.** Nothing here is written to disk, put in a report or sent anywhere. After a
//! restart Home starts empty and fills as the add-ons report again.
//!
//! [`Store::view`] is pure: it takes the time, so ranking, staleness and caps are tested with
//! made-up times.

use crate::addon_services::clip;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use toolboks_addon_proto::{
    Evidence, Finding, FindingAction, FindingStatus, Severity, MAX_EVIDENCE_LABEL_CHARS,
    MAX_EVIDENCE_VALUES, MAX_FINDINGS, MAX_FINDING_LABEL_CHARS, MAX_FINDING_TEXT_CHARS,
    MAX_FINDING_TITLE_CHARS, MAX_FINDING_WHY_CHARS, MAX_STALE_AFTER_SECS, MIN_STALE_AFTER_SECS,
};

/// "Needs attention" shows at most this many rows per add-on; the rest are counted in
/// [`View::more`].
pub const MAX_ATTENTION_PER_SOURCE: usize = 3;

/// The findings of every add-on that reported, by add-on id.
#[derive(Default)]
pub struct Store {
    sources: BTreeMap<String, Source>,
    /// How many sets were received in total; Home's "Checking…" ends when it moves.
    received: u64,
}

struct Source {
    findings: Vec<Kept>,
    running: bool,
}

struct Kept {
    finding: Finding,
    received_at: u64,
}

/// One finding as Home shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The add-on it came from (the connection's id).
    pub source: String,
    /// That add-on's title from its installed entry, drawn next to the finding.
    pub source_title: String,
    /// Cleaned and within every limit.
    pub finding: Finding,
    /// `now - measured_at > stale_after_secs`.
    pub out_of_date: bool,
    /// False once the add-on stopped: "<Add-on> is not running".
    pub running: bool,
    /// When the host received it, Unix seconds.
    pub received_at: u64,
}

/// What Home shows, ranked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// "Needs attention": `Attention` findings, at most [`MAX_ATTENTION_PER_SOURCE`] per add-on.
    pub attention: Vec<Row>,
    /// "Also checked": every other finding.
    pub checked: Vec<Row>,
    /// `(add-on id, count)` of attention rows left out by the cap ("2 more from Disk Manager"),
    /// in the order the add-ons first appear in `attention`.
    pub more: Vec<(String, usize)>,
}

impl Store {
    /// The add-on `id` sent its full set: it replaces the previous one. `pages` are the add-on's
    /// catalog pages (an `OpenPage` route must start with one), `now` is the receive time in
    /// Unix seconds. Returns one line per dropped finding for the add-on's report log; the lines
    /// name a finding by its position and, once valid, its id, never its text.
    pub fn replace(
        &mut self,
        id: &str,
        list: &[Finding],
        pages: &[String],
        now: u64,
    ) -> Vec<String> {
        let mut dropped = Vec::new();
        if list.len() > MAX_FINDINGS {
            dropped.push(format!(
                "dropped {} findings over the limit of {MAX_FINDINGS}",
                list.len() - MAX_FINDINGS
            ));
        }
        let mut seen = BTreeSet::new();
        let mut findings = Vec::new();
        for (index, finding) in list.iter().take(MAX_FINDINGS).enumerate() {
            match clean(finding, pages, now, &mut dropped) {
                Ok(finding) if !seen.insert(finding.id.clone()) => dropped.push(format!(
                    "dropped finding {index} ({}): its id repeats an earlier one",
                    finding.id
                )),
                Ok(finding) => findings.push(Kept {
                    finding,
                    received_at: now,
                }),
                Err(why) => dropped.push(format!("dropped finding {index}{why}")),
            }
        }
        self.sources.insert(
            id.to_string(),
            Source {
                findings,
                running: true,
            },
        );
        self.received += 1;
        dropped
    }

    /// How many sets were received since start; it only grows.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// The add-on has reported a set (possibly empty) that is still kept.
    pub fn reported(&self, id: &str) -> bool {
        self.sources.contains_key(id)
    }

    /// The add-on started or stopped. Its findings stay either way, so Home can show their age
    /// and say it is not running.
    pub fn set_running(&mut self, id: &str, running: bool) {
        if let Some(source) = self.sources.get_mut(id) {
            source.running = running;
        }
    }

    /// The add-on was turned off or uninstalled: its findings go.
    pub fn remove(&mut self, id: &str) {
        self.sources.remove(id);
    }

    /// Ranks every finding for Home at `now` (Unix seconds). `titles` gives an add-on's title
    /// from its installed entry; an add-on it has no title for (not installed or turned off)
    /// is left out.
    pub fn view(&self, now: u64, titles: &dyn Fn(&str) -> Option<String>) -> View {
        let mut attention = Vec::new();
        let mut checked = Vec::new();
        for (id, source) in &self.sources {
            let Some(title) = titles(id) else {
                continue;
            };
            for kept in &source.findings {
                let row = Row {
                    source: id.clone(),
                    source_title: title.clone(),
                    finding: kept.finding.clone(),
                    out_of_date: out_of_date(&kept.finding, now),
                    running: source.running,
                    received_at: kept.received_at,
                };
                if row.finding.status == FindingStatus::Attention {
                    attention.push(row);
                } else {
                    checked.push(row);
                }
            }
        }
        // Current before out of date, Problem before Watch, newest, then source title and id.
        attention.sort_by(|a, b| {
            let key = |row: &Row| {
                (
                    row.out_of_date,
                    row.finding.severity != Severity::Problem,
                    Reverse(row.finding.measured_at),
                )
            };
            key(a)
                .cmp(&key(b))
                .then_with(|| a.source_title.cmp(&b.source_title))
                .then_with(|| a.finding.id.cmp(&b.finding.id))
                .then_with(|| a.source.cmp(&b.source))
        });
        // Not checked, unavailable, out-of-date OK, current OK; then source title and title.
        checked.sort_by(|a, b| {
            group(a)
                .cmp(&group(b))
                .then_with(|| a.source_title.cmp(&b.source_title))
                .then_with(|| a.finding.title.cmp(&b.finding.title))
                .then_with(|| a.finding.id.cmp(&b.finding.id))
                .then_with(|| a.source.cmp(&b.source))
        });
        let mut shown: BTreeMap<String, usize> = BTreeMap::new();
        let mut more: Vec<(String, usize)> = Vec::new();
        attention.retain(|row| {
            let count = shown.entry(row.source.clone()).or_default();
            *count += 1;
            if *count <= MAX_ATTENTION_PER_SOURCE {
                return true;
            }
            match more.iter_mut().find(|(id, _)| *id == row.source) {
                Some((_, left)) => *left += 1,
                None => more.push((row.source.clone(), 1)),
            }
            false
        });
        View {
            attention,
            checked,
            more,
        }
    }
}

/// The reading is older than its add-on said it stays current.
fn out_of_date(finding: &Finding, now: u64) -> bool {
    now.saturating_sub(finding.measured_at) > u64::from(finding.stale_after_secs)
}

/// The order of the "Also checked" groups.
fn group(row: &Row) -> u8 {
    match row.finding.status {
        FindingStatus::NotChecked => 0,
        FindingStatus::Unavailable => 1,
        FindingStatus::Ok if row.out_of_date => 2,
        FindingStatus::Ok => 3,
        // Never in "Also checked".
        FindingStatus::Attention => 4,
    }
}

/// The finding cut to every limit, or why it is dropped (appended to "dropped finding N").
/// A broken action is dropped with a line in `notes`; the finding stays.
fn clean(
    finding: &Finding,
    pages: &[String],
    now: u64,
    notes: &mut Vec<String>,
) -> Result<Finding, String> {
    if !Finding::valid_id(&finding.id) {
        return Err(": its id is not 1 to 48 characters of a-z, 0-9, '.' and '-'".into());
    }
    let id = &finding.id;
    let title = clip(&finding.title, MAX_FINDING_TITLE_CHARS);
    if title.is_empty() {
        return Err(format!(" ({id}): its title is empty"));
    }
    let action = match &finding.action {
        None => None,
        Some(FindingAction::OpenPage { label, route }) => {
            let route = clip(route, MAX_FINDING_TEXT_CHARS);
            let page = route.split('/').next().unwrap_or_default();
            if !pages.iter().any(|own| own == page) {
                return Err(format!(
                    " ({id}): its action opens a page the add-on does not have"
                ));
            }
            let label = clip(label, MAX_FINDING_LABEL_CHARS);
            if label.is_empty() {
                notes.push(format!(
                    "dropped the action of finding {id}: its label is empty"
                ));
                None
            } else {
                Some(FindingAction::OpenPage { label, route })
            }
        }
        Some(FindingAction::Instruction { text }) => {
            let text = clip(text, MAX_FINDING_TEXT_CHARS);
            if text.is_empty() {
                notes.push(format!(
                    "dropped the action of finding {id}: its instruction is empty"
                ));
                None
            } else {
                Some(FindingAction::Instruction { text })
            }
        }
    };
    Ok(Finding {
        id: id.clone(),
        title,
        observation: clip(&finding.observation, MAX_FINDING_TEXT_CHARS),
        status: finding.status,
        severity: finding.severity,
        measured_at: finding.measured_at.min(now),
        stale_after_secs: finding
            .stale_after_secs
            .clamp(MIN_STALE_AFTER_SECS, MAX_STALE_AFTER_SECS),
        action,
        why: finding
            .why
            .as_deref()
            .map(|why| clip(why, MAX_FINDING_WHY_CHARS))
            .filter(|why| !why.is_empty()),
        evidence: finding
            .evidence
            .as_ref()
            .and_then(|evidence| clean_evidence(evidence, id, notes)),
    })
}

/// A number for a picture: 0 to 1, and 0 for one that is not a number.
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The evidence cut to every limit, or `None` (with a line in `notes`) when it cannot be shown
/// honestly: no label to carry the numbers, a bar that is not a number, or a series without
/// values.
fn clean_evidence(evidence: &Evidence, id: &str, notes: &mut Vec<String>) -> Option<Evidence> {
    let (left, right) = evidence.labels();
    let left = clip(left, MAX_EVIDENCE_LABEL_CHARS);
    let right = clip(right, MAX_EVIDENCE_LABEL_CHARS);
    let drop = |notes: &mut Vec<String>, why: &str| {
        notes.push(format!("dropped the evidence of finding {id}: {why}"));
        None
    };
    if left.is_empty() && right.is_empty() {
        return drop(notes, "it has no labels");
    }
    match evidence {
        Evidence::Bar { fraction, .. } if !fraction.is_finite() => {
            drop(notes, "its bar is not a number")
        }
        Evidence::Bar { fraction, .. } => Some(Evidence::Bar {
            fraction: unit(*fraction),
            left,
            right,
        }),
        Evidence::Series { values, .. } if values.is_empty() => drop(notes, "it has no values"),
        Evidence::Series { values, .. } => {
            let newest = &values[values.len().saturating_sub(MAX_EVIDENCE_VALUES)..];
            Some(Evidence::Series {
                values: newest.iter().copied().map(unit).collect(),
                left,
                right,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fake clock: every test receives at this time.
    const NOW: u64 = 1_800_000_000;

    fn pages(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn ok(id: &str) -> Finding {
        Finding::ok(id, format!("Title {id}"), "Seen")
            .measured_at(NOW)
            .stale_after(600)
    }

    fn attention(id: &str, severity: Severity, measured_at: u64) -> Finding {
        Finding::attention(id, format!("Title {id}"), "Seen", severity)
            .measured_at(measured_at)
            .stale_after(600)
    }

    fn titles(id: &str) -> Option<String> {
        match id {
            "a.disk" => Some("Disk Manager".into()),
            "b.memory" => Some("Memory Cleaner".into()),
            "c.events" => Some("Event Watch".into()),
            _ => None,
        }
    }

    fn view(store: &Store, now: u64) -> View {
        store.view(now, &titles)
    }

    fn ids(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| format!("{}:{}", row.source, row.finding.id))
            .collect()
    }

    fn kept(store: &Store, id: &str) -> Vec<Finding> {
        store.sources[id]
            .findings
            .iter()
            .map(|kept| kept.finding.clone())
            .collect()
    }

    #[test]
    fn at_most_16_findings_are_kept_and_the_17th_is_dropped() {
        let mut store = Store::default();
        let list: Vec<_> = (0..17).map(|i| ok(&format!("check-{i}"))).collect();
        let dropped = store.replace("a.disk", &list, &[], NOW);
        let kept = kept(&store, "a.disk");
        assert_eq!(kept.len(), 16);
        assert_eq!(kept.last().unwrap().id, "check-15");
        assert!(!kept.iter().any(|finding| finding.id == "check-16"));
        assert_eq!(dropped, ["dropped 1 findings over the limit of 16"]);
    }

    #[test]
    fn a_bad_or_repeated_id_drops_only_that_finding_and_the_first_wins() {
        let mut store = Store::default();
        let too_long = "a".repeat(49);
        let mut second = ok("same");
        second.title = "Second".into();
        let list = [
            ok("same"),
            ok("Upper"),
            ok(""),
            ok(&too_long),
            ok("has space"),
            ok("slash/no"),
            ok("ünicode"),
            second,
            ok(&"a".repeat(48)),
            ok("dots.and-dashes.09"),
        ];
        let dropped = store.replace("a.disk", &list, &[], NOW);
        let kept = kept(&store, "a.disk");
        let kept_ids: Vec<_> = kept.iter().map(|finding| finding.id.as_str()).collect();
        assert_eq!(
            kept_ids,
            ["same", "a".repeat(48).as_str(), "dots.and-dashes.09"]
        );
        assert_eq!(kept[0].title, "Title same", "the first one wins");
        assert_eq!(dropped.len(), 7, "{dropped:?}");
        for (line, index) in dropped.iter().zip(1..=6) {
            assert!(
                line.starts_with(&format!("dropped finding {index}: its id")),
                "{line}"
            );
        }
        assert_eq!(
            dropped[6],
            "dropped finding 7 (same): its id repeats an earlier one"
        );
        // A bad id is never written into the log line: it may be any text.
        assert!(!dropped.iter().any(|line| line.contains("has space")));
    }

    #[test]
    fn a_title_empty_after_cleaning_drops_the_finding() {
        let mut store = Store::default();
        let mut blank = ok("blank");
        blank.title = " \n\t\u{7}  ".into();
        let dropped = store.replace("a.disk", &[blank, ok("fine")], &[], NOW);
        assert_eq!(kept(&store, "a.disk").len(), 1);
        assert_eq!(dropped, ["dropped finding 0 (blank): its title is empty"]);
    }

    #[test]
    fn a_route_outside_the_add_ons_own_pages_drops_the_finding() {
        let mut store = Store::default();
        let own = pages(&["Home", "Optimization"]);
        let list = [
            ok("home").open_page("Open", "Home"),
            ok("deep").open_page("Open drive", "Optimization/C:"),
            ok("other").open_page("Open", "Crashes"),
            ok("case").open_page("Open", "home"),
            ok("empty").open_page("Open", ""),
            ok("web").open_page("Open", "https://example.com/Home"),
            ok("lead").open_page("Open", "/Home"),
        ];
        let dropped = store.replace("a.disk", &list, &own, NOW);
        let kept = kept(&store, "a.disk");
        let kept_ids: Vec<_> = kept.iter().map(|finding| finding.id.as_str()).collect();
        assert_eq!(kept_ids, ["home", "deep"]);
        assert_eq!(
            kept[1].action,
            Some(FindingAction::OpenPage {
                label: "Open drive".into(),
                route: "Optimization/C:".into()
            })
        );
        assert_eq!(dropped.len(), 5, "{dropped:?}");
        assert!(dropped
            .iter()
            .all(|line| line.ends_with("its action opens a page the add-on does not have")));
        // With no pages at all, no OpenPage is kept.
        store.replace("a.disk", &list[..1], &[], NOW);
        assert!(store.sources["a.disk"].findings.is_empty());
    }

    #[test]
    fn a_measured_at_in_the_future_becomes_the_receive_time() {
        let mut store = Store::default();
        let list = [
            ok("future").measured_at(NOW + 3600),
            ok("past").measured_at(NOW - 5),
        ];
        store.replace("a.disk", &list, &[], NOW);
        let kept = kept(&store, "a.disk");
        assert_eq!(kept[0].measured_at, NOW);
        assert_eq!(kept[1].measured_at, NOW - 5);
        assert_eq!(store.sources["a.disk"].findings[0].received_at, NOW);
    }

    #[test]
    fn stale_after_is_clamped_to_a_minute_to_seven_days() {
        let mut store = Store::default();
        let list = [
            ok("zero").stale_after(0),
            ok("short").stale_after(59),
            ok("low").stale_after(60),
            ok("high").stale_after(604_800),
            ok("long").stale_after(604_801),
            ok("max").stale_after(u32::MAX),
        ];
        store.replace("a.disk", &list, &[], NOW);
        let stale: Vec<_> = kept(&store, "a.disk")
            .iter()
            .map(|finding| finding.stale_after_secs)
            .collect();
        assert_eq!(stale, [60, 60, 60, 604_800, 604_800, 604_800]);
    }

    #[test]
    fn text_is_cleaned_of_control_characters_trimmed_and_cut() {
        let mut store = Store::default();
        let mut finding = ok("text").open_page(
            format!("Open\u{1b}[31m {}", "x".repeat(60)),
            "Home/\u{0}deep",
        );
        finding.title = format!("  Line\none\r\t{}  ", "t".repeat(100));
        finding.observation = format!("\u{7}Seen\u{85} {}", "o".repeat(300));
        let instruction = ok("tip").instruction(format!("Do\u{1}this {}", "i".repeat(300)));
        store.replace("a.disk", &[finding, instruction], &pages(&["Home"]), NOW);
        let kept = kept(&store, "a.disk");
        let title = &kept[0].title;
        // Cut to the limit first, then trimmed: the two leading spaces count.
        assert!(title.starts_with("Lineone"), "{title}");
        assert_eq!(title.chars().count(), MAX_FINDING_TITLE_CHARS - 2);
        assert!(kept[0].observation.starts_with("Seen o"));
        assert_eq!(kept[0].observation.chars().count(), MAX_FINDING_TEXT_CHARS);
        let Some(FindingAction::OpenPage { label, route }) = &kept[0].action else {
            panic!("{:?}", kept[0].action);
        };
        assert!(label.starts_with("Open[31m x"), "{label}");
        assert_eq!(label.chars().count(), MAX_FINDING_LABEL_CHARS);
        assert_eq!(route, "Home/deep");
        let Some(FindingAction::Instruction { text }) = &kept[1].action else {
            panic!("{:?}", kept[1].action);
        };
        assert!(text.starts_with("Dothis i"));
        assert_eq!(text.chars().count(), MAX_FINDING_TEXT_CHARS);
        for finding in &kept {
            let all = format!(
                "{}{}{:?}",
                finding.title, finding.observation, finding.action
            );
            assert!(!all.chars().any(char::is_control), "{all:?}");
        }
    }

    #[test]
    fn why_is_cleaned_and_cut_and_an_empty_one_is_none() {
        let mut store = Store::default();
        let list = [
            ok("long").why(format!("\u{7}Because\n {}", "w".repeat(300))),
            ok("blank").why(" \t "),
            ok("none"),
        ];
        let dropped = store.replace("a.disk", &list, &[], NOW);
        assert!(dropped.is_empty(), "{dropped:?}");
        let kept = kept(&store, "a.disk");
        let why = kept[0].why.as_deref().unwrap();
        assert!(why.starts_with("Because w"), "{why}");
        assert_eq!(why.chars().count(), MAX_FINDING_WHY_CHARS);
        assert!(!why.chars().any(char::is_control));
        assert_eq!(kept[1].why, None);
        assert_eq!(kept[2].why, None);
    }

    #[test]
    fn evidence_is_clamped_cut_and_dropped_when_it_cannot_be_read() {
        let mut store = Store::default();
        let long = "l".repeat(60);
        let many: Vec<f32> = (0..30).map(|i| i as f32 / 10.0 - 1.0).collect();
        let list = [
            ok("over").evidence(Evidence::bar(1.7, &long, "6 GB\u{0} free")),
            ok("under").evidence(Evidence::bar(-0.2, "", "right only")),
            ok("nan").evidence(Evidence::bar(f32::NAN, "a", "b")),
            ok("unlabelled").evidence(Evidence::bar(0.5, " ", "\n")),
            ok("series").evidence(Evidence::series(many.clone(), "10 min ago", "now")),
            ok("odd").evidence(Evidence::series(
                vec![f32::INFINITY, f32::NAN, 0.25],
                "a",
                "b",
            )),
            ok("empty").evidence(Evidence::series(Vec::new(), "a", "b")),
        ];
        let dropped = store.replace("a.disk", &list, &[], NOW);
        // Bad evidence goes, the finding stays.
        let kept = kept(&store, "a.disk");
        assert_eq!(kept.len(), 7);
        assert_eq!(
            dropped,
            [
                "dropped the evidence of finding nan: its bar is not a number",
                "dropped the evidence of finding unlabelled: it has no labels",
                "dropped the evidence of finding empty: it has no values",
            ]
        );
        let Some(Evidence::Bar {
            fraction,
            left,
            right,
        }) = &kept[0].evidence
        else {
            panic!("{:?}", kept[0].evidence);
        };
        assert_eq!(*fraction, 1.0);
        assert_eq!(left.chars().count(), MAX_EVIDENCE_LABEL_CHARS);
        assert_eq!(right, "6 GB free");
        assert_eq!(kept[1].evidence, Some(Evidence::bar(0.0, "", "right only")));
        assert_eq!(kept[2].evidence, None);
        assert_eq!(kept[3].evidence, None);
        let Some(Evidence::Series { values, .. }) = &kept[4].evidence else {
            panic!("{:?}", kept[4].evidence);
        };
        // The newest 24 (the last ones), each within 0..=1.
        assert_eq!(values.len(), MAX_EVIDENCE_VALUES);
        let expected: Vec<f32> = many[6..].iter().map(|v| v.clamp(0.0, 1.0)).collect();
        assert_eq!(values, &expected);
        assert_eq!(
            kept[5].evidence,
            Some(Evidence::series(vec![0.0, 0.0, 0.25], "a", "b"))
        );
        assert_eq!(kept[6].evidence, None);
    }

    #[test]
    fn an_empty_action_is_dropped_but_the_finding_stays() {
        let mut store = Store::default();
        let list = [
            ok("label").open_page(" \n ", "Home"),
            ok("tip").instruction("\t"),
        ];
        let dropped = store.replace("a.disk", &list, &pages(&["Home"]), NOW);
        let kept = kept(&store, "a.disk");
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|finding| finding.action.is_none()));
        assert_eq!(dropped.len(), 2, "{dropped:?}");
    }

    #[test]
    fn each_message_replaces_the_whole_set_of_that_add_on_only() {
        let mut store = Store::default();
        store.replace("a.disk", &[ok("one"), ok("two")], &[], NOW);
        store.replace("b.memory", &[ok("mem")], &[], NOW);
        store.replace("a.disk", &[ok("three")], &[], NOW);
        let shown = view(&store, NOW);
        assert_eq!(ids(&shown.checked), ["a.disk:three", "b.memory:mem"]);
        store.replace("a.disk", &[], &[], NOW);
        assert_eq!(ids(&view(&store, NOW).checked), ["b.memory:mem"]);
    }

    #[test]
    fn a_finding_is_out_of_date_only_after_stale_after_has_passed() {
        let mut store = Store::default();
        store.replace(
            "a.disk",
            &[ok("ok").measured_at(NOW - 100).stale_after(60)],
            &[],
            NOW,
        );
        let at = |now| view(&store, now).checked[0].out_of_date;
        // Measured at NOW - 100, current for 60 s: out of date from NOW - 39 on.
        assert!(!at(NOW - 100));
        assert!(!at(NOW - 40), "exactly stale_after old is still current");
        assert!(at(NOW - 39));
        assert!(at(NOW + 10_000));
        // A clock that went backwards does not make it out of date.
        assert!(!at(NOW - 1_000));
    }

    #[test]
    fn an_out_of_date_attention_stays_in_needs_attention_and_an_out_of_date_ok_is_not_ok() {
        let mut store = Store::default();
        let list = [
            attention("old", Severity::Problem, NOW - 1000),
            ok("ok-old").measured_at(NOW - 1000),
            ok("ok-new"),
        ];
        store.replace("a.disk", &list, &[], NOW);
        let shown = view(&store, NOW);
        assert_eq!(ids(&shown.attention), ["a.disk:old"]);
        assert!(shown.attention[0].out_of_date);
        assert_eq!(ids(&shown.checked), ["a.disk:ok-old", "a.disk:ok-new"]);
        assert!(shown.checked[0].out_of_date);
        assert!(!shown.checked[1].out_of_date);
    }

    #[test]
    fn findings_are_kept_after_the_add_on_stops() {
        let mut store = Store::default();
        store.replace("c.events", &[ok("crashes")], &[], NOW);
        assert!(view(&store, NOW).checked[0].running);
        store.set_running("c.events", false);
        let shown = view(&store, NOW + 3600);
        assert_eq!(ids(&shown.checked), ["c.events:crashes"]);
        assert!(!shown.checked[0].running);
        assert!(shown.checked[0].out_of_date, "it ages while stopped");
        // Running again and a new report.
        store.set_running("c.events", true);
        assert!(view(&store, NOW).checked[0].running);
        store.set_running("c.events", false);
        store.replace("c.events", &[ok("crashes")], &[], NOW);
        assert!(
            view(&store, NOW).checked[0].running,
            "a report means it runs"
        );
        // An add-on that never reported gets no entry.
        store.set_running("b.memory", true);
        assert!(!store.sources.contains_key("b.memory"));
    }

    #[test]
    fn turning_an_add_on_off_or_removing_it_removes_its_findings() {
        let mut store = Store::default();
        store.replace("a.disk", &[ok("disk")], &[], NOW);
        store.replace("b.memory", &[ok("mem")], &[], NOW);
        store.remove("a.disk");
        assert_eq!(ids(&view(&store, NOW).checked), ["b.memory:mem"]);
        store.remove("a.disk");
        store.remove("b.memory");
        assert_eq!(view(&store, NOW), View::default());
    }

    #[test]
    fn an_add_on_without_a_title_is_not_shown() {
        let mut store = Store::default();
        store.replace(
            "z.unknown",
            &[attention("x", Severity::Problem, NOW)],
            &[],
            NOW,
        );
        store.replace("a.disk", &[ok("disk")], &[], NOW);
        let shown = view(&store, NOW);
        assert!(shown.attention.is_empty());
        assert_eq!(ids(&shown.checked), ["a.disk:disk"]);
        assert_eq!(shown.checked[0].source_title, "Disk Manager");
    }

    #[test]
    fn needs_attention_shows_at_most_3_per_add_on_and_counts_the_rest() {
        let mut store = Store::default();
        let five: Vec<_> = (0..5)
            .map(|i| attention(&format!("p{i}"), Severity::Problem, NOW - i))
            .collect();
        store.replace("a.disk", &five, &[], NOW);
        store.replace(
            "b.memory",
            &[attention("w", Severity::Watch, NOW)],
            &[],
            NOW,
        );
        let four: Vec<_> = (0..4)
            .map(|i| attention(&format!("e{i}"), Severity::Watch, NOW - 100 - i))
            .collect();
        store.replace("c.events", &four, &[], NOW);
        let shown = view(&store, NOW);
        assert_eq!(
            ids(&shown.attention),
            [
                "a.disk:p0",
                "a.disk:p1",
                "a.disk:p2",
                "b.memory:w",
                "c.events:e0",
                "c.events:e1",
                "c.events:e2",
            ]
        );
        assert_eq!(
            shown.more,
            [("a.disk".to_string(), 2), ("c.events".to_string(), 1)]
        );
        // The cap applies to attention only.
        let oks: Vec<_> = (0..10).map(|i| ok(&format!("ok{i}"))).collect();
        store.replace("b.memory", &oks, &[], NOW);
        assert_eq!(view(&store, NOW).checked.len(), 10);
    }

    #[test]
    fn needs_attention_is_ranked_current_problem_newest_title_id() {
        let mut store = Store::default();
        store.replace(
            "a.disk",
            &[
                attention("old-problem", Severity::Problem, NOW - 1000),
                attention("watch-new", Severity::Watch, NOW),
                attention("problem-older", Severity::Problem, NOW - 20),
            ],
            &[],
            NOW,
        );
        store.replace(
            "b.memory",
            &[
                // Same time and severity as Disk Manager's "problem-older": title decides.
                attention("problem-same", Severity::Problem, NOW - 20),
                attention("problem-new", Severity::Problem, NOW - 10),
            ],
            &[],
            NOW,
        );
        store.replace(
            "c.events",
            &[
                // Same time, severity and add-on as each other: the id decides.
                attention("z", Severity::Watch, NOW - 50),
                attention("y", Severity::Watch, NOW - 50),
                // Out of date: after every current one, even as a Problem.
                attention("old-watch", Severity::Watch, NOW - 2000),
            ],
            &[],
            NOW,
        );
        let shown = view(&store, NOW);
        assert_eq!(
            ids(&shown.attention),
            [
                // Current Problems, newest first; at the same time "Disk Manager" < "Memory".
                "b.memory:problem-new",
                "a.disk:problem-older",
                "b.memory:problem-same",
                // Current Watches, newest first; then by id.
                "a.disk:watch-new",
                "c.events:y",
                "c.events:z",
                // Out of date: Problem before Watch.
                "a.disk:old-problem",
                "c.events:old-watch",
            ]
        );
        assert!(shown.more.is_empty());
        assert!(shown.checked.is_empty());
    }

    #[test]
    fn also_checked_is_ranked_not_checked_unavailable_old_ok_ok_then_title() {
        let mut store = Store::default();
        let not_checked = |id: &str, title: &str| {
            Finding::not_checked(id, title, "Not yet")
                .measured_at(NOW)
                .stale_after(600)
        };
        let unavailable = |id: &str, title: &str| {
            Finding::unavailable(id, title, "Needs administrator rights")
                .measured_at(NOW)
                .stale_after(600)
        };
        let ok_titled = |id: &str, title: &str, at: u64| {
            Finding::ok(id, title, "Seen")
                .measured_at(at)
                .stale_after(600)
        };
        store.replace(
            "a.disk",
            &[
                ok_titled("ok-b", "B fine", NOW),
                ok_titled("ok-a", "A fine", NOW),
                ok_titled("ok-old", "Z old", NOW - 1000),
                unavailable("unavailable", "Drive health of D:"),
                attention("attention", Severity::Watch, NOW),
            ],
            &[],
            NOW,
        );
        store.replace(
            "c.events",
            &[
                not_checked("crashes", "Recent crashes"),
                ok_titled("boot", "A boot", NOW - 1000),
                // Out of date, but not OK: stays in its own group.
                unavailable("log", "Boot log").measured_at(NOW - 9999),
            ],
            &[],
            NOW,
        );
        store.replace(
            "b.memory",
            &[
                not_checked("pressure", "Memory pressure"),
                ok_titled("ok", "A memory", NOW),
            ],
            &[],
            NOW,
        );
        let shown = view(&store, NOW);
        assert_eq!(ids(&shown.attention), ["a.disk:attention"]);
        assert_eq!(
            ids(&shown.checked),
            [
                // Not checked, by source title: Event Watch < Memory Cleaner.
                "c.events:crashes",
                "b.memory:pressure",
                // Unavailable: Disk Manager < Event Watch.
                "a.disk:unavailable",
                "c.events:log",
                // Out-of-date OK: Disk Manager < Event Watch.
                "a.disk:ok-old",
                "c.events:boot",
                // Current OK: by source title, then title.
                "a.disk:ok-a",
                "a.disk:ok-b",
                "b.memory:ok",
            ]
        );
    }

    #[test]
    fn severity_is_ignored_outside_attention() {
        let mut store = Store::default();
        let mut problem_ok = ok("b");
        problem_ok.severity = Severity::Problem;
        store.replace("a.disk", &[problem_ok, ok("a")], &[], NOW);
        let shown = view(&store, NOW);
        assert!(shown.attention.is_empty());
        let titles: Vec<_> = shown
            .checked
            .iter()
            .map(|row| row.finding.title.as_str())
            .collect();
        assert_eq!(titles, ["Title a", "Title b"]);
    }
}
