//! Number and time formatting shared by the tools.

/// Sizes for people: "850 MB", "1.2 GB".
pub fn size_plain(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

/// A throughput for people: "0 B/s", "512 B/s", "340 KB/s", "8.4 MB/s", "12 MB/s", "1.2 GB/s".
pub fn rate(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes_per_sec;
    if b.is_nan() || b < 1.0 {
        "0 B/s".to_string()
    } else if b < KB {
        format!("{b:.0} B/s")
    } else if b < MB {
        format!("{:.0} KB/s", b / KB)
    } else if b < GB {
        let m = b / MB;
        if m >= 10.0 {
            format!("{m:.0} MB/s")
        } else {
            format!("{m:.1} MB/s")
        }
    } else {
        format!("{:.1} GB/s", b / GB)
    }
}

/// A process name for people: "chrome.exe" becomes "Chrome".
pub fn friendly_name(exe: &str) -> String {
    let stem = match exe.get(exe.len().saturating_sub(4)..) {
        Some(tail) if exe.len() > 4 && tail.eq_ignore_ascii_case(".exe") => &exe[..exe.len() - 4],
        _ => exe,
    };
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A clock speed in MHz as "3.6 GHz" (or "800 MHz" below 1 GHz).
pub fn clock(mhz: u64) -> String {
    if mhz >= 1000 {
        format!("{:.1} GHz", mhz as f64 / 1000.0)
    } else {
        format!("{mhz} MHz")
    }
}

/// A duration as the two largest units: "2d 4h", "5h 12m", "12m", "40s".
pub fn uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_plain_sizes() {
        assert_eq!(size_plain(0), "0 MB");
        assert_eq!(size_plain(850 * 1024 * 1024), "850 MB");
        assert_eq!(size_plain(1536 * 1024 * 1024), "1.5 GB");
    }

    #[test]
    fn formats_rates() {
        assert_eq!(rate(f64::NAN), "0 B/s");
        assert_eq!(rate(-5.0), "0 B/s");
        assert_eq!(rate(0.0), "0 B/s");
        assert_eq!(rate(0.5), "0 B/s");
        assert_eq!(rate(1.0), "1 B/s");
        assert_eq!(rate(512.0), "512 B/s");
        assert_eq!(rate(1024.0), "1 KB/s");
        assert_eq!(rate(340.0 * 1024.0), "340 KB/s");
        assert_eq!(rate(1024.0 * 1024.0), "1.0 MB/s");
        assert_eq!(rate(8.4 * 1024.0 * 1024.0), "8.4 MB/s");
        assert_eq!(rate(12.0 * 1024.0 * 1024.0), "12 MB/s");
        assert_eq!(rate(1.5 * 1024.0 * 1024.0 * 1024.0), "1.5 GB/s");
    }

    #[test]
    fn makes_process_names_friendly() {
        assert_eq!(friendly_name("chrome.exe"), "Chrome");
        assert_eq!(friendly_name("Code.EXE"), "Code");
        assert_eq!(friendly_name("System"), "System");
        assert_eq!(friendly_name(".exe"), ".exe");
        assert_eq!(friendly_name(""), "");
        assert_eq!(friendly_name("\u{e9}diteur.exe"), "\u{c9}diteur");
    }

    #[test]
    fn formats_clock_speeds() {
        assert_eq!(clock(3600), "3.6 GHz");
        assert_eq!(clock(4300), "4.3 GHz");
        assert_eq!(clock(800), "800 MHz");
        assert_eq!(clock(0), "0 MHz");
    }

    #[test]
    fn formats_uptime_with_two_units() {
        assert_eq!(uptime(40), "40s");
        assert_eq!(uptime(12 * 60 + 5), "12m");
        assert_eq!(uptime(5 * 3600 + 12 * 60), "5h 12m");
        assert_eq!(uptime(2 * 86_400 + 4 * 3600 + 59), "2d 4h");
        assert_eq!(uptime(0), "0s");
    }
}
