//! Server-side profiling for `/tps`, `/mspt` and `/profile`.
//!
//! [`Profiler`] owns the two pieces the tick loop needs: a [`TickMonitor`]
//! fed by the region and global tick loops, and a [`Timings`] tree the loops
//! enter scopes in. Command handlers read it without touching the game loop,
//! so `/tps` and `/mspt` never wait for a tick to finish.

use ferrite_profiler::paper;
use ferrite_profiler::{TickMonitor, Timings, Tps};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Region slots kept by the tick monitor.
///
/// Live regions are assigned slots round-robin; a region created after the
/// slots wrap reuses a slot whose previous owner no longer ticks.
pub const REGION_SLOTS: usize = TickMonitor::DEFAULT_REGIONS;

/// Number of scopes `/profile stop` lists.
pub const TOP_SCOPES: usize = 10;

/// How long a region may go without ticking before `/tps` stops listing it.
const REGION_STALE: Duration = Duration::from_secs(5);

/// Tick-loop profiler shared by the game loop and the command handlers.
pub struct Profiler {
    monitor: TickMonitor,
    timings: Arc<Timings>,
    next_region: AtomicU64,
    /// When each slot last ticked, so retired regions do not linger in `/tps`.
    last_region_tick: Mutex<[Option<Instant>; REGION_SLOTS]>,
    session: Mutex<Option<Session>>,
    state_lock_active: std::sync::atomic::AtomicBool,
    state_lock_calls: AtomicU64,
    state_lock_nanos: AtomicU64,
}

struct Session {
    started: Instant,
    global_ticks: u64,
}

/// One finished `/profile` period.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileReport {
    /// Wall time the profile was running.
    pub elapsed: Duration,
    /// Global ticks that ran during the period.
    pub ticks: u64,
    /// The slowest scopes by self time, most expensive first.
    pub rows: Vec<ProfileRow>,
}

/// One scope of a [`ProfileReport`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRow {
    /// Slash-separated scope path, for example `region-tick/player-update`.
    pub path: String,
    /// Number of times the scope was entered.
    pub calls: u64,
    /// Inclusive time of the scope body, including nested scopes.
    pub total_ms: f64,
    /// Exclusive time: `total_ms` minus the inclusive time of direct children.
    pub self_ms: f64,
}

impl ProfileReport {
    /// Chat lines for the report: a header, then one line per scope.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.rows.len() + 1);
        let header = format!(
            "Profile: {:.1} s, {} ticks",
            self.elapsed.as_secs_f64(),
            self.ticks
        );
        if self.rows.is_empty() {
            lines.push(header);
            lines.push("No tick scopes were recorded during the profile".into());
            return lines;
        }
        lines.push(format!(
            "{header}, slowest {} systems by self time",
            self.rows.len()
        ));
        for row in &self.rows {
            let average = if row.calls == 0 {
                0.0
            } else {
                row.total_ms / row.calls as f64
            };
            lines.push(format!(
                "{:>9.2} ms self  {:>9.2} ms total  {:>7} calls  {:>7.3} ms/call  {}",
                row.self_ms, row.total_ms, row.calls, average, row.path
            ));
        }
        lines
    }
}

impl Profiler {
    /// Creates a profiler with a fresh tick monitor and timing tree.
    pub fn new() -> Self {
        Self {
            monitor: TickMonitor::new(),
            timings: Arc::new(Timings::new()),
            next_region: AtomicU64::new(0),
            last_region_tick: Mutex::new([None; REGION_SLOTS]),
            session: Mutex::new(None),
            state_lock_active: std::sync::atomic::AtomicBool::new(false),
            state_lock_calls: AtomicU64::new(0),
            state_lock_nanos: AtomicU64::new(0),
        }
    }

    /// Tick statistics for all windows.
    pub fn monitor(&self) -> &TickMonitor {
        &self.monitor
    }

    /// Scope tree the tick loops record into.
    pub fn timings(&self) -> &Arc<Timings> {
        &self.timings
    }

    /// Returns the monitor slot for a region, allocating one on first use.
    ///
    /// `slot` is the region's own storage for its slot; it is set once per
    /// region and survives ticks. Wrapping past [`REGION_SLOTS`] clears the
    /// reused slot so the new owner starts with an empty window.
    pub fn region_slot(&self, slot: &mut Option<u32>) -> u32 {
        if let Some(existing) = *slot {
            return existing;
        }
        let sequence = self.next_region.fetch_add(1, Ordering::Relaxed);
        let index = (sequence % REGION_SLOTS as u64) as u32;
        self.monitor.reset_region(index);
        if let Some(last) = lock(&self.last_region_tick).get_mut(index as usize) {
            *last = None;
        }
        *slot = Some(index);
        index
    }

    /// Records a regional tick duration.
    pub fn record_region(&self, slot: u32, duration: Duration) {
        self.monitor.record_region(slot, duration);
        if let Some(last) = lock(&self.last_region_tick).get_mut(slot as usize) {
            *last = Some(Instant::now());
        }
    }

    /// Records the global region's tick duration.
    pub fn record_global(&self, duration: Duration) {
        self.monitor.record_global(duration);
    }

    /// `/tps` output with `§` colors, for chat.
    pub fn tps_lines(&self) -> Vec<String> {
        self.tps_lines_with(true)
    }

    /// `/tps` output without formatting codes, for the console.
    pub fn tps_lines_plain(&self) -> Vec<String> {
        self.tps_lines_with(false)
    }

    /// `/mspt` output with `§` colors, for chat.
    pub fn mspt_lines(&self) -> Vec<String> {
        self.mspt_lines_with(true)
    }

    /// `/mspt` output without formatting codes, for the console.
    pub fn mspt_lines_plain(&self) -> Vec<String> {
        self.mspt_lines_with(false)
    }

    /// Starts a profiling period, clearing the previous period's scopes.
    ///
    /// Returns an error when a period is already running.
    pub fn start_profile(&self) -> Result<(), String> {
        let mut session = lock(&self.session);
        if session.is_some() {
            return Err("a profile is already running".into());
        }
        self.timings.reset();
        self.state_lock_calls.store(0, Ordering::Relaxed);
        self.state_lock_nanos.store(0, Ordering::Relaxed);
        self.state_lock_active.store(true, Ordering::Relaxed);
        *session = Some(Session {
            started: Instant::now(),
            global_ticks: self.monitor.ticks(),
        });
        Ok(())
    }

    /// Stops the running profiling period and returns its slowest scopes.
    ///
    /// Returns an error when no period is running.
    pub fn stop_profile(&self) -> Result<ProfileReport, String> {
        let session = lock(&self.session)
            .take()
            .ok_or_else(|| "no profile is running; use /profile start".to_owned())?;
        self.state_lock_active.store(false, Ordering::Relaxed);
        let elapsed = session.started.elapsed();
        let ticks = self.monitor.ticks().saturating_sub(session.global_ticks);
        let mut rows: Vec<ProfileRow> = self
            .timings
            .report_all(50.0)
            .rows
            .into_iter()
            .map(|row| ProfileRow {
                path: row.path,
                calls: row.calls,
                total_ms: row.total_ms,
                self_ms: row.self_ms,
            })
            .collect();
        let lock_calls = self.state_lock_calls.load(Ordering::Relaxed);
        if lock_calls > 0 {
            let wait_ms = self.state_lock_nanos.load(Ordering::Relaxed) as f64 / 1_000_000.0;
            rows.push(ProfileRow {
                path: "game-state-lock-wait".into(),
                calls: lock_calls,
                total_ms: wait_ms,
                self_ms: wait_ms,
            });
        }
        rows.sort_by(|left, right| {
            right
                .self_ms
                .total_cmp(&left.self_ms)
                .then_with(|| right.total_ms.total_cmp(&left.total_ms))
        });
        rows.truncate(TOP_SCOPES);
        Ok(ProfileReport {
            elapsed,
            ticks,
            rows,
        })
    }

    /// Whether a profiling period is running.
    pub fn profile_running(&self) -> bool {
        lock(&self.session).is_some()
    }

    pub fn record_state_lock_wait(&self, wait: Duration) {
        if self.state_lock_active.load(Ordering::Relaxed) {
            self.state_lock_calls.fetch_add(1, Ordering::Relaxed);
            self.state_lock_nanos.fetch_add(
                wait.as_nanos().min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
        }
    }

    fn tps_lines_with(&self, colored: bool) -> Vec<String> {
        let global = self.monitor.tps();
        let mut lines = vec![if colored {
            paper::tps_line_colored(&global)
        } else {
            paper::tps_line(&global)
        }];
        for slot in 0..self.monitor.regions() as u32 {
            if !self.region_is_current(slot) {
                continue;
            }
            let Some(tps) = self.region_tps(slot) else {
                continue;
            };
            lines.push(if colored {
                paper::region_tps_line_colored(slot, &tps)
            } else {
                paper::region_tps_line(slot, &tps)
            });
        }
        lines
    }

    /// Whether the slot recorded a tick recently enough to be shown.
    fn region_is_current(&self, slot: u32) -> bool {
        lock(&self.last_region_tick)
            .get(slot as usize)
            .and_then(|last| *last)
            .is_some_and(|last| last.elapsed() <= REGION_STALE)
    }

    fn region_tps(&self, slot: u32) -> Option<Tps> {
        if self.monitor.region_ticks(slot)? == 0 {
            return None;
        }
        self.monitor.tps_of(slot)
    }

    fn mspt_lines_with(&self, colored: bool) -> Vec<String> {
        let mspt = self.monitor.mspt();
        let mut lines = Vec::with_capacity(2);
        for (window, stats) in [("10s", &mspt.last_10s), ("1m", &mspt.last_1m)] {
            lines.push(if colored {
                paper::mspt_line_colored(stats, window)
            } else {
                paper::mspt_line(stats, window)
            });
        }
        lines
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn tps_lines_cover_the_global_tick_and_active_regions() {
        let profiler = Profiler::new();
        assert_eq!(profiler.tps_lines_plain().len(), 1);
        profiler.record_region(2, Duration::from_millis(50));

        let lines = profiler.tps_lines_plain();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0], "TPS from last 1m, 5m, 15m: 20.0, 20.0, 20.0");
        assert_eq!(
            lines[1],
            "Region 2 TPS from last 5s, 1m, 5m: 20.0, 20.0, 20.0"
        );

        let colored = profiler.tps_lines();
        assert!(colored[0].starts_with("§6TPS"), "{colored:?}");
        assert!(colored[1].starts_with("§6Region 2"), "{colored:?}");
    }

    #[test]
    fn tps_does_not_list_regions_without_ticks() {
        let profiler = Profiler::new();
        profiler.record_region(1, Duration::from_millis(100));

        let lines = profiler.tps_lines_plain();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[1].starts_with("Region 1 "), "{lines:?}");
    }

    #[test]
    fn mspt_lines_cover_both_windows() {
        let profiler = Profiler::new();
        profiler.record_global(Duration::from_millis(10));

        let lines = profiler.mspt_lines_plain();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with("MSPT from last 10s: avg 10.00"),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with("MSPT from last 1m: avg 10.00"),
            "{lines:?}"
        );

        let colored = profiler.mspt_lines();
        assert!(
            colored[0].starts_with("§6MSPT from last 10s: "),
            "{colored:?}"
        );
    }

    #[test]
    fn region_slots_are_stable_and_reset_after_wrapping() {
        let profiler = Profiler::new();
        let mut first = None;
        let mut second = None;
        assert_eq!(profiler.region_slot(&mut first), 0);
        assert_eq!(profiler.region_slot(&mut second), 1);
        assert_eq!(profiler.region_slot(&mut first), 0);

        profiler.record_region(0, Duration::from_millis(50));
        assert_eq!(profiler.monitor().region_ticks(0), Some(1));

        for _ in 2..REGION_SLOTS {
            let mut slot = None;
            profiler.region_slot(&mut slot);
        }
        let mut wrapped = None;
        assert_eq!(profiler.region_slot(&mut wrapped), 0);
        assert_eq!(
            profiler.monitor().region_ticks(0),
            Some(0),
            "the reused slot starts empty"
        );
        assert_eq!(
            profiler.tps_lines_plain().len(),
            1,
            "a reused slot is not listed until it ticks again"
        );
    }

    #[test]
    fn tps_hides_regions_that_stopped_ticking() {
        let profiler = Profiler::new();
        profiler.record_region(3, Duration::from_millis(50));
        assert_eq!(profiler.tps_lines_plain().len(), 2);

        {
            let mut last = lock(&profiler.last_region_tick);
            last[3] = Some(Instant::now() - REGION_STALE - Duration::from_secs(1));
        }
        assert_eq!(profiler.tps_lines_plain().len(), 1);
    }

    #[test]
    fn profile_session_reports_the_slowest_scopes() {
        let profiler = Profiler::new();
        {
            let _before = profiler.timings().scope("before-start");
        }

        profiler.start_profile().unwrap();
        {
            let _slow = profiler.timings().scope("slow");
            // Windows' scheduler granularity is ~15 ms, and a busy test run can deschedule
            // a thread for tens of milliseconds; keep the two scopes far enough apart that
            // their order cannot invert.
            thread::sleep(Duration::from_millis(120));
        }
        {
            let _fast = profiler.timings().scope("fast");
            thread::sleep(Duration::from_millis(1));
        }
        let report = profiler.stop_profile().unwrap();

        assert_eq!(report.rows[0].path, "slow", "{report:?}");
        assert_eq!(report.rows[0].calls, 1);
        assert!(report.rows[0].self_ms >= 10.0, "{report:?}");
        assert_eq!(report.rows[1].path, "fast", "{report:?}");
        assert!(
            !report.rows.iter().any(|row| row.path == "before-start"),
            "start clears earlier scopes: {report:?}"
        );

        let lines = report.lines();
        assert!(lines[0].starts_with("Profile: "), "{lines:?}");
        assert!(lines[0].contains("2 systems"), "{lines:?}");
        assert!(lines[1].contains("slow"), "{lines:?}");
        assert!(lines[1].contains("ms self"), "{lines:?}");
    }

    #[test]
    fn profile_reports_game_state_lock_wait() {
        let profiler = Profiler::new();
        profiler.start_profile().unwrap();
        profiler.record_state_lock_wait(Duration::from_millis(3));
        let report = profiler.stop_profile().unwrap();
        let row = report
            .rows
            .iter()
            .find(|row| row.path == "game-state-lock-wait")
            .unwrap();
        assert_eq!(row.calls, 1);
        assert!(row.total_ms >= 3.0);
    }

    #[test]
    fn profile_start_and_stop_are_exclusive() {
        let profiler = Profiler::new();
        assert!(!profiler.profile_running());
        assert_eq!(
            profiler.stop_profile().unwrap_err(),
            "no profile is running; use /profile start"
        );

        profiler.start_profile().unwrap();
        assert!(profiler.profile_running());
        assert_eq!(
            profiler.start_profile().unwrap_err(),
            "a profile is already running"
        );

        let report = profiler.stop_profile().unwrap();
        assert!(report.rows.is_empty());
        assert!(!profiler.profile_running());
        assert!(
            report
                .lines()
                .iter()
                .any(|line| line.contains("No tick scopes"))
        );
    }
}
