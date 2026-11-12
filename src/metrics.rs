//! What the monitors show: which series each chart has, its headline text and its scale.

use crate::model::NetworkUnit;
use crate::resources::{Sample, format_bytes, format_percent, format_rate};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metric {
    Cpu,
    Ram,
    CpuRam,
    Disk,
    Net,
}

pub struct SeriesDef {
    pub label: &'static str,
    /// The first series is drawn in the accent, the second in a neutral, so no second hue is needed.
    pub primary: bool,
    pub pick: fn(&Sample) -> Option<f32>,
    pub format: fn(Option<f32>, NetworkUnit) -> String,
}

fn pct(v: Option<f32>, _: NetworkUnit) -> String {
    v.map_or("n/a".into(), format_percent)
}

fn rate(v: Option<f32>, unit: NetworkUnit) -> String {
    format_rate(v.map(f64::from), unit)
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::Cpu => "CPU",
            Metric::Ram => "RAM",
            Metric::CpuRam => "CPU and RAM",
            Metric::Disk => "Disk I/O",
            Metric::Net => "Network I/O",
        }
    }

    pub fn series(self) -> Vec<SeriesDef> {
        let cpu = |primary| SeriesDef { label: "CPU", primary, pick: |s| Some(s.cpu), format: pct };
        let ram = |primary| SeriesDef {
            label: "RAM",
            primary,
            pick: |s| (s.mem_total > 0).then(|| s.ram_percent()),
            format: pct,
        };
        match self {
            Metric::Cpu => vec![cpu(true)],
            Metric::Ram => vec![ram(true)],
            Metric::CpuRam => vec![cpu(true), ram(false)],
            Metric::Disk => vec![
                SeriesDef { label: "Read", primary: true, pick: |s| s.disk_read.map(|v| v as f32), format: rate },
                SeriesDef { label: "Write", primary: false, pick: |s| s.disk_write.map(|v| v as f32), format: rate },
            ],
            Metric::Net => vec![
                SeriesDef { label: "Down", primary: true, pick: |s| s.net_down.map(|v| v as f32), format: rate },
                SeriesDef { label: "Up", primary: false, pick: |s| s.net_up.map(|v| v as f32), format: rate },
            ],
        }
    }

    /// Percent metrics have a fixed scale; rates scale to the data.
    pub fn fixed_max(self) -> Option<f32> {
        matches!(self, Metric::Cpu | Metric::Ram | Metric::CpuRam).then_some(100.0)
    }

    /// Rate charts never zoom in below this, so a quiet connection does not look busy.
    pub fn min_max(self) -> f32 {
        match self {
            Metric::Disk => 1_000_000.0,
            Metric::Net => 100_000.0,
            _ => 100.0,
        }
    }

    /// The top of the chart for these samples.
    pub fn scale_max(self, samples: &[Sample]) -> f32 {
        if let Some(fixed) = self.fixed_max() {
            return fixed;
        }
        let peak = samples
            .iter()
            .flat_map(|s| self.series().into_iter().filter_map(|d| (d.pick)(s)))
            .fold(0.0_f32, f32::max);
        (peak * 1.15).max(self.min_max())
    }

    pub fn headline(self, s: Option<&Sample>, unit: NetworkUnit) -> String {
        let Some(s) = s else { return "n/a".into() };
        match self {
            Metric::Cpu => format_percent(s.cpu),
            Metric::Ram => format_percent(s.ram_percent()),
            Metric::CpuRam => format!("{} / {}", format_percent(s.cpu), format_percent(s.ram_percent())),
            Metric::Disk => format!("{} read", format_rate(s.disk_read, unit)),
            Metric::Net => format!("{} down", format_rate(s.net_down, unit)),
        }
    }

    pub fn detail(self, s: Option<&Sample>, unit: NetworkUnit) -> String {
        let Some(s) = s else { return String::new() };
        match self {
            Metric::Cpu => format!("{} cores", s.cores),
            Metric::Ram => format!("{} of {}", format_bytes(s.mem_used), format_bytes(s.mem_total)),
            Metric::CpuRam => format!(
                "{} cores, {} of {}",
                s.cores,
                format_bytes(s.mem_used),
                format_bytes(s.mem_total)
            ),
            Metric::Disk => format!("{} write", format_rate(s.disk_write, unit)),
            Metric::Net => format!("{} up", format_rate(s.net_up, unit)),
        }
    }

    /// The value shown next to the label in the sidebar.
    pub fn compact(self, s: Option<&Sample>, unit: NetworkUnit) -> String {
        let Some(s) = s else { return "n/a".into() };
        match self {
            Metric::Cpu => format_percent(s.cpu),
            Metric::Ram => format_percent(s.ram_percent()),
            Metric::CpuRam => format_percent(s.cpu),
            Metric::Disk => format_rate(s.disk_read.map(|r| r + s.disk_write.unwrap_or(0.0)), unit),
            Metric::Net => format_rate(s.net_down, unit),
        }
    }
}

/// The pop-out windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Summary,
    Cpu,
    CpuRam,
    Ram,
    Disk,
    Net,
    Processes,
}

pub const WIDGETS: [WidgetKind; 7] = [
    WidgetKind::Summary,
    WidgetKind::Cpu,
    WidgetKind::CpuRam,
    WidgetKind::Ram,
    WidgetKind::Disk,
    WidgetKind::Net,
    WidgetKind::Processes,
];

impl WidgetKind {
    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::Summary => "Resources summary",
            WidgetKind::Cpu => "CPU",
            WidgetKind::CpuRam => "CPU and RAM",
            WidgetKind::Ram => "RAM",
            WidgetKind::Disk => "Disk I/O",
            WidgetKind::Net => "Network I/O",
            WidgetKind::Processes => "Processes",
        }
    }

    /// Width, height and minimum height, as in the Electron app.
    pub fn size(self) -> (f32, f32, f32) {
        match self {
            WidgetKind::Summary => (340.0, 700.0, 360.0),
            WidgetKind::Cpu | WidgetKind::Ram => (320.0, 190.0, 150.0),
            WidgetKind::CpuRam | WidgetKind::Disk | WidgetKind::Net => (320.0, 210.0, 170.0),
            WidgetKind::Processes => (480.0, 560.0, 280.0),
        }
    }

    /// Which charts the window shows, top to bottom.
    pub fn metrics(self) -> Vec<Metric> {
        match self {
            WidgetKind::Summary => vec![Metric::Cpu, Metric::Ram, Metric::Disk, Metric::Net],
            WidgetKind::Cpu => vec![Metric::Cpu],
            WidgetKind::Ram => vec![Metric::Ram],
            WidgetKind::CpuRam => vec![Metric::CpuRam],
            WidgetKind::Disk => vec![Metric::Disk],
            WidgetKind::Net => vec![Metric::Net],
            WidgetKind::Processes => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            cpu: 44.4,
            cores: 10,
            mem_used: 12_000_000_000,
            mem_total: 16_000_000_000,
            disk_read: Some(2_500_000.0),
            disk_write: Some(60_000_000.0),
            net_down: Some(4_200_000.0),
            net_up: Some(1_000.0),
        }
    }

    #[test]
    fn headlines_and_details() {
        let s = sample();
        let u = NetworkUnit::Bytes;
        assert_eq!(Metric::Cpu.headline(Some(&s), u), "44%");
        assert_eq!(Metric::Cpu.detail(Some(&s), u), "10 cores");
        assert_eq!(Metric::Ram.headline(Some(&s), u), "75%");
        assert_eq!(Metric::Ram.detail(Some(&s), u), "12.0 GB of 16.0 GB");
        assert_eq!(Metric::CpuRam.headline(Some(&s), u), "44% / 75%");
        assert_eq!(Metric::Disk.headline(Some(&s), u), "2.50 MB/s read");
        assert_eq!(Metric::Net.headline(Some(&s), NetworkUnit::Bits), "33.6 Mb/s down");
        assert_eq!(Metric::Net.headline(None, u), "n/a");
    }

    #[test]
    fn charts_scale_to_the_data_but_never_below_the_minimum() {
        assert_eq!(Metric::Cpu.scale_max(&[]), 100.0);
        assert_eq!(Metric::Net.scale_max(&[]), 100_000.0);
        let quiet = Sample { net_down: Some(10.0), net_up: Some(5.0), ..sample() };
        assert_eq!(Metric::Net.scale_max(&[quiet]), 100_000.0);
        let busy = Sample { net_down: Some(10_000_000.0), ..sample() };
        assert!(Metric::Net.scale_max(&[busy]) > 10_000_000.0);
    }

    #[test]
    fn every_window_has_a_size_and_charts() {
        for kind in WIDGETS {
            let (w, h, min) = kind.size();
            assert!(w > 0.0 && h >= min);
            assert_eq!(kind.metrics().is_empty(), kind == WidgetKind::Processes);
        }
    }
}
