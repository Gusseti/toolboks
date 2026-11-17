//! Evidence for every entry the Debloater lists: registry items, services, scheduled tasks and
//! removable apps (issue #33).
//!
//! One table keyed by the entry id, so the catalogs stay as they are. Rules:
//! - `Evidence::Unproven` where the change claims or implies a speed or performance benefit and no
//!   controlled measurement shows one. Nothing here qualifies as `Measured` or `Mixed` yet.
//! - `Evidence::NotSpeed` where the change is for privacy, fewer ads or housekeeping.
//! - Sources are only the ones in docs/RESEARCH-debloat.md, the sources already cited for the
//!   Gaming profile, and pages we read. A link we only confirmed to exist has
//!   `source_checked: false`; it is not claimed to back the sentence beyond what the research says.

use crate::model::{Evidence, EvidenceInfo};

const MS_CONNECTIONS: &str = "https://learn.microsoft.com/windows/privacy/manage-connections-from-windows-operating-system-components-to-microsoft-services";
const CSP_SYSTEM: &str =
    "https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-system";
const CSP_EXPERIENCE: &str =
    "https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-experience";
const CSP_WINDOWSAI: &str =
    "https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-windowsai";
const CSP_NEWS: &str =
    "https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-newsandinterests";
const CSP_SEARCH: &str =
    "https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-search";
const DIAG_DATA: &str = "https://learn.microsoft.com/en-us/windows/privacy/configure-windows-diagnostic-data-in-your-organization";
const EDGE_POLICY: &str = "https://learn.microsoft.com/en-us/deployedge/microsoft-edge-browser-policies/startupboostenabled";
const SERVER_SERVICES: &str = "https://learn.microsoft.com/en-us/windows-server/security/windows-services/security-guidelines-for-disabling-system-services-in-windows-server";
const SYSMAIN_BENCHMARK: &str = "https://www.windowslatest.com/2026/10/26/everyone-says-disabling-sysmain-speeds-up-windows-11-so-i-benchmarked-it-on-two-pcs-k4l9/";
const APPS_LIST: &str =
    "https://raw.githubusercontent.com/Raphire/Win11Debloat/master/Config/Apps.json";
const R_PREFS: &str =
    "docs/RESEARCH-debloat.md section 6.2 (community-documented keys, not documented by Microsoft)";
const R_UNVERIFIED_KEYS: &str =
    "docs/RESEARCH-debloat.md section 6.2 (key names not verified on this Windows build)";
const R_AI: &str = "docs/RESEARCH-debloat.md section 1, priority 5 (AI switches)";
const R_EDGE: &str = "docs/RESEARCH-debloat.md section 6.4 (Edge policy pages)";
const R_SERVICES: &str =
    "docs/RESEARCH-debloat.md section 3 (services; most rows rest on community consensus)";
const R_TASKS: &str = "docs/RESEARCH-debloat.md section 4 (observed on build 26300; the recommendation is our judgment, not a Microsoft statement)";

const fn e(
    level: Evidence,
    note: &'static str,
    source: Option<&'static str>,
    source_checked: bool,
) -> EvidenceInfo {
    EvidenceInfo {
        level,
        note,
        source,
        source_checked,
    }
}

const fn not_speed(note: &'static str, source: &'static str) -> EvidenceInfo {
    e(Evidence::NotSpeed, note, Some(source), false)
}

const fn unproven(note: &'static str, source: Option<&'static str>) -> EvidenceInfo {
    e(Evidence::Unproven, note, source, false)
}

/// A removable app that the community Win11Debloat list also names (we read that list).
const fn app() -> EvidenceInfo {
    e(
        Evidence::NotSpeed,
        "Housekeeping for an app you may not use; no speed claim is made, and the community Win11Debloat list names this package.",
        Some(APPS_LIST),
        true,
    )
}

static TABLE: &[(&str, EvidenceInfo)] = &[
    // --- Registry items (catalog.rs) -------------------------------------------------------
    ("start-promos", not_speed("Hides Start menu promotions; this is about ads, not speed, and the keys are community-documented.", R_PREFS)),
    ("lock-screen-tips", not_speed("Hides lock screen tips and ads; this is about ads, not speed, and the keys are community-documented.", R_PREFS)),
    ("settings-suggestions", not_speed("Hides suggestions in Settings; this is about ads, not speed, and the keys are community-documented.", R_PREFS)),
    ("suggested-notifications", not_speed("Stops tip pop-ups; this is about nags, not speed, and the keys are community-documented.", R_PREFS)),
    ("welcome-experience", not_speed("Stops the welcome page after updates; this is about nags, not speed, and the key is community-documented.", R_PREFS)),
    ("bing-search", not_speed("Keeps what you type in Start search on this PC; a privacy choice, not speed, and a similar search setting once broke search in another tool.", CSP_SEARCH)),
    ("advertising-id", not_speed("Stops apps using a personal ad ID; a privacy setting, not a speed setting.", MS_CONNECTIONS)),
    ("tailored-experiences", not_speed("Stops Windows using diagnostic data for tips and ads; a privacy setting, not speed, and Microsoft documents the matching policy.", CSP_EXPERIENCE)),
    ("taskbar-widgets", not_speed("Hides the Widgets button; a look-and-feel change, not a speed setting, and the key name is unverified.", R_UNVERIFIED_KEYS)),
    ("taskbar-buttons", not_speed("Hides taskbar buttons; a look-and-feel change, not a speed setting, and the key names are unverified.", R_UNVERIFIED_KEYS)),
    ("game-bar-capture", unproven("Turns off background game capture, which might help games; no controlled measurement of a gain was found.", None)),
    ("consumer-features", not_speed("Stops Windows installing promoted apps on its own; about ads, not speed, and Microsoft documents it for Enterprise and Education only.", CSP_EXPERIENCE)),
    ("activity-history", not_speed("Stops Windows collecting and syncing the apps and files you open; a privacy setting, not a speed setting.", MS_CONNECTIONS)),
    ("widgets-policy", not_speed("Turns the Widgets feature off for every user; a feature switch, not a speed setting.", CSP_NEWS)),
    ("paint-ai", not_speed("Turns off AI tools in Paint; a feature switch, not a speed setting.", R_AI)),
    ("recall-remove", not_speed("Removes Recall and deletes its snapshots; a privacy choice, not a speed setting.", CSP_WINDOWSAI)),
    ("recall-snapshots", not_speed("Stops Recall saving new screen snapshots; a privacy choice, not a speed setting.", CSP_WINDOWSAI)),
    ("click-to-do", not_speed("Turns off Click to Do; a feature switch, not a speed setting, and Microsoft lists it for Insider builds only.", CSP_WINDOWSAI)),
    ("diagnostic-data-minimum", not_speed("Sends only required diagnostic data; a privacy setting, not speed, and Required is the lowest level Microsoft allows on Home and Pro.", DIAG_DATA)),
    ("feedback-notifications", not_speed("Stops feedback pop-ups; about nags, not speed.", CSP_EXPERIENCE)),
    ("diagnostic-logs-limit", not_speed("Limits the logs and crash dumps Windows collects; a privacy setting, not a speed setting.", CSP_SYSTEM)),
    ("advertising-id-policy", not_speed("Stops the ad ID for every user; a privacy setting, not a speed setting.", MS_CONNECTIONS)),
    ("clipboard-history", not_speed("Turns off the Win+V clipboard history; a feature switch, not a speed setting.", CSP_EXPERIENCE)),
    ("clipboard-cloud-sync", not_speed("Stops sharing your clipboard with other devices; a privacy setting, not a speed setting.", MS_CONNECTIONS)),
    ("edge-shopping-sidebar", not_speed("Hides Edge shopping, rewards and the sidebar; about nags, not speed.", R_EDGE)),
    ("edge-background", unproven("Stops Edge starting early and running in the background; no controlled measurement of a gain was found, and the first Edge start after a restart is slower.", Some(EDGE_POLICY))),
    ("edge-recommendations", not_speed("Hides Edge recommendations and nags; about nags, not speed.", R_EDGE)),
    // --- Services (services.rs) ------------------------------------------------------------
    ("svc-diagtrack", not_speed("Stops the usage and telemetry service; a privacy choice, not a speed setting.", R_SERVICES)),
    ("svc-fax", not_speed("Turns off an optional feature you probably do not use; no speed claim is made.", R_SERVICES)),
    ("svc-retaildemo", not_speed("Turns off shop-demo mode; no speed claim is made.", R_SERVICES)),
    ("svc-mapsbroker", not_speed("Stops offline map updates; no speed claim is made, and Microsoft's Server guidance allows turning it off.", SERVER_SERVICES)),
    ("svc-lfsvc", not_speed("Stops the location service; a privacy choice, not a speed setting, and Microsoft's Server guidance allows turning it off.", SERVER_SERVICES)),
    ("svc-phonesvc", not_speed("Turns off phone-call support you probably do not use; no speed claim is made, and Microsoft's Server guidance allows turning it off.", SERVER_SERVICES)),
    ("svc-icssvc", not_speed("Turns off the mobile hotspot service; no speed claim is made, and Microsoft's Server guidance allows turning it off.", SERVER_SERVICES)),
    ("svc-walletservice", not_speed("Turns off an optional wallet feature; no speed claim is made.", R_SERVICES)),
    ("svc-semgrsvc", not_speed("Turns off NFC and payment support; no speed claim is made.", R_SERVICES)),
    ("svc-wecsvc", not_speed("Turns off event collection from other PCs; no speed claim is made.", R_SERVICES)),
    ("svc-sysmain", unproven("Stops app preloading; a published benchmark found no gain on a 16 GB PC and slower app launches on a 4 GB virtual machine, so no benefit is shown.", Some(SYSMAIN_BENCHMARK))),
    ("svc-spooler", unproven("Keeps printing from being ready at startup; no controlled measurement of a gain was found, and printing stops until the service starts.", Some(SERVER_SERVICES))),
    ("svc-pcasvc", not_speed("Stops the check for old programs that have problems; no speed claim is made.", SERVER_SERVICES)),
    ("svc-trkwks", not_speed("Stops tracking moved files for shortcuts; no speed claim is made.", R_SERVICES)),
    ("svc-wersvc", not_speed("Sends fewer crash reports to Microsoft; a privacy choice, not a speed setting.", R_SERVICES)),
    ("svc-xblauthmanager", unproven("Keeps Xbox sign-in from being ready at startup; no controlled measurement of a gain was found, and Microsoft's Server advice does not transfer to a gaming PC.", Some(SERVER_SERVICES))),
    ("svc-xblgamesave", unproven("Keeps Xbox cloud saves from being ready at startup; no controlled measurement of a gain was found.", Some(SERVER_SERVICES))),
    ("svc-xboxnetapisvc", unproven("Keeps the Xbox network service from being ready at startup; no controlled measurement of a gain was found.", Some(SERVER_SERVICES))),
    ("svc-xboxgipsvc", unproven("Keeps the Xbox accessory service from being ready at startup; no controlled measurement of a gain was found.", Some(SERVER_SERVICES))),
    ("svc-wsaifabricsvc", unproven("Keeps the Windows AI service from being ready at startup; no controlled measurement of a gain was found, and the only sources are forum-grade.", Some(R_SERVICES))),
    // --- Scheduled tasks (tasks.rs) --------------------------------------------------------
    ("ceip-consolidator", not_speed("Stops usage statistics upload; a privacy choice, not a speed setting.", R_TASKS)),
    ("ceip-usb", not_speed("Stops USB usage data upload; a privacy choice, not a speed setting.", R_TASKS)),
    ("siuf-dmclient", not_speed("Stops the feedback question helper; about nags, not speed.", R_TASKS)),
    ("siuf-dmclient-scenario", not_speed("Stops the feedback question downloader; about nags, not speed.", R_TASKS)),
    ("feedback-toast", not_speed("Stops the feedback pop-up; about nags, not speed.", R_TASKS)),
    ("maps-toast", not_speed("Stops Maps notifications; about nags, not speed.", R_TASKS)),
    ("wer-queue-reporting", not_speed("Stops queued crash report upload; a privacy choice, not a speed setting.", R_TASKS)),
    ("device-info", not_speed("Stops hardware details upload; a privacy choice, not a speed setting.", R_TASKS)),
    ("device-info-user", not_speed("Stops per-user hardware details upload; a privacy choice, not a speed setting.", R_TASKS)),
    ("disk-footprint-diagnostics", not_speed("Stops disk usage data collection; a privacy choice, not a speed setting.", R_TASKS)),
    ("appexp-mare-backup", unproven("Stops a background compatibility-data job that has no privacy or feature purpose, so only a resource saving could be claimed; no controlled measurement of one was found.", Some(R_TASKS))),
    ("appexp-pca-patch-db", unproven("Stops a background compatibility-database job that has no privacy or feature purpose, so only a resource saving could be claimed; no controlled measurement of one was found.", Some(R_TASKS))),
    ("appexp-startup-app", not_speed("Stops the notice about new startup apps; about nags, not speed.", R_TASKS)),
    ("autochk-proxy", not_speed("Stops a system quality data helper; a privacy choice, not a speed setting.", R_TASKS)),
    ("xbl-game-save", not_speed("Stops Xbox cloud save sync; a feature switch, not a speed setting.", R_TASKS)),
    ("xbl-game-save-logon", not_speed("Stops Xbox cloud save sync at sign-in; a feature switch, not a speed setting.", R_TASKS)),
    ("family-safety-monitor", not_speed("Stops Family Safety enforcement; a feature switch, not a speed setting.", R_TASKS)),
    ("family-safety-refresh", not_speed("Stops Family Safety refresh; a feature switch, not a speed setting.", R_TASKS)),
    // --- Removable apps (apps.rs) ----------------------------------------------------------
    ("clipchamp", app()),
    ("bing-news", app()),
    ("bing-weather", app()),
    ("solitaire", app()),
    ("tips", app()),
    ("feedback-hub", app()),
    ("skype", app()),
    ("mixed-reality", app()),
    ("cortana", app()),
    ("copilot", e(Evidence::NotSpeed, "Housekeeping for an app you may not use; no speed claim is made, and the Win11Debloat list we read does not name this package.", Some("docs/RESEARCH-debloat.md section 2, conflict 4 (Copilot app removal)"), false)),
    ("teams-personal", app()),
    ("people", app()),
    ("maps", app()),
];

/// Used when an id has no entry, which the tests prevent. The honest default is "no benefit shown".
const MISSING: EvidenceInfo = e(
    Evidence::Unproven,
    "No evidence has been recorded for this entry.",
    None,
    false,
);

/// The evidence for an entry id from any of the four catalogs.
pub fn for_id(id: &str) -> EvidenceInfo {
    TABLE
        .iter()
        .find(|(known, _)| *known == id)
        .map_or(MISSING, |(_, info)| *info)
}

/// Every id that has an entry, for the tests.
#[cfg(test)]
fn ids() -> impl Iterator<Item = &'static str> {
    TABLE.iter().map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{apps, catalog, services, tasks};
    use std::collections::HashSet;

    fn catalog_ids() -> Vec<&'static str> {
        let mut all: Vec<&'static str> = catalog::items().iter().map(|i| i.id).collect();
        all.extend(services::catalog().iter().map(|s| s.id));
        all.extend(tasks::catalog().iter().map(|t| t.id));
        all.extend(apps::curated().iter().map(|a| a.id));
        all
    }

    #[test]
    fn every_catalog_entry_has_evidence_and_no_entry_is_orphaned() {
        let wanted = catalog_ids();
        let have: HashSet<&str> = ids().collect();
        assert_eq!(
            have.len(),
            TABLE.len(),
            "duplicate id in the evidence table"
        );
        for id in &wanted {
            assert!(have.contains(id), "{id} has no evidence entry");
        }
        let wanted: HashSet<&str> = wanted.into_iter().collect();
        for id in &have {
            assert!(
                wanted.contains(id),
                "{id} is in the table but in no catalog"
            );
        }
        assert_eq!(
            wanted.len(),
            27 + services::catalog().len() + tasks::catalog().len() + apps::curated().len(),
            "an id is shared by two catalogs"
        );
    }

    #[test]
    fn every_note_is_one_filled_in_sentence() {
        for (id, info) in TABLE {
            assert!(info.note.trim().len() > 20, "{id}");
            assert!(info.note.ends_with('.'), "{id}");
            // One sentence: a full stop only at the end.
            assert_eq!(info.note.matches(". ").count(), 0, "{id} has two sentences");
        }
    }

    #[test]
    fn measured_and_mixed_need_a_source_and_none_is_invented() {
        for (id, info) in TABLE {
            if matches!(info.level, Evidence::Measured | Evidence::Mixed) {
                assert!(
                    info.source.is_some(),
                    "{id} claims evidence without a source"
                );
            }
            if let Some(source) = info.source {
                assert!(!source.is_empty(), "{id}");
                if source.starts_with("http") {
                    assert!(source.starts_with("https://"), "{id}");
                }
            }
            // A checked source needs a source.
            assert!(!info.source_checked || info.source.is_some(), "{id}");
        }
    }

    #[test]
    fn nothing_is_labelled_measured_or_mixed_without_a_measurement() {
        // No controlled measurement exists for any of these yet (issues #32 and #33).
        assert!(TABLE
            .iter()
            .all(|(_, info)| matches!(info.level, Evidence::Unproven | Evidence::NotSpeed)));
    }

    #[test]
    fn unproven_entries_are_never_on_by_default() {
        let mut seen_unproven = false;
        for (id, info) in TABLE {
            if info.level == Evidence::Unproven {
                seen_unproven = true;
                assert!(!info.level.may_default_on(), "{id}");
            }
        }
        assert!(seen_unproven);
        assert!(!Evidence::Mixed.may_default_on());
        assert!(Evidence::Measured.may_default_on());
    }

    #[test]
    fn unproven_entries_say_no_benefit_is_shown() {
        for (id, info) in TABLE {
            if info.level == Evidence::Unproven {
                assert!(
                    info.note.contains("no controlled measurement")
                        || info.note.contains("no benefit is shown"),
                    "{id}: an Unproven note must say plainly that no benefit is shown"
                );
            }
        }
    }

    #[test]
    fn lookups_go_through_each_catalog_type() {
        assert_eq!(
            catalog::items()[0].evidence(),
            for_id(catalog::items()[0].id)
        );
        assert_eq!(for_id("no-such-entry").level, Evidence::Unproven);
        assert_eq!(for_id("svc-sysmain").level, Evidence::Unproven);
        assert_eq!(for_id("advertising-id").level, Evidence::NotSpeed);
    }
}
