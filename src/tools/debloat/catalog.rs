//! The static table of things the debloater can turn off.
#![allow(dead_code)]

use super::model::{Edition, Group, Hive, Item, RegSetting, Risk};

const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const DSH: &str = r"SOFTWARE\Policies\Microsoft\Dsh";
const PAINT: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\Paint";
const WINDOWS_AI: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI";
const DATA_COLLECTION: &str = r"SOFTWARE\Policies\Microsoft\Windows\DataCollection";
const SYSTEM_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\System";
const EDGE: &str = r"SOFTWARE\Policies\Microsoft\Edge";
const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

const fn user(path: &'static str, name: &'static str, off_value: u32) -> RegSetting {
    RegSetting {
        hive: Hive::CurrentUser,
        path,
        name,
        off_value,
    }
}

const fn machine(path: &'static str, name: &'static str, off_value: u32) -> RegSetting {
    RegSetting {
        hive: Hive::LocalMachine,
        path,
        name,
        off_value,
    }
}

static ITEMS: &[Item] = &[
    Item {
        id: "start-promos",
        group: Group::PrivacyAds,
        title: "Start menu suggestions and promoted apps",
        what_changes: "Stops Windows from showing suggested and promoted apps in the Start menu.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            user(CDM, "SubscribedContent-338388Enabled", 0),
            user(CDM, "SystemPaneSuggestionsEnabled", 0),
            user(CDM, "SilentInstalledAppsEnabled", 0),
        ],
    },
    Item {
        id: "lock-screen-tips",
        group: Group::PrivacyAds,
        title: "Lock screen tips and ads",
        what_changes: "Hides tips, tricks and ads on the lock screen.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            user(CDM, "RotatingLockScreenOverlayEnabled", 0),
            user(CDM, "SubscribedContent-338387Enabled", 0),
        ],
    },
    Item {
        id: "settings-suggestions",
        group: Group::PrivacyAds,
        title: "Suggestions in Settings",
        what_changes: "Hides suggested content and offers (such as Microsoft 365) in the Settings app.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            user(CDM, "SubscribedContent-338393Enabled", 0),
            user(CDM, "SubscribedContent-353694Enabled", 0),
            user(CDM, "SubscribedContent-353696Enabled", 0),
        ],
    },
    Item {
        id: "suggested-notifications",
        group: Group::PrivacyAds,
        title: "Tip and suggestion notifications",
        what_changes: "Stops pop-up notifications that suggest ways to use Windows.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            user(CDM, "SubscribedContent-338389Enabled", 0),
            user(CDM, "SoftLandingEnabled", 0),
        ],
    },
    Item {
        id: "welcome-experience",
        group: Group::PrivacyAds,
        title: "Welcome screen after updates",
        what_changes: "Stops the extra welcome and tips page that can appear after an update.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[user(CDM, "SubscribedContent-310093Enabled", 0)],
    },
    Item {
        id: "bing-search",
        group: Group::PrivacyAds,
        title: "Web results in Start search",
        what_changes: "Start search only looks at your PC and no longer sends what you type to Bing.",
        could_break: "Web suggestions no longer appear when you search from Start.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            // verify on build 26300
            user(
                r"Software\Microsoft\Windows\CurrentVersion\Search",
                "BingSearchEnabled",
                0,
            ),
            user(
                r"Software\Policies\Microsoft\Windows\Explorer",
                "DisableSearchBoxSuggestions",
                1,
            ),
        ],
    },
    Item {
        id: "advertising-id",
        group: Group::PrivacyAds,
        title: "Advertising ID",
        what_changes: "Stops apps from using a personal ad ID to show you targeted ads.",
        could_break: "Nothing noticeable. You may see less relevant ads in some apps.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[user(
            r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            "Enabled",
            0,
        )],
    },
    Item {
        id: "tailored-experiences",
        group: Group::PrivacyAds,
        title: "Tailored experiences",
        what_changes: "Stops Windows from using your diagnostic data to suggest tips and ads.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[user(
            r"Software\Microsoft\Windows\CurrentVersion\Privacy",
            "TailoredExperiencesWithDiagnosticDataEnabled",
            0,
        )],
    },
    Item {
        id: "taskbar-widgets",
        group: Group::PrivacyAds,
        title: "Widgets button on the taskbar",
        what_changes: "Removes the Widgets (news and weather) button from the taskbar.",
        could_break: "The Widgets panel is no longer one click away. You can turn it back on at any time.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        // verify on build 26300
        settings: &[user(ADVANCED, "TaskbarDa", 0)],
    },
    Item {
        id: "taskbar-buttons",
        group: Group::PrivacyAds,
        title: "Chat, Task View and Copilot taskbar buttons",
        what_changes: "Removes the Chat, Task View and Copilot buttons from the taskbar.",
        could_break: "These features are still available by other routes, such as keyboard shortcuts.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        // verify on build 26300
        settings: &[
            user(ADVANCED, "TaskbarMn", 0),
            user(ADVANCED, "ShowTaskViewButton", 0),
            user(ADVANCED, "ShowCopilotButton", 0),
        ],
    },
    Item {
        id: "game-bar-capture",
        group: Group::PrivacyAds,
        title: "Game Bar capture overlay",
        what_changes: "Turns off background game capture and the Game Bar pop-ups. Xbox itself stays installed.",
        could_break: "Recording a clip with the Game Bar will not work until you turn this back on.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            user(
                r"Software\Microsoft\Windows\CurrentVersion\GameDVR",
                "AppCaptureEnabled",
                0,
            ),
            user(r"System\GameConfigStore", "GameDVR_Enabled", 0),
        ],
    },
    Item {
        id: "consumer-features",
        group: Group::PrivacyAds,
        title: "Automatically installed suggested apps",
        what_changes: "Tells Windows not to install promoted apps and shortcuts on its own for every user of this PC.",
        could_break: "Nothing noticeable. Only takes effect on Enterprise and Education editions.",
        risk: Risk::Careful,
        min_edition: Edition::Enterprise,
        settings: &[machine(
            r"SOFTWARE\Policies\Microsoft\Windows\CloudContent",
            "DisableWindowsConsumerFeatures",
            1,
        )],
    },
    Item {
        id: "activity-history",
        group: Group::PrivacyAds,
        title: "Activity history",
        what_changes: "Stops Windows from collecting and syncing a list of the apps and files you open, for every user of this PC.",
        could_break: "The Timeline-style \"pick up where you left off\" features will not have history to show.",
        risk: Risk::Careful,
        min_edition: Edition::Pro,
        settings: &[
            machine(
                r"SOFTWARE\Policies\Microsoft\Windows\System",
                "PublishUserActivities",
                0,
            ),
            machine(
                r"SOFTWARE\Policies\Microsoft\Windows\System",
                "UploadUserActivities",
                0,
            ),
        ],
    },
    Item {
        id: "widgets-policy",
        group: Group::PrivacyAds,
        title: "Widgets (news and weather board)",
        what_changes: "Turns the Widgets feature off for every user of this PC, including the taskbar news and weather.",
        could_break: "The Widgets panel is gone until you turn this back on.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[machine(DSH, "AllowNewsAndInterests", 0)],
    },
    Item {
        id: "paint-ai",
        group: Group::PrivacyAds,
        title: "AI features in Paint",
        what_changes: "Turns off Cocreator, Generative fill and Image creator in Paint.",
        could_break: "Nothing noticeable. Paint works as before without the AI tools.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[
            machine(PAINT, "DisableCocreator", 1),
            machine(PAINT, "DisableGenerativeFill", 1),
            machine(PAINT, "DisableImageCreator", 1),
        ],
    },
    Item {
        id: "recall-remove",
        group: Group::PrivacyAds,
        title: "Remove Recall",
        what_changes: "Removes Recall (the feature that takes snapshots of your screen) and deletes the snapshots it already saved. Only matters on Copilot+ PCs.",
        could_break: "Recall is gone and its saved snapshots are deleted for good. toolboks cannot bring them back, even if you undo this. A restart is needed.",
        risk: Risk::Careful,
        min_edition: Edition::Pro,
        settings: &[machine(WINDOWS_AI, "AllowRecallEnablement", 0)],
    },
    Item {
        id: "recall-snapshots",
        group: Group::PrivacyAds,
        title: "Stop Recall saving snapshots",
        what_changes: "Stops Windows from saving new screen snapshots for Recall. Recall itself stays installed. Only matters on Copilot+ PCs.",
        could_break: "Snapshots Recall already saved are deleted and toolboks cannot bring them back. No new ones are made.",
        risk: Risk::Careful,
        min_edition: Edition::Pro,
        settings: &[machine(WINDOWS_AI, "DisableAIDataAnalysis", 1)],
    },
    Item {
        id: "click-to-do",
        group: Group::PrivacyAds,
        title: "Click to Do",
        what_changes: "Turns off Click to Do, the AI actions that appear on text and images on your screen.",
        could_break: "Click to Do shortcuts stop working. Microsoft lists this setting only for Windows Insider builds, so it may do nothing on regular Windows 11.",
        risk: Risk::Careful,
        min_edition: Edition::Pro,
        // verify on build 26300
        settings: &[machine(WINDOWS_AI, "DisableClickToDo", 1)],
    },
    Item {
        id: "diagnostic-data-minimum",
        group: Group::PrivacyAds,
        title: "Send only required diagnostic data",
        what_changes: "Sets Windows diagnostic data to the lowest level allowed on Home and Pro (Required only), for every user of this PC.",
        could_break: "Nothing noticeable. Optional diagnostic settings in Windows are greyed out.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[machine(DATA_COLLECTION, "AllowTelemetry", 1)],
    },
    Item {
        id: "feedback-notifications",
        group: Group::PrivacyAds,
        title: "Feedback pop-ups",
        what_changes: "Stops Windows from asking you for feedback.",
        could_break: "Nothing noticeable.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[machine(DATA_COLLECTION, "DoNotShowFeedbackNotifications", 1)],
    },
    Item {
        id: "diagnostic-logs-limit",
        group: Group::PrivacyAds,
        title: "Limit diagnostic logs and crash dumps",
        what_changes: "Limits how much log and crash data Windows collects for Microsoft.",
        could_break: "Nothing noticeable. Microsoft has less detail if you ask them for help.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[
            machine(DATA_COLLECTION, "LimitDiagnosticLogCollection", 1),
            machine(DATA_COLLECTION, "LimitDumpCollection", 1),
        ],
    },
    Item {
        id: "onesettings-downloads",
        group: Group::PrivacyAds,
        title: "Settings pushed by Microsoft",
        what_changes: "Stops Windows from downloading configuration changes that Microsoft sends to your PC in the background.",
        could_break: "Microsoft can no longer adjust some Windows features remotely. The exact effect on staged updates is not confirmed.",
        risk: Risk::Careful,
        min_edition: Edition::Pro,
        settings: &[machine(DATA_COLLECTION, "DisableOneSettingsDownloads", 1)],
    },
    Item {
        id: "advertising-id-policy",
        group: Group::PrivacyAds,
        title: "Advertising ID for every user",
        what_changes: "Stops apps from using an ad ID for every user of this PC, and stops users from turning it back on.",
        could_break: "Nothing noticeable. You may see less relevant ads in some apps.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[machine(
            r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo",
            "DisabledByGroupPolicy",
            1,
        )],
    },
    Item {
        id: "clipboard-history",
        group: Group::PrivacyAds,
        title: "Clipboard history",
        what_changes: "Turns off the clipboard history list (Win+V) for every user of this PC.",
        could_break: "Win+V no longer shows things you copied earlier. Only the latest copy is kept.",
        risk: Risk::Careful,
        min_edition: Edition::Pro,
        settings: &[machine(SYSTEM_POLICY, "AllowClipboardHistory", 0)],
    },
    Item {
        id: "clipboard-cloud-sync",
        group: Group::PrivacyAds,
        title: "Clipboard sync between devices",
        what_changes: "Stops your clipboard from being shared with your other Windows devices.",
        could_break: "Nothing noticeable unless you copy on one device and paste on another.",
        risk: Risk::Safe,
        min_edition: Edition::Pro,
        settings: &[machine(SYSTEM_POLICY, "AllowCrossDeviceClipboard", 0)],
    },
    Item {
        id: "edge-shopping-sidebar",
        group: Group::PrivacyAds,
        title: "Edge shopping, rewards and sidebar",
        what_changes: "Hides the Edge sidebar and stops Edge from showing shopping helpers and Microsoft Rewards prompts.",
        could_break: "Nothing noticeable. Edge will say it is managed by your organization.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            machine(EDGE, "HubsSidebarEnabled", 0),
            machine(EDGE, "EdgeShoppingAssistantEnabled", 0),
            machine(EDGE, "ShowMicrosoftRewards", 0),
        ],
    },
    Item {
        id: "edge-background",
        group: Group::PrivacyAds,
        title: "Edge background running",
        what_changes: "Stops Edge from starting early and from staying active in the background after you close it.",
        could_break: "The first time you open Edge after a restart it opens a little slower.",
        risk: Risk::Careful,
        min_edition: Edition::Home,
        settings: &[
            machine(EDGE, "StartupBoostEnabled", 0),
            machine(EDGE, "BackgroundModeEnabled", 0),
        ],
    },
    Item {
        id: "edge-recommendations",
        group: Group::PrivacyAds,
        title: "Edge recommendations and nags",
        what_changes: "Hides recommended content on the Edge new tab page, stops personalised usage reports and stops Edge asking to be your default browser.",
        could_break: "Nothing noticeable. Edge will say it is managed by your organization.",
        risk: Risk::Safe,
        min_edition: Edition::Home,
        settings: &[
            machine(EDGE, "ShowRecommendationsEnabled", 0),
            machine(EDGE, "PersonalizationReportingEnabled", 0),
            machine(EDGE, "DefaultBrowserSettingEnabled", 0),
        ],
    },
];

/// All known items. Phase 1 only has the privacy and ads items (`Group::PrivacyAds`).
pub fn items() -> &'static [Item] {
    ITEMS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_kebab_case() {
        let mut seen = HashSet::new();
        for item in items() {
            assert!(seen.insert(item.id), "duplicate id {}", item.id);
            assert!(!item.id.is_empty());
            assert!(item
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        }
    }

    #[test]
    fn texts_are_filled_in() {
        for item in items() {
            assert!(!item.title.is_empty(), "{}", item.id);
            assert!(!item.what_changes.is_empty(), "{}", item.id);
            assert!(!item.could_break.is_empty(), "{}", item.id);
        }
    }

    #[test]
    fn settings_are_filled_in() {
        for item in items() {
            assert!(!item.settings.is_empty(), "{}", item.id);
            for s in item.settings {
                assert!(!s.path.is_empty(), "{}", item.id);
                assert!(!s.name.is_empty(), "{}", item.id);
                assert!(!s.path.starts_with('\\') && !s.path.ends_with('\\'));
            }
        }
    }

    #[test]
    fn machine_settings_need_pro_and_some_are_careful() {
        for item in items() {
            if item.needs_admin() {
                // Edge reads its own policy keys on every edition; the Windows ones need Pro.
                assert!(
                    item.min_edition >= Edition::Pro || item.id.starts_with("edge-"),
                    "{}",
                    item.id
                );
            }
        }
        assert!(items().iter().any(|i| i.risk == Risk::Careful));
    }

    #[test]
    fn off_values_are_small_dwords() {
        for item in items() {
            for s in item.settings {
                assert!(s.off_value <= 3, "{} {}", item.id, s.name);
            }
        }
    }

    #[test]
    fn no_duplicate_settings_across_items() {
        let mut seen = HashSet::new();
        for item in items() {
            for s in item.settings {
                let key = (
                    s.hive == Hive::LocalMachine,
                    s.path.to_lowercase(),
                    s.name.to_lowercase(),
                );
                assert!(seen.insert(key), "{} repeats {}", item.id, s.name);
            }
        }
    }

    #[test]
    fn all_items_are_privacy_and_ads() {
        assert!(items().iter().all(|i| i.group == Group::PrivacyAds));
        assert!(items().len() >= 26);
    }
}
