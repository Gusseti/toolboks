//! The static table of things the debloater can turn off.
#![allow(dead_code)]

use super::model::{Edition, Group, Hive, Item, RegSetting, Risk};

const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
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
    fn machine_settings_mean_careful() {
        for item in items() {
            if item.needs_admin() {
                assert_eq!(item.risk, Risk::Careful, "{}", item.id);
            }
        }
        assert!(items().iter().any(|i| i.risk == Risk::Careful));
    }

    #[test]
    fn all_items_are_privacy_and_ads() {
        assert!(items().iter().all(|i| i.group == Group::PrivacyAds));
        assert!(items().len() >= 12);
    }
}
