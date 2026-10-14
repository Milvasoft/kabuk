//! System figures for the bar: CPU load, used memory, network download rate and GPU
//! temperature (ARCHITECTURE §5 "Modules and data sources"). Each call is a cheap
//! kernel query; the caller decides how often to sample.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::FILETIME;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfEntry2, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

/// Idle and total CPU time since boot, in 100 ns units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Busy share between two samples, 0-100 %, rounded. `None` if no time passed.
pub fn cpu_percent(prev: CpuTimes, now: CpuTimes) -> Option<u32> {
    let total = now.total.checked_sub(prev.total)?;
    let idle = now.idle.checked_sub(prev.idle)?;
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some(((busy as f64 / total as f64) * 100.0).round().clamp(0.0, 100.0) as u32)
}

/// Bytes per second between two counter readings, in MB/s (10^6 bytes).
pub fn rate_mbs(prev: u64, now: u64, elapsed: Duration) -> Option<f64> {
    let bytes = now.checked_sub(prev)?;
    let secs = elapsed.as_secs_f64();
    (secs > 0.0).then(|| bytes as f64 / secs / 1_000_000.0)
}

fn ft(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

pub fn cpu_times() -> Option<CpuTimes> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: valid out pointers.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    // Kernel time includes idle time.
    Some(CpuTimes {
        idle: ft(idle),
        total: ft(kernel) + ft(user),
    })
}

/// Physical memory in use, in GiB.
pub fn memory_used_gib() -> Option<f64> {
    let mut m = MEMORYSTATUSEX {
        dwLength: u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).unwrap_or_default(),
        ..Default::default()
    };
    // SAFETY: correctly sized out structure.
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    Some((m.ullTotalPhys.saturating_sub(m.ullAvailPhys)) as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// `MIB_IF_ROW2.InterfaceAndOperStatusFlags` bits.
const HARDWARE_INTERFACE: u8 = 1 << 0;
const FILTER_INTERFACE: u8 = 1 << 1;
/// `IF_TYPE_SOFTWARE_LOOPBACK`.
const LOOPBACK: u32 = 24;

/// Indexes of the connected physical interfaces (no loopback, no filter drivers, no
/// virtual adapters, so nothing is counted twice). Enumerating is slow (about a
/// millisecond), so callers cache the list; see [`Sampler`].
pub fn counted_interfaces() -> Option<Vec<u32>> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table; it is freed with FreeMibTable below.
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let list = rows
            .iter()
            .filter(|r| {
                let flags = r.InterfaceAndOperStatusFlags._bitfield;
                r.OperStatus == IfOperStatusUp
                    && r.Type != LOOPBACK
                    && flags & HARDWARE_INTERFACE != 0
                    && flags & FILTER_INTERFACE == 0
            })
            .map(|r| r.InterfaceIndex)
            .collect();
        FreeMibTable(table.cast());
        Some(list)
    }
}

/// Bytes received so far by the given interfaces (a few microseconds each).
/// `None` if any of them is gone, so the caller re-enumerates.
pub fn received_bytes_of(interfaces: &[u32]) -> Option<u64> {
    let mut total = 0u64;
    for &index in interfaces {
        let mut row = MIB_IF_ROW2 {
            InterfaceIndex: index,
            ..Default::default()
        };
        // SAFETY: `row` is a valid in/out structure naming the interface by index.
        if unsafe { GetIfEntry2(&mut row) }.is_err() {
            return None;
        }
        total += row.InOctets;
    }
    Some(total)
}

/// Bytes received so far by every counted interface.
pub fn received_bytes() -> Option<u64> {
    received_bytes_of(&counted_interfaces()?)
}

/// The hottest GPU's temperature in °C, if the driver reports one (WDDM 2.4+).
pub fn gpu_temperature_c() -> Option<u32> {
    use windows::Wdk::Graphics::Direct3D::*;
    // SAFETY: two-call enumeration with a correctly sized buffer; adapters are closed.
    unsafe {
        let mut e = D3DKMT_ENUMADAPTERS2::default();
        if D3DKMTEnumAdapters2(&mut e).is_err() || e.NumAdapters == 0 {
            return None;
        }
        let mut adapters = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
        e.pAdapters = adapters.as_mut_ptr();
        if D3DKMTEnumAdapters2(&mut e).is_err() {
            return None;
        }
        adapters.truncate(e.NumAdapters as usize);
        let mut hottest: Option<u32> = None;
        for a in &adapters {
            let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
            let mut q = D3DKMT_QUERYADAPTERINFO {
                hAdapter: a.hAdapter,
                Type: KMTQAITYPE_ADAPTERPERFDATA,
                pPrivateDriverData: (&mut perf as *mut D3DKMT_ADAPTER_PERFDATA).cast(),
                PrivateDriverDataSize: u32::try_from(std::mem::size_of::<D3DKMT_ADAPTER_PERFDATA>())
                    .unwrap_or_default(),
            };
            if D3DKMTQueryAdapterInfo(&mut q).is_ok() && perf.Temperature > 0 {
                // Reported in tenths of a degree.
                let c = (perf.Temperature + 5) / 10;
                hottest = Some(hottest.map_or(c, |h: u32| h.max(c)));
            }
            let close = D3DKMT_CLOSEADAPTER { hAdapter: a.hAdapter };
            let _ = D3DKMTCloseAdapter(&close);
        }
        hottest
    }
}

/// How long the list of counted network interfaces is reused.
pub const INTERFACE_REFRESH: Duration = Duration::from_secs(60);

/// Which figures to read; the others are skipped (their modules are off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wants {
    pub cpu: bool,
    pub ram: bool,
    pub net: bool,
    pub gpu: bool,
}

impl Wants {
    pub const ALL: Wants = Wants {
        cpu: true,
        ram: true,
        net: true,
        gpu: true,
    };
}

/// Keeps the previous readings so each sample yields rates.
#[derive(Debug, Default)]
pub struct Sampler {
    cpu: Option<CpuTimes>,
    net: Option<(u64, Instant)>,
    interfaces: Option<(Vec<u32>, Instant)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub cpu: Option<u32>,
    pub ram_gib: Option<f64>,
    pub net_mbs: Option<f64>,
    pub gpu_c: Option<u32>,
}

impl Sampler {
    pub fn sample(&mut self, wants: Wants) -> Sample {
        let cpu = if wants.cpu {
            let now = cpu_times();
            let pct = match (self.cpu, now) {
                (Some(prev), Some(now)) => cpu_percent(prev, now),
                _ => None,
            };
            self.cpu = now.or(self.cpu);
            pct
        } else {
            self.cpu = None;
            None
        };
        let net_mbs = if wants.net {
            self.network()
        } else {
            self.net = None;
            None
        };
        Sample {
            cpu,
            ram_gib: if wants.ram { memory_used_gib() } else { None },
            net_mbs,
            gpu_c: if wants.gpu { gpu_temperature_c() } else { None },
        }
    }

    fn network(&mut self) -> Option<f64> {
        let stale = self
            .interfaces
            .as_ref()
            .is_none_or(|(_, at)| at.elapsed() >= INTERFACE_REFRESH);
        if stale {
            let list = counted_interfaces().unwrap_or_default();
            let changed = self.interfaces.as_ref().is_some_and(|(old, _)| *old != list);
            self.interfaces = Some((list, Instant::now()));
            if changed {
                // A different set of interfaces: the totals are not comparable.
                self.net = None;
            }
        }
        let list = self.interfaces.as_ref().map(|(l, _)| l.clone()).unwrap_or_default();
        let Some(bytes) = received_bytes_of(&list) else {
            // An interface went away: enumerate again next time.
            self.interfaces = None;
            self.net = None;
            return None;
        };
        let now = (bytes, Instant::now());
        let rate = self
            .net
            .and_then(|(pb, pt)| rate_mbs(pb, now.0, now.1.duration_since(pt)));
        self.net = Some(now);
        rate
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no elapsed time, counters going backwards (wrap or reset), fully
    //! idle and fully busy intervals, rounding; live readings are plausible.
    use super::*;

    #[test]
    fn cpu_percent_math() {
        let a = CpuTimes {
            idle: 1000,
            total: 2000,
        };
        assert_eq!(
            cpu_percent(
                a,
                CpuTimes {
                    idle: 1750,
                    total: 3000
                }
            ),
            Some(25)
        );
        assert_eq!(
            cpu_percent(
                a,
                CpuTimes {
                    idle: 2000,
                    total: 3000
                }
            ),
            Some(0)
        );
        assert_eq!(
            cpu_percent(
                a,
                CpuTimes {
                    idle: 1000,
                    total: 3000
                }
            ),
            Some(100)
        );
        assert_eq!(cpu_percent(a, a), None);
        assert_eq!(cpu_percent(a, CpuTimes { idle: 0, total: 0 }), None);
        assert_eq!(
            cpu_percent(
                a,
                CpuTimes {
                    idle: 1995,
                    total: 3000
                }
            ),
            Some(1)
        ); // 0.5 rounds up
    }

    #[test]
    fn rates() {
        assert_eq!(rate_mbs(0, 4_200_000, Duration::from_secs(2)), Some(2.1));
        assert_eq!(rate_mbs(10, 5, Duration::from_secs(2)), None);
        assert_eq!(rate_mbs(0, 5, Duration::ZERO), None);
    }

    #[test]
    fn live_readings_are_plausible() {
        let mut s = Sampler::default();
        let first = s.sample(Wants::ALL);
        assert_eq!(first.cpu, None, "the first sample has nothing to compare with");
        assert!(first.ram_gib.is_some_and(|g| g > 0.1 && g < 4096.0));
        assert!(received_bytes().is_some());
        if let Some(t) = first.gpu_c {
            assert!((1..=130).contains(&t), "{t} °C");
        }
        // Burn a little CPU so the interval is not empty.
        let until = Instant::now() + Duration::from_millis(50);
        while Instant::now() < until {
            std::hint::black_box(0u64.wrapping_add(1));
        }
        let second = s.sample(Wants {
            gpu: false,
            ..Wants::ALL
        });
        assert!(second.cpu.is_some_and(|c| c <= 100));
        assert!(second.net_mbs.is_some_and(|r| r >= 0.0));
        assert_eq!(second.gpu_c, None);
        // Switched-off figures are not read.
        let none = s.sample(Wants {
            cpu: false,
            ram: false,
            net: false,
            gpu: false,
        });
        assert_eq!(none, Sample::default());
        let interfaces = counted_interfaces().unwrap();
        assert!(received_bytes_of(&interfaces).is_some());
        assert_eq!(received_bytes_of(&[u32::MAX]), None);
    }
}
