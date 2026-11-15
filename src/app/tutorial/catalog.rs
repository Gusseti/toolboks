use toolboks_addon_sdk::TutorialStep as S;
pub(super) fn for_browser() -> Vec<S> {
    vec![
        S::new("browser", "browser-search", "Find a tool", "Search names, descriptions and publishers. This tour does not select or install anything."),
        S::new("browser", "browser-category", "Narrow the list", "Choose a category or show only installed add-ons. Clear these filters to see everything again."),
        S::new("browser", "browser-list", "Compare add-ons", "Select an add-on to see its size, version, publisher and access. Verified means it came from the checked Toolboks catalog, not that it is sandboxed."),
        S::new("browser", "browser-review", "Review before installing", "Install or Update opens a review of what each tool asks to access. Several selected tools share one review. Nothing installs until you confirm."),
        S::new("browser", "browser-sidebar", "Open tools here", "Downloads and failed installs stay in the sidebar. Open a finished tool here without leaving Toolboks. Retry a failed download or dismiss it when you are done."),
    ]
}
