//! Power: the graphics card's measured draw and an estimate for the processor.
//!
//! Windows has no safe way to read a processor's power draw (it needs a kernel driver), so the
//! processor figure is always an estimate from its load and a rated power the user can change.
//! The graphics card figure is measured by NVIDIA's driver. The two are never added into one
//! number: they are shown side by side and labelled.

/// The part of its rated power a processor draws when idle, as a fraction. A rough, typical
/// value for desktop processors; the real figure varies by model.
pub const IDLE_FRACTION: f32 = 0.15;
/// Rated power used until the user sets their own.
pub const DEFAULT_RATED_W: f32 = 65.0;

/// Estimated processor draw in watts for a load of 0 to 100 percent.
pub fn cpu_estimate_w(load_percent: f32, rated_w: f32) -> f32 {
    let load = (load_percent / 100.0).clamp(0.0, 1.0);
    rated_w.max(0.0) * (IDLE_FRACTION + (1.0 - IDLE_FRACTION) * load)
}

/// Watts to energy: the sum of the samples times the seconds between them, in watt-hours.
pub fn energy_wh(watts: &[f32], step_secs: f32) -> f32 {
    watts.iter().sum::<f32>() * step_secs / 3600.0
}

pub fn fmt_watts(v: f32) -> String {
    format!("{v:.0} W")
}

/// "12.3 Wh", or "1.25 kWh" from a thousand watt-hours up.
pub fn fmt_energy(wh: f32) -> String {
    if wh >= 1000.0 {
        format!("{:.2} kWh", wh / 1000.0)
    } else if wh >= 100.0 {
        format!("{wh:.0} Wh")
    } else {
        format!("{wh:.1} Wh")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn estimate_runs_from_idle_to_rated() {
        assert!(near(cpu_estimate_w(0.0, 100.0), 15.0));
        assert!(near(cpu_estimate_w(100.0, 100.0), 100.0));
        assert!(near(cpu_estimate_w(50.0, 100.0), 57.5));
    }

    #[test]
    fn estimate_clamps_load_and_rating() {
        assert!(near(cpu_estimate_w(250.0, 100.0), 100.0));
        assert!(near(cpu_estimate_w(-5.0, 100.0), 15.0));
        assert!(near(cpu_estimate_w(50.0, -10.0), 0.0));
    }

    #[test]
    fn average_of_estimates_is_estimate_of_average() {
        // The estimate is linear in load, so averaging load first loses nothing.
        let loads = [10.0f32, 90.0];
        let mean_of_est = (cpu_estimate_w(loads[0], 80.0) + cpu_estimate_w(loads[1], 80.0)) / 2.0;
        assert!((mean_of_est - cpu_estimate_w(50.0, 80.0)).abs() < 1e-4);
    }

    #[test]
    fn energy_is_watts_times_time() {
        // 100 W for one hour in 1 s steps.
        let w = vec![100.0f32; 3600];
        assert!((energy_wh(&w, 1.0) - 100.0).abs() < 1e-3);
        // 60 one-minute points of 60 W is 60 Wh.
        let m = vec![60.0f32; 60];
        assert!((energy_wh(&m, 60.0) - 60.0).abs() < 1e-3);
        assert_eq!(energy_wh(&[], 1.0), 0.0);
    }

    #[test]
    fn energy_text_units() {
        assert_eq!(fmt_energy(0.0), "0.0 Wh");
        assert_eq!(fmt_energy(12.34), "12.3 Wh");
        assert_eq!(fmt_energy(123.4), "123 Wh");
        assert_eq!(fmt_energy(1250.0), "1.25 kWh");
        assert_eq!(fmt_watts(57.6), "58 W");
    }
}
