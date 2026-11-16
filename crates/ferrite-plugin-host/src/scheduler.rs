//! Tick-driven scheduler for plugin and host tasks.
//!
//! Java owns the task bodies (`TaskRegistry`); Rust owns the queue, cancellation token and
//! repeat timing. A Java task is identified by the opaque handle Java created and by the
//! [`TaskId`] returned from `scheduler.schedule`. Host-internal Rust closures can be scheduled
//! with [`crate::PluginHost::schedule`].

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ferrite_jvm::Jvm;

use crate::protocol::{self, RunTaskResponse};

/// Cancellation token returned by [`crate::PluginHost::schedule`] and `scheduler.schedule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u64);

impl TaskId {
    /// Raw id, as sent to Java in `scheduler.cancel`.
    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Outcome of cancelling one task through `scheduler.cancel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelOutcome {
    Cancelled,
    NotFound,
    NotOwner,
}

pub(crate) struct Scheduler {
    inner: Mutex<Inner>,
    next: AtomicU64,
    tick: AtomicU64,
    stopped: AtomicBool,
}

#[derive(Default)]
struct Inner {
    tasks: BTreeMap<u64, Task>,
}

#[derive(Clone)]
enum TaskBody {
    Java { handle: i64, is_async: bool },
    Rust(Arc<Mutex<Box<dyn FnMut() + Send>>>),
}

struct Task {
    plugin: Option<String>,
    due: u64,
    period: Option<u64>,
    body: TaskBody,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            next: AtomicU64::new(0),
            tick: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn insert(
        &self,
        plugin: Option<String>,
        delay_ticks: u64,
        period_ticks: Option<u64>,
        body: TaskBody,
    ) -> TaskId {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let due = self.tick.load(Ordering::SeqCst).saturating_add(delay_ticks);
        self.lock().tasks.insert(
            id,
            Task {
                plugin,
                due,
                period: period_ticks.filter(|period| *period > 0),
                body,
            },
        );
        TaskId::new(id)
    }

    /// Schedules a task whose body lives in Java.
    pub(crate) fn schedule_java(
        &self,
        plugin: &str,
        handle: i64,
        delay_ticks: u64,
        period_ticks: u64,
        is_async: bool,
    ) -> TaskId {
        self.insert(
            Some(plugin.to_owned()),
            delay_ticks,
            Some(period_ticks),
            TaskBody::Java { handle, is_async },
        )
    }

    /// Schedules a host-internal Rust closure.
    pub(crate) fn schedule_rust(
        &self,
        plugin: Option<String>,
        delay_ticks: u64,
        period_ticks: Option<u64>,
        task: Box<dyn FnMut() + Send>,
    ) -> TaskId {
        self.insert(
            plugin,
            delay_ticks,
            period_ticks,
            TaskBody::Rust(Arc::new(Mutex::new(task))),
        )
    }

    /// Cancels a task by id. Returns whether it existed.
    pub(crate) fn cancel(&self, id: TaskId) -> bool {
        self.lock().tasks.remove(&id.0).is_some()
    }

    /// Cancels one task only when `plugin` owns it.
    pub(crate) fn cancel_for_plugin(&self, plugin: &str, id: TaskId) -> CancelOutcome {
        let mut inner = self.lock();
        let owner = inner
            .tasks
            .get(&id.0)
            .map(|task| task.plugin.as_deref().map(str::to_owned));
        match owner {
            None => CancelOutcome::NotFound,
            Some(Some(owner)) if owner != plugin => CancelOutcome::NotOwner,
            Some(_) => {
                inner.tasks.remove(&id.0);
                CancelOutcome::Cancelled
            }
        }
    }

    /// Cancels every task owned by `plugin` and returns the Java handles to forget.
    pub(crate) fn cancel_plugin(&self, plugin: &str) -> Vec<i64> {
        let mut inner = self.lock();
        let ids: Vec<u64> = inner
            .tasks
            .iter()
            .filter(|(_, task)| task.plugin.as_deref() == Some(plugin))
            .map(|(id, _)| *id)
            .collect();
        let mut handles = Vec::new();
        for id in ids {
            if let Some(task) = inner.tasks.remove(&id)
                && let TaskBody::Java { handle, .. } = task.body
            {
                handles.push(handle);
            }
        }
        handles
    }

    /// Stops the scheduler and drops every pending task.
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.lock().tasks.clear();
    }

    /// Runs every task that became due and reschedules repeating tasks.
    ///
    /// `now` is the tick number this call represents. A task scheduled with delay 0 on tick N
    /// is due at N and therefore runs on tick N+1, the next call.
    pub(crate) fn tick(&self, jvm: Option<Jvm>) {
        if self.stopped.load(Ordering::SeqCst) {
            return;
        }
        let now = self.tick.fetch_add(1, Ordering::SeqCst) + 1;
        let due: Vec<(u64, TaskBody, Option<u64>)> = {
            let inner = self.lock();
            inner
                .tasks
                .iter()
                .filter(|(_, task)| task.due <= now)
                .map(|(id, task)| (*id, task.body.clone(), task.period))
                .collect()
        };
        for (id, body, period) in due {
            let keep = match body {
                TaskBody::Java { handle, is_async } => {
                    let Some(jvm) = jvm else {
                        tracing::error!(task = id, "a Java task is due without a running JVM");
                        self.finish(id, now, period, false);
                        continue;
                    };
                    let method = if is_async { "runTaskAsync" } else { "runTask" };
                    match protocol::call_java_long(
                        jvm,
                        "dev/ferrite/host/Bootstrap",
                        method,
                        handle,
                    ) {
                        Ok(response) => match protocol::parse_run_task(&response) {
                            Ok(RunTaskResponse::Keep) => true,
                            Ok(RunTaskResponse::Done | RunTaskResponse::Missing) => false,
                            Ok(RunTaskResponse::Failed(error)) => {
                                tracing::warn!(task = id, %error, "a plugin task failed");
                                false
                            }
                            Err(error) => {
                                tracing::warn!(task = id, %error, "invalid plugin task response");
                                false
                            }
                        },
                        Err(error) => {
                            tracing::warn!(task = id, %error, "calling a plugin task failed");
                            false
                        }
                    }
                }
                TaskBody::Rust(body) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        if let Ok(mut task) = body.lock() {
                            task();
                        }
                    }));
                    if result.is_err() {
                        tracing::error!(task = id, "a plugin host task panicked");
                        false
                    } else {
                        period.is_some()
                    }
                }
            };
            self.finish(id, now, period, keep);
        }
    }

    fn finish(&self, id: u64, now: u64, period: Option<u64>, keep: bool) {
        let mut inner = self.lock();
        if keep {
            if let Some(task) = inner.tasks.get_mut(&id) {
                task.due = now.saturating_add(period.unwrap_or(1).max(1));
            }
        } else {
            inner.tasks.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn run_tick(scheduler: &Scheduler) {
        // These tests only exercise Rust task bodies, which never call Java.
        scheduler.tick(None);
    }

    #[test]
    fn delayed_task_runs_once_after_its_delay() {
        let scheduler = Scheduler::new();
        let counter = Arc::new(AtomicU32::new(0));
        let task_counter = Arc::clone(&counter);
        scheduler.schedule_rust(
            None,
            2,
            None,
            Box::new(move || {
                task_counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        run_tick(&scheduler);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        run_tick(&scheduler);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        run_tick(&scheduler);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn repeating_task_runs_every_period_until_cancelled() {
        let scheduler = Scheduler::new();
        let counter = Arc::new(AtomicU32::new(0));
        let task_counter = Arc::clone(&counter);
        let id = scheduler.schedule_rust(
            None,
            0,
            Some(2),
            Box::new(move || {
                task_counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        for _ in 0..5 {
            run_tick(&scheduler);
        }
        // Ticks 1..=5: due at 0 -> 1, then 3, then 5.
        assert_eq!(counter.load(Ordering::SeqCst), 3);
        assert!(scheduler.cancel(id));
        run_tick(&scheduler);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn cancel_plugin_only_removes_that_plugins_tasks_and_checks_ownership() {
        let scheduler = Scheduler::new();
        let first = scheduler.schedule_rust(Some("One".into()), 10, None, Box::new(|| {}));
        let second = scheduler.schedule_rust(Some("Two".into()), 10, None, Box::new(|| {}));
        assert_eq!(
            scheduler.cancel_for_plugin("One", second),
            CancelOutcome::NotOwner
        );
        assert_eq!(
            scheduler.cancel_for_plugin("One", first),
            CancelOutcome::Cancelled
        );
        assert!(scheduler.cancel(second));
        assert_eq!(
            scheduler.cancel_for_plugin("One", first),
            CancelOutcome::NotFound
        );
    }
}
