//! Live numbers about the computer: CPU, RAM, disk, network and the busiest processes.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, Task};
use parking_lot::Mutex;
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

use crate::model::NetworkUnit;

/// How many samples the charts keep.
pub const HISTORY: usize = 90;

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    /// Total CPU use, 0 to 100.
    pub cpu: f32,
    pub cores: usize,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second, none when the platform cannot say.
    pub disk_read: Option<f64>,
    pub disk_write: Option<f64>,
    pub net_down: Option<f64>,
    pub net_up: Option<f64>,
}

impl Sample {
    pub fn ram_percent(&self) -> f32 {
        if self.mem_total == 0 { 0.0 } else { self.mem_used as f32 / self.mem_total as f32 * 100.0 }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// Percent of one core, like Activity Monitor and Task Manager (can exceed 100).
    pub cpu: f32,
    /// Resident memory in bytes.
    pub rss: u64,
}

pub struct Sampler {
    sys: Option<System>,
    disks: Option<Disks>,
    networks: Option<Networks>,
    last: Instant,
}

/// macOS "Memory Used" in Activity Monitor is app + wired + compressed memory. The generic
/// "total minus free" figure is misleading there, because the system keeps caches.
#[cfg(target_os = "macos")]
fn mac_memory_used() -> Option<u64> {
    let out = std::process::Command::new("vm_stat").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let page: u64 = text
        .split("page size of ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(16384);
    let pages = |name: &str| -> u64 {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().trim_end_matches('.').parse().ok())
            .unwrap_or(0)
    };
    let used = pages("Anonymous pages")
        .saturating_sub(pages("Pages purgeable"))
        + pages("Pages wired down")
        + pages("Pages occupied by compressor");
    (used > 0).then_some(used * page)
}

#[cfg(not(target_os = "macos"))]
fn mac_memory_used() -> Option<u64> {
    None
}

/// Interfaces that are not the real connection: loopback, tunnels, bridges, virtual machines.
/// Counting them would double count, since a VPN carries the same bytes as the interface under it.
fn is_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    ["lo", "utun", "awdl", "llw", "bridge", "ap1", "gif", "stf", "anpi", "vmnet", "docker", "veth", "virbr", "tun", "tap", "vethernet", "loopback", "tailscale"]
        .iter()
        .any(|p| n == *p || n.starts_with(p))
}

impl Sampler {
    /// Any part the platform cannot give simply stays empty (shown as "n/a").
    pub fn new() -> Self {
        let sys = System::new().ok().map(|mut sys| {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            sys
        });
        Self {
            sys,
            disks: Disks::new_with_refreshed_list().ok(),
            networks: Networks::new_with_refreshed_list().ok(),
            last: Instant::now(),
        }
    }

    pub fn sample(&mut self) -> Sample {
        let elapsed = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        let mut sample = Sample::default();

        if let Some(sys) = self.sys.as_mut() {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let total = sys.total_memory();
            sample.cpu = sys.global_cpu_usage().clamp(0.0, 100.0);
            sample.cores = sys.cpus().len();
            sample.mem_total = total;
            sample.mem_used = mac_memory_used().unwrap_or_else(|| sys.used_memory()).min(total);
        }

        if let Some(disks) = self.disks.as_mut() {
            disks.refresh(true);
            let (mut read, mut write) = (0u64, 0u64);
            for disk in disks.iter() {
                let usage = disk.usage();
                read += usage.read_bytes;
                write += usage.written_bytes;
            }
            sample.disk_read = Some(read as f64 / elapsed);
            sample.disk_write = Some(write as f64 / elapsed);
        }

        if let Some(networks) = self.networks.as_mut() {
            networks.refresh(true);
            let busiest = networks
                .iter()
                .filter(|(name, _)| !is_virtual(&name.to_string_lossy()))
                .max_by_key(|(_, n)| n.received() + n.transmitted());
            if let Some((_, n)) = busiest {
                sample.net_down = Some(n.received() as f64 / elapsed);
                sample.net_up = Some(n.transmitted() as f64 / elapsed);
            }
        }
        sample
    }

    /// The busiest processes right now.
    pub fn processes(&mut self, limit: usize, by_memory: bool) -> Vec<ProcessInfo> {
        let Some(sys) = self.sys.as_mut() else { return Vec::new() };
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut list: Vec<ProcessInfo> = sys
            .processes()
            .values()
            .map(|p| ProcessInfo {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().to_string(),
                cpu: p.cpu_usage(),
                rss: p.memory(),
            })
            .collect();
        if by_memory {
            list.sort_by_key(|p| std::cmp::Reverse(p.rss));
        } else {
            list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        }
        list.truncate(limit);
        list
    }
}

/// The history of samples, filled by a task that runs for as long as the app does.
pub struct ResourceFeed {
    pub samples: VecDeque<Sample>,
    pub interval_ms: u32,
    sampler: Arc<Mutex<Sampler>>,
    _task: Option<Task<()>>,
}

impl ResourceFeed {
    pub fn new(interval_ms: u32, cx: &mut Context<Self>) -> Self {
        let sampler = Arc::new(Mutex::new(Sampler::new()));
        let task = cx.spawn({
            let sampler = sampler.clone();
            async move |this, cx| {
                loop {
                    let sampler = sampler.clone();
                    let sample = cx
                        .background_executor()
                        .spawn(async move { sampler.lock().sample() })
                        .await;
                    let next = this.update(cx, |feed, cx| {
                        feed.samples.push_back(sample);
                        while feed.samples.len() > HISTORY {
                            feed.samples.pop_front();
                        }
                        cx.notify();
                        feed.interval_ms
                    });
                    let Ok(interval) = next else { break };
                    cx.background_executor().timer(Duration::from_millis(interval as u64)).await;
                }
            }
        });
        Self { samples: VecDeque::new(), interval_ms, sampler, _task: Some(task) }
    }

    pub fn latest(&self) -> Option<Sample> {
        self.samples.back().copied()
    }
}

// ---- Numbers as text -----------------------------------------------------------------------

const BYTE_UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
const BIT_UNITS: [&str; 5] = ["b", "Kb", "Mb", "Gb", "Tb"];

fn scale(value: f64, units: &[&str; 5]) -> String {
    let (mut v, mut i) = (value, 0);
    while v >= 1000.0 && i < units.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    let digits = if v >= 100.0 || i == 0 { 0 } else if v >= 10.0 { 1 } else { 2 };
    format!("{v:.digits$} {}", units[i])
}

pub fn format_percent(v: f32) -> String {
    format!("{}%", v.round() as i32)
}

pub fn format_bytes(bytes: u64) -> String {
    scale(bytes as f64, &BYTE_UNITS)
}

/// Throughput, like "4.2 MB/s" or, in bits, "33.6 Mb/s".
pub fn format_rate(bytes_per_sec: Option<f64>, unit: NetworkUnit) -> String {
    match (bytes_per_sec, unit) {
        (None, _) => "n/a".into(),
        (Some(v), NetworkUnit::Bits) => format!("{}/s", scale(v * 8.0, &BIT_UNITS)),
        (Some(v), NetworkUnit::Bytes) => format!("{}/s", scale(v, &BYTE_UNITS)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_like_the_electron_app() {
        assert_eq!(format_percent(44.4), "44%");
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500), "1.50 KB");
        assert_eq!(format_bytes(15_000_000), "15.0 MB");
        assert_eq!(format_bytes(250_000_000_000), "250 GB");
        assert_eq!(format_rate(Some(4_200_000.0), NetworkUnit::Bytes), "4.20 MB/s");
        assert_eq!(format_rate(Some(4_200_000.0), NetworkUnit::Bits), "33.6 Mb/s");
        assert_eq!(format_rate(None, NetworkUnit::Bytes), "n/a");
    }

    #[test]
    fn virtual_interfaces_are_not_counted() {
        for name in ["lo0", "utun3", "awdl0", "bridge100", "docker0", "veth12ab"] {
            assert!(is_virtual(name), "{name}");
        }
        for name in ["en0", "en7", "eth0", "wlan0", "Wi-Fi", "Ethernet"] {
            assert!(!is_virtual(name), "{name}");
        }
    }

    #[test]
    fn the_sampler_reports_sensible_numbers() {
        let mut sampler = Sampler::new();
        std::thread::sleep(Duration::from_millis(300));
        let s = sampler.sample();
        assert!((0.0..=100.0).contains(&s.cpu));
        assert!(s.cores > 0);
        assert!(s.mem_total > 0 && s.mem_used > 0 && s.mem_used <= s.mem_total);
        let busiest = sampler.processes(5, false);
        assert!(!busiest.is_empty());
        assert!(busiest.windows(2).all(|w| w[0].cpu >= w[1].cpu));
        println!("sample: {s:?}\nprocesses: {busiest:?}");
    }
}
