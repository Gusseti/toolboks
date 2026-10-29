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
