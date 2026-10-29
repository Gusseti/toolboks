//! Groups processes into the colored segments of the Home "Where your memory goes" bar.

use super::stats::ProcInfo;

/// One colored piece of the "Where your memory goes" bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// Display name, e.g. "Claude", "Chrome", "Other apps", "Windows and other".
    pub name: String,
    /// Memory in bytes after scaling to fit inside the memory in use.
    pub bytes: u64,
    /// Number of processes merged into this segment (0 for the leftover part).
    pub count: usize,
    /// Share of the memory in use, 0 to 100.
    pub percent: f32,
    /// RGB color.
    pub color: [u8; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakdown {
    /// Memory in use in bytes (what 100 % of the bar means).
    pub in_use: u64,
    /// Largest first; "Other apps" and "Windows and other" come last, in that order.
    pub segments: Vec<Segment>,
}

const OTHER_COLOR: [u8; 3] = [120, 124, 132];
const WINDOWS_COLOR: [u8; 3] = [91, 96, 104];
const MAX_GROUPS: usize = 10;

const WINDOWS_NAMES: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "secure system",
    "smss",
    "csrss",
    "wininit",
    "winlogon",
    "services",
    "lsass",
    "svchost",
    "dwm",
    "explorer",
    "fontdrvhost",
    "audiodg",
    "sihost",
    "ctfmon",
    "taskhostw",
    "runtimebroker",
    "searchhost",
    "startmenuexperiencehost",
    "shellexperiencehost",
    "textinputhost",
    "conhost",
    "wmiprvse",
    "spoolsv",
    "dllhost",
    "securityhealthservice",
    "msmpeng",
    "nissrv",
];

/// Distinct colors that avoid the brand colors and the greys.
const PALETTE: [[u8; 3]; 14] = [
    [239, 71, 111],
    [255, 159, 28],
    [254, 228, 64],
    [155, 93, 229],
    [241, 91, 181],
    [181, 101, 29],
    [200, 60, 60],
    [199, 125, 255],
    [214, 51, 132],
    [154, 155, 40],
    [201, 163, 56],
    [142, 68, 173],
    [219, 112, 147],
    [0, 150, 136],
];

fn normalize(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Brand display name and color for a normalized process name.
fn alias(n: &str) -> Option<(&'static str, [u8; 3])> {
    let r = if n.starts_with("claude") {
        ("Claude", [217, 119, 87])
    } else if n == "chrome" || n == "chromium" {
        ("Chrome", [66, 133, 244])
    } else if n == "msedge" || n == "msedgewebview2" {
        ("Edge", [47, 184, 166])
    } else if n.starts_with("discord") {
        ("Discord", [88, 101, 242])
    } else if n.starts_with("steam") {
        ("Steam", [102, 192, 244])
    } else if n == "chatgpt" {
        ("ChatGPT", [16, 163, 127])
    } else if n == "node" {
        ("Node.js", [139, 195, 74])
    } else if n == "firefox" {
        ("Firefox", [255, 113, 57])
    } else if n == "spotify" {
        ("Spotify", [29, 185, 84])
    } else if n == "code" {
        ("VS Code", [59, 156, 255])
    } else if n.starts_with("battle.net") {
        ("Battle.net", [20, 142, 255])
    } else {
        return None;
    };
    Some(r)
}

/// Start position in the palette for a group name (FNV-1a over the lowercase name).
fn palette_index(name: &str) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.to_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h % PALETTE.len() as u64) as usize
}

/// Stable palette color for a group name.
pub fn color_for(name: &str) -> [u8; 3] {
    PALETTE[palette_index(name)]
}

/// Gives every non-brand group its own palette color: the hashed color is the starting point and
/// the next free palette entry is taken on a clash (also with brand and grey colors). Groups are
/// handled in name order, so the result does not depend on their sizes and colors do not flicker
/// when the size order changes between refreshes.
fn assign_unique_colors(
    groups: &mut [(String, u64, usize, [u8; 3])],
    brand: &[String],
    visible: &[usize],
) {
    let mut used: Vec<[u8; 3]> = vec![OTHER_COLOR, WINDOWS_COLOR];
    used.extend(groups.iter().filter(|g| brand.contains(&g.0)).map(|g| g.3));
    let mut generated: Vec<usize> = visible
        .iter()
        .copied()
        .filter(|&i| !brand.contains(&groups[i].0))
        .collect();
    generated.sort_by(|&a, &b| groups[a].0.cmp(&groups[b].0));
    for i in generated {
        let start = palette_index(&groups[i].0);
        let pick = (0..PALETTE.len())
            .map(|step| PALETTE[(start + step) % PALETTE.len()])
            .find(|c| !used.contains(c))
            .unwrap_or(PALETTE[start]);
        groups[i].3 = pick;
        used.push(pick);
    }
}
fn group_name(n: &str) -> String {
    let key: String = n.chars().take_while(|c| c.is_alphabetic()).collect();
    if key.chars().count() >= 4 {
        capitalize(&key)
    } else {
        capitalize(n)
    }
}

/// Groups the running processes into colored segments that fill `in_use` bytes.
pub fn breakdown(procs: &[ProcInfo], in_use: u64) -> Breakdown {
    if in_use == 0 {
        return Breakdown::default();
    }
    // (name, bytes, count, color)
    let mut groups: Vec<(String, u64, usize, [u8; 3])> = Vec::new();
    let mut win_bytes: u64 = 0;
    let mut win_count = 0usize;
    // Groups with a fixed brand color; everything else gets a palette color (see below).
    let mut brand: Vec<String> = Vec::new();
    for p in procs {
        let n = normalize(&p.name);
        if WINDOWS_NAMES.contains(&n.as_str()) {
            win_bytes = win_bytes.saturating_add(p.working_set);
            win_count += 1;
            continue;
        }
        let (name, color, is_brand) = match alias(&n) {
            Some((name, color)) => (name.to_string(), color, true),
            None => {
                let name = group_name(&n);
                let color = color_for(&name);
                (name, color, false)
            }
        };
        if is_brand && !brand.contains(&name) {
            brand.push(name.clone());
        }
        match groups.iter_mut().find(|g| g.0 == name) {
            Some(g) => {
                g.1 = g.1.saturating_add(p.working_set);
                g.2 += 1;
            }
            None => groups.push((name, p.working_set, 1, color)),
        }
    }

    let apps: u128 = groups.iter().map(|g| u128::from(g.1)).sum();
    let total = apps + u128::from(win_bytes);
    if total > u128::from(in_use) {
        for g in &mut groups {
            g.1 = (u128::from(g.1) * u128::from(in_use) / total) as u64;
        }
    }
    let apps_scaled: u64 = groups.iter().map(|g| g.1).sum();
    // Leftover (or the scaled Windows share plus rounding) goes to "Windows and other".
    let windows = in_use.saturating_sub(apps_scaled);

    groups.retain(|g| g.1 > 0);
    groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    // Only the groups that appear in the bar need distinct colors (hundreds of tiny groups would
    // otherwise use up the palette before the big ones are colored).
    let visible: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(i, g)| *i < MAX_GROUPS && u128::from(g.1) * 100 >= u128::from(in_use))
        .map(|(i, _)| i)
        .collect();
    assign_unique_colors(&mut groups, &brand, &visible);

    let pct = |bytes: u64| (bytes as f64 / in_use as f64 * 100.0) as f32;
    let mut segments = Vec::new();
    let mut other_bytes = 0u64;
    let mut other_count = 0usize;
    for (i, (name, bytes, count, color)) in groups.into_iter().enumerate() {
        let small = u128::from(bytes) * 100 < u128::from(in_use);
        if i >= MAX_GROUPS || small {
            other_bytes += bytes;
            other_count += count;
        } else {
            segments.push(Segment {
                name,
                bytes,
                count,
                percent: pct(bytes),
                color,
            });
        }
    }
    if other_bytes > 0 {
        segments.push(Segment {
            name: "Other apps".to_string(),
            bytes: other_bytes,
            count: other_count,
            percent: pct(other_bytes),
            color: OTHER_COLOR,
        });
    }
    if windows > 0 {
        segments.push(Segment {
            name: "Windows and other".to_string(),
            bytes: windows,
            count: win_count,
            percent: pct(windows),
            color: WINDOWS_COLOR,
        });
    }
    Breakdown { in_use, segments }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, ws: u64) -> ProcInfo {
        ProcInfo {
            pid: 1,
            name: name.to_string(),
            working_set: ws,
            private: 0,
            count: 1,
        }
    }

    fn seg<'a>(b: &'a Breakdown, name: &str) -> &'a Segment {
        b.segments.iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn generated_colors_are_unique_and_independent_of_size_order() {
        let names = [
            "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
        ];
        let procs: Vec<ProcInfo> = names
            .iter()
            .enumerate()
            .map(|(i, n)| p(&format!("{n}.exe"), 100 + i as u64 * 10))
            .collect();
        let a = breakdown(&procs, 10_000);
        let colors: Vec<[u8; 3]> = a.segments.iter().map(|s| s.color).collect();
        let mut unique = colors.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), colors.len(), "no two groups share a color");

        // Reversing the sizes must not change which group has which color.
        let swapped: Vec<ProcInfo> = names
            .iter()
            .enumerate()
            .map(|(i, n)| p(&format!("{n}.exe"), 100 + (names.len() - i) as u64 * 10))
            .collect();
        let b = breakdown(&swapped, 10_000);
        for name in names {
            let cap = capitalize(name);
            assert_eq!(seg(&a, &cap).color, seg(&b, &cap).color, "{name}");
        }
    }

    #[test]
    fn real_world_names_get_distinct_colors() {
        let names = [
            "claude.exe",
            "msedge.exe",
            "Overwatch.exe",
            "Discord.exe",
            "steamwebhelper.exe",
            "Everything.exe",
            "Battle.net.exe",
            "RazerAppEngine.exe",
            "toolboks.exe",
            "ChatGPT.exe",
            "node.exe",
        ];
        let procs: Vec<ProcInfo> = names
            .iter()
            .enumerate()
            .map(|(i, n)| p(n, 400 + i as u64 * 40))
            .collect();
        let b = breakdown(&procs, 20_000);
        let mut seen: Vec<([u8; 3], String)> = Vec::new();
        for s in &b.segments {
            if let Some((_, other)) = seen.iter().find(|(c, _)| *c == s.color) {
                panic!("{} and {other} share {:?}", s.name, s.color);
            }
            seen.push((s.color, s.name.clone()));
        }
    }

    #[test]
    fn many_tiny_groups_do_not_use_up_the_palette() {
        // Hundreds of tiny alphabetical-first groups must not cause clashes among the big ones.
        let mut procs: Vec<ProcInfo> = (0..60)
            .map(|i| p(&format!("aaa{i:02}tiny.exe"), 1))
            .collect();
        for (i, n) in [
            "Everything.exe",
            "Overwatch.exe",
            "RazerAppEngine.exe",
            "toolboks.exe",
            "Zeta.exe",
            "Yankee.exe",
        ]
        .iter()
        .enumerate()
        {
            procs.push(p(n, 2_000 + i as u64 * 100));
        }
        let b = breakdown(&procs, 20_000);
        let mut colors: Vec<[u8; 3]> = b.segments.iter().map(|s| s.color).collect();
        let n = colors.len();
        colors.sort();
        colors.dedup();
        assert_eq!(
            colors.len(),
            n,
            "visible segments must have distinct colors"
        );
    }

    #[test]
    fn claude_helpers_merge() {
        let b = breakdown(&[p("claude.exe", 100), p("Claude Helper.exe", 50)], 1000);
        let s = seg(&b, "Claude");
        assert_eq!((s.bytes, s.count, s.color), (150, 2, [217, 119, 87]));
    }

    #[test]
    fn edge_variants() {
        let b = breakdown(&[p("msedge.exe", 100), p("msedgewebview2.exe", 100)], 1000);
        assert_eq!(seg(&b, "Edge").bytes, 200);
    }

    #[test]
    fn windows_names_go_to_windows() {
        let b = breakdown(&[p("svchost.exe", 100), p("DWM.exe", 50)], 1000);
        assert_eq!(b.segments.len(), 1);
        let s = seg(&b, "Windows and other");
        assert_eq!((s.bytes, s.count), (1000, 2));
    }

    #[test]
    fn shared_prefix_merges() {
        let b = breakdown(
            &[
                p("foobar.exe", 100),
                p("foobar2.exe", 100),
                p("foobar_x.exe", 100),
            ],
            1000,
        );
        assert_eq!(seg(&b, "Foobar").count, 3);
        let b = breakdown(&[p("ab1.exe", 100), p("ab2.exe", 100)], 1000);
        assert!(b.segments.iter().any(|s| s.name == "Ab1"));
        assert!(b.segments.iter().any(|s| s.name == "Ab2"));
    }

    #[test]
    fn rollup_small_and_cap() {
        let b = breakdown(
            &[
                p("bigapp.exe", 500),
                p("tinyapp.exe", 5),
                p("minyapp.exe", 4),
            ],
            1000,
        );
        let o = seg(&b, "Other apps");
        assert_eq!((o.bytes, o.count), (9, 2));

        let names = [
            "alpha", "bravo", "delta", "echoo", "foxes", "golfs", "hotel", "india", "julia",
            "kilos", "limas", "mikes",
        ];
        let procs: Vec<ProcInfo> = names
            .iter()
            .enumerate()
            .map(|(i, n)| p(&format!("{n}.exe"), 100 - i as u64))
            .collect();
        let b = breakdown(&procs, 1200);
        let named = b
            .segments
            .iter()
            .filter(|s| s.name != "Other apps" && s.name != "Windows and other")
            .count();
        assert_eq!(named, 10);
        assert_eq!(seg(&b, "Other apps").count, 2);
    }

    #[test]
    fn scales_when_over() {
        let b = breakdown(
            &[
                p("aaaa.exe", 700),
                p("bbbb.exe", 500),
                p("svchost.exe", 300),
            ],
            600,
        );
        assert_eq!(b.segments.iter().map(|s| s.bytes).sum::<u64>(), 600);
    }

    #[test]
    fn remainder_when_under() {
        let b = breakdown(&[p("aaaa.exe", 300), p("svchost.exe", 100)], 1000);
        assert_eq!(seg(&b, "Aaaa").bytes, 300);
        let w = seg(&b, "Windows and other");
        assert_eq!((w.bytes, w.count), (700, 1));
        assert!((w.percent - 70.0).abs() < 0.01);
    }

    #[test]
    fn zero_in_use() {
        assert!(breakdown(&[p("aaaa.exe", 5)], 0).segments.is_empty());
    }

    #[test]
    fn colors_stable() {
        assert_eq!(color_for("Foobar"), color_for("foobar"));
        assert!(PALETTE.contains(&color_for("anything")));
    }

    #[test]
    fn ordering_special_last() {
        let b = breakdown(
            &[
                p("small.exe", 100),
                p("large.exe", 400),
                p("tiny.exe", 1),
                p("svchost.exe", 50),
            ],
            1000,
        );
        let names: Vec<&str> = b.segments.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Large", "Small", "Other apps", "Windows and other"]);
    }
}
