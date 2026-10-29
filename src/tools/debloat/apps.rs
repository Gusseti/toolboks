//! Curated consumer apps the user may remove for the current account.
#![allow(dead_code)]

use windows::core::HSTRING;
use windows::ApplicationModel::PackageSignatureKind;
use windows::Management::Deployment::PackageManager;

/// One removable app we know and describe in plain words.
#[derive(Clone, Copy, Debug)]
pub struct Curated {
    pub id: &'static str,
    pub title: &'static str,
    /// Package name (the part of the full name before the first underscore), e.g. "Clipchamp.Clipchamp".
    pub package_name: &'static str,
    pub what_changes: &'static str,
    pub could_break: &'static str,
}

/// A package installed for the current user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub full_name: String,
    pub family_name: String,
    pub name: String,
    pub publisher: String,
    pub is_framework: bool,
    /// Signed by Windows itself (SignatureKind::System).
    pub is_system: bool,
}

/// Package access behind a trait so the logic is tested with a fake. The real one uses WinRT
/// `PackageManager` and must be called from a worker thread, never the UI thread.
pub trait Packages {
    fn list(&self) -> Result<Vec<Installed>, String>;
    fn remove(&mut self, full_name: &str) -> Result<(), String>;
}

pub struct WinPackages;

fn hex(code: i32) -> String {
    format!("0x{:08X}", code as u32)
}

impl Packages for WinPackages {
    fn list(&self) -> Result<Vec<Installed>, String> {
        // RoInitialize needs the Win32_System_WinRT feature, which is not enabled. The WinRT
        // calls and `.get()` work on a fresh worker thread (implicit MTA), so we rely on that.
        let mgr =
            PackageManager::new().map_err(|e| format!("Could not open the app manager: {e}"))?;
        let found = mgr
            .FindPackagesByUserSecurityId(&HSTRING::new())
            .map_err(|e| format!("Could not list installed apps: {e}"))?;
        let text = |r: windows::core::Result<HSTRING>| r.map(|h| h.to_string()).unwrap_or_default();
        let mut out = Vec::new();
        for p in found {
            let Ok(id) = p.Id() else { continue };
            out.push(Installed {
                full_name: text(id.FullName()),
                family_name: text(id.FamilyName()),
                name: text(id.Name()),
                publisher: text(id.Publisher()),
                is_framework: p.IsFramework().unwrap_or(false),
                is_system: p
                    .SignatureKind()
                    .map(|k| k == PackageSignatureKind::System)
                    .unwrap_or(false),
            });
        }
        Ok(out)
    }

    fn remove(&mut self, full_name: &str) -> Result<(), String> {
        let fail = |code: String| {
            format!("Windows could not remove this app (error {code}). It may be in use or managed by your organisation.")
        };
        let mgr =
            PackageManager::new().map_err(|e| format!("Could not open the app manager: {e}"))?;
        let op = mgr
            .RemovePackageAsync(&HSTRING::from(full_name))
            .map_err(|e| fail(hex(e.code().0)))?;
        let result = op.join().map_err(|e| fail(hex(e.code().0)))?;
        match result.ExtendedErrorCode() {
            Ok(code) if code.0 == 0 => Ok(()),
            Ok(code) => Err(fail(hex(code.0))),
            Err(e) => Err(fail(hex(e.code().0))),
        }
    }
}

/// A curated app and whether it is installed right now.
#[derive(Clone, Debug)]
pub struct AppRow {
    pub entry: &'static Curated,
    pub installed: Option<Installed>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppRun {
    /// (curated id, full package name) of removed apps.
    pub removed: Vec<(&'static str, String)>,
    /// (curated id, plain error text).
    pub failed: Vec<(&'static str, String)>,
}

pub fn curated() -> &'static [Curated] {
    const C: &[Curated] = &[
        Curated {
            id: "clipchamp",
            title: "Clipchamp video editor",
            package_name: "Clipchamp.Clipchamp",
            what_changes: "Removes the Clipchamp video editor for your account.",
            could_break: "Nothing else depends on it. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "bing-news",
            title: "News",
            package_name: "Microsoft.BingNews",
            what_changes: "Removes the Microsoft News app.",
            could_break: "Nothing else depends on it. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "bing-weather",
            title: "Weather",
            package_name: "Microsoft.BingWeather",
            what_changes: "Removes the Weather app.",
            could_break: "Weather may stop showing in some widgets. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "solitaire",
            title: "Microsoft Solitaire Collection",
            package_name: "Microsoft.MicrosoftSolitaireCollection",
            what_changes: "Removes the Solitaire card games.",
            could_break: "Nothing else depends on it. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "tips",
            title: "Tips",
            package_name: "Microsoft.Getstarted",
            what_changes: "Removes the Tips app that shows hints about Windows.",
            could_break: "Nothing else depends on it. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "feedback-hub",
            title: "Feedback Hub",
            package_name: "Microsoft.WindowsFeedbackHub",
            what_changes: "Removes the app used to send feedback to Microsoft.",
            could_break: "You can no longer send feedback from this app. Reinstall from the Microsoft Store if needed.",
        },
        Curated {
            id: "skype",
            title: "Skype",
            package_name: "Microsoft.SkypeApp",
            what_changes: "Removes the Skype app.",
            could_break: "You cannot make Skype calls from this PC until it is reinstalled from the Microsoft Store.",
        },
        Curated {
            id: "mixed-reality",
            title: "Mixed Reality Portal",
            package_name: "Microsoft.MixedReality.Portal",
            what_changes: "Removes the Mixed Reality Portal for VR headsets.",
            could_break: "Windows Mixed Reality headsets will not work until it is reinstalled from the Microsoft Store.",
        },
        Curated {
            id: "cortana",
            title: "Cortana",
            package_name: "Microsoft.549981C3F5F10",
            what_changes: "Removes the old Cortana assistant app.",
            could_break: "Nothing else depends on it. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "copilot",
            title: "Copilot",
            package_name: "Microsoft.Copilot",
            what_changes: "Removes the Copilot app for your account.",
            could_break: "Copilot buttons may stop working. Big Windows updates can bring it back; reinstall from the Microsoft Store if needed.",
        },
        Curated {
            id: "teams-personal",
            title: "Microsoft Teams (personal chat)",
            package_name: "MicrosoftTeams",
            what_changes: "Removes the built-in personal Teams chat app. Work or school Teams is separate.",
            could_break: "Chat from the taskbar will not open. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "people",
            title: "People",
            package_name: "Microsoft.People",
            what_changes: "Removes the People contacts app.",
            could_break: "Nothing else depends on it. Reinstall from the Microsoft Store if you want it back.",
        },
        Curated {
            id: "maps",
            title: "Maps",
            package_name: "Microsoft.WindowsMaps",
            what_changes: "Removes the Windows Maps app.",
            could_break: "Offline maps stop working. Reinstall from the Microsoft Store if you want it back.",
        },
    ];
    C
}

/// Package names that are never offered or removed, whatever the user ticks.
pub fn denylist() -> &'static [&'static str] {
    &[
        "Microsoft.WindowsStore",
        "Microsoft.StorePurchaseApp",
        "Microsoft.DesktopAppInstaller",
        "Microsoft.MicrosoftEdge",
        "Microsoft.Edge",
        "Microsoft.WebView2",
        "Microsoft.SecHealthUI",
        "Microsoft.Windows.ShellExperienceHost",
        "Microsoft.Windows.StartMenuExperienceHost",
        "MicrosoftWindows.Client.",
        "Microsoft.Windows.Search",
        "Microsoft.WindowsAppRuntime",
        "Microsoft.UI.Xaml",
        "Microsoft.VCLibs",
        "Microsoft.NET",
        "Microsoft.Windows.Photos",
        "Microsoft.WindowsCalculator",
        "Microsoft.GamingServices",
        "Microsoft.XboxIdentityProvider",
        "Microsoft.Xbox.TCUI",
        "Microsoft.WindowsTerminal",
        "Microsoft.OneDrive",
        "MicrosoftCorporationII.WindowsSubsystemForLinux",
        "Microsoft.Windows.CloudExperienceHost",
        "Microsoft.AAD.BrokerPlugin",
        "Microsoft.LockApp",
        "Microsoft.AccountsControl",
        "Microsoft.Win32WebViewHost",
    ]
}

pub fn is_protected(package_name: &str) -> bool {
    if package_name.is_empty() {
        return true;
    }
    let lower = package_name.to_ascii_lowercase();
    denylist()
        .iter()
        .any(|d| lower.starts_with(&d.to_ascii_lowercase()))
}

/// Curated apps matched against the installed packages (same order as `curated()`).
pub fn rows(pkgs: &dyn Packages) -> Result<Vec<AppRow>, String> {
    let all = pkgs.list()?;
    Ok(curated()
        .iter()
        .map(|entry| AppRow {
            entry,
            installed: all
                .iter()
                .find(|p| {
                    !p.is_framework
                        && !p.is_system
                        && p.name.eq_ignore_ascii_case(entry.package_name)
                })
                .cloned(),
        })
        .collect())
}

/// Removes the ticked, installed, non-protected apps for the current user.
pub fn remove_apps(rows: &[AppRow], selected: &[&str], pkgs: &mut dyn Packages) -> AppRun {
    let mut run = AppRun::default();
    for row in rows {
        if !selected.contains(&row.entry.id) {
            continue;
        }
        let Some(inst) = &row.installed else { continue };
        if is_protected(&inst.name) || is_protected(row.entry.package_name) {
            run.failed.push((
                row.entry.id,
                "This app is protected and is never removed.".to_string(),
            ));
        } else if inst.is_framework || inst.is_system {
            run.failed.push((
                row.entry.id,
                "This is a Windows component and is never removed.".to_string(),
            ));
        } else {
            match pkgs.remove(&inst.full_name) {
                Ok(()) => run.removed.push((row.entry.id, inst.full_name.clone())),
                Err(e) => run.failed.push((row.entry.id, e)),
            }
        }
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Fake {
        items: Vec<Installed>,
        fail: HashSet<String>,
        removed: Vec<String>,
    }

    impl Packages for Fake {
        fn list(&self) -> Result<Vec<Installed>, String> {
            Ok(self.items.clone())
        }
        fn remove(&mut self, full_name: &str) -> Result<(), String> {
            if self.fail.contains(full_name) {
                return Err("nope".into());
            }
            self.removed.push(full_name.to_string());
            Ok(())
        }
    }

    fn inst(name: &str, framework: bool, system: bool) -> Installed {
        Installed {
            full_name: format!("{name}_1.0_x64__abc"),
            family_name: format!("{name}_abc"),
            name: name.to_string(),
            publisher: "CN=Test".into(),
            is_framework: framework,
            is_system: system,
        }
    }

    fn fake(items: Vec<Installed>, fail: &[&str]) -> Fake {
        Fake {
            items,
            fail: fail.iter().map(|s| s.to_string()).collect(),
            removed: vec![],
        }
    }

    fn row_of<'a>(rows: &'a [AppRow], id: &str) -> &'a AppRow {
        rows.iter().find(|r| r.entry.id == id).unwrap()
    }

    #[test]
    fn rows_match_installed_and_ignore_framework() {
        let f = fake(
            vec![
                inst("microsoft.bingnews", false, false),
                inst("Microsoft.BingWeather", true, false),
                inst("Microsoft.People", false, true),
            ],
            &[],
        );
        let r = rows(&f).unwrap();
        assert_eq!(r.len(), curated().len());
        assert!(row_of(&r, "bing-news").installed.is_some());
        assert!(row_of(&r, "bing-weather").installed.is_none());
        assert!(row_of(&r, "people").installed.is_none());
        assert!(row_of(&r, "maps").installed.is_none());
    }

    #[test]
    fn remove_ok_and_failure_collected() {
        let news = inst("Microsoft.BingNews", false, false);
        let maps = inst("Microsoft.WindowsMaps", false, false);
        let tips = inst("Microsoft.Getstarted", false, false);
        let mut f = fake(vec![news.clone(), maps.clone(), tips], &[&maps.full_name]);
        let r = rows(&f).unwrap();
        let run = remove_apps(&r, &["bing-news", "maps", "skype"], &mut f);
        assert_eq!(run.removed, vec![("bing-news", news.full_name)]);
        assert_eq!(run.failed.len(), 1);
        assert_eq!(run.failed[0].0, "maps");
        assert_eq!(f.removed.len(), 1);
    }

    #[test]
    fn protected_framework_and_system_refused_even_if_ticked() {
        let mut f = fake(vec![], &[]);
        let bad = |name: &'static str| Curated {
            id: "x",
            title: "t",
            package_name: name,
            what_changes: "w",
            could_break: "c",
        };
        let e1: &'static Curated = Box::leak(Box::new(bad("Microsoft.WindowsStore")));
        let e2: &'static Curated = Box::leak(Box::new(bad("Some.Framework")));
        let e3: &'static Curated = Box::leak(Box::new(bad("Some.SystemApp")));
        let rs = vec![
            AppRow {
                entry: e1,
                installed: Some(inst("Microsoft.WindowsStore", false, false)),
            },
            AppRow {
                entry: e2,
                installed: Some(inst("Some.Framework", true, false)),
            },
            AppRow {
                entry: e3,
                installed: Some(inst("Some.SystemApp", false, true)),
            },
        ];
        let run = remove_apps(&rs, &["x"], &mut f);
        assert!(run.removed.is_empty());
        assert_eq!(run.failed.len(), 3);
        assert!(f.removed.is_empty());
    }

    #[test]
    fn no_curated_entry_is_protected() {
        for c in curated() {
            assert!(
                !is_protected(c.package_name),
                "{} matches denylist",
                c.package_name
            );
        }
        assert!(is_protected(""));
        assert!(is_protected("microsoft.windowsstore"));
        assert!(!is_protected("Microsoft.WebExperience"));
    }

    #[test]
    fn curated_ids_unique_and_texts_filled() {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for c in curated() {
            assert!(ids.insert(c.id), "dup id {}", c.id);
            assert!(
                names.insert(c.package_name.to_ascii_lowercase()),
                "dup name {}",
                c.package_name
            );
            assert!(!c.id.is_empty() && !c.title.is_empty() && !c.package_name.is_empty());
            assert!(!c.what_changes.is_empty() && !c.could_break.is_empty());
        }
    }

    /// Read-only: lists the real packages. Run with `-- --ignored smoke_list_real_packages`.
    #[test]
    #[ignore]
    fn smoke_list_real_packages() {
        let list = WinPackages.list().unwrap();
        assert!(!list.is_empty());
    }
}
