//! Parallel entity-phase barriers; every task joins before advancing.

use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::{Result, ensure};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

pub(crate) struct Scheduler {
    workers: ThreadPool,
}

impl Scheduler {
    pub(crate) fn new(worker_count: usize) -> Result<Self> {
        ensure!(worker_count > 0, "worker count must be positive");

        let workers = ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .thread_name(|index| format!("scrimmage-{index}"))
            .build()?;

        Ok(Self { workers })
    }

    /// A phase completes every task before returning an error or allowing the next phase.
    pub(crate) fn run_phase<T: Send>(
        &self,
        items: &mut [T],
        update: impl Fn(&mut T) -> Result<()> + Sync,
    ) -> Result<()> {
        let run = |(index, item): (usize, &mut T)| {
            catch_unwind(AssertUnwindSafe(|| update(item)))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("task {index} panicked")))
        };
        // DEVNOTE: one worker runs on the calling thread instead of the pool.
        // Profiling perf.py's motion workload (64 aircraft, 20,000 steps) with macOS
        // `sample` showed the main thread mostly waiting on rayon's latch: each phase
        // is handed to a pool thread, which is woken, and then joined, costing ~7 µs
        // even with one thread (~35 µs with eight). That exceeds the work in light
        // phases. Running inline took that run from 2.04 s to 1.28 s with identical
        // output. The same per-phase cost is why 8 workers only pay off when phases
        // carry real work (e.g. many sensors); more workers still use the pool.
        let results: Vec<_> = if self.workers.current_num_threads() == 1 {
            items.iter_mut().enumerate().map(run).collect()
        } else {
            self.workers
                .install(|| items.par_iter_mut().enumerate().map(run).collect())
        };

        for result in results {
            result?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_task_does_not_skip_other_entities() -> Result<()> {
        for worker_count in [1, 2, 8] {
            let scheduler = Scheduler::new(worker_count)?;
            let mut updates = [0; 32];

            let result = scheduler.run_phase(&mut updates, |count| {
                *count += 1;
                anyhow::bail!("deliberate plugin failure");
            });

            assert!(result.is_err());
            assert_eq!(updates, [1; 32]);
        }
        Ok(())
    }

    #[test]
    fn a_panicking_task_is_joined_before_the_phase_returns() -> Result<()> {
        let scheduler = Scheduler::new(4)?;
        let mut entities: Vec<_> = (0..32).map(|id| (id, false)).collect();

        let result = scheduler.run_phase(&mut entities, |(id, updated)| {
            *updated = true;
            assert_ne!(*id, 7, "deliberate plugin panic");
            Ok(())
        });

        assert!(result.unwrap_err().to_string().contains("task 7 panicked"));
        assert!(entities.iter().all(|(_, updated)| *updated));
        Ok(())
    }

    #[test]
    fn first_error_keeps_its_cause_after_all_tasks_finish() -> Result<()> {
        for worker_count in [1, 2, 8] {
            let scheduler = Scheduler::new(worker_count)?;
            let mut entities = [(1, false), (2, false)];
            let error = scheduler
                .run_phase(&mut entities, |(id, updated)| {
                    *updated = true;
                    Err(anyhow::anyhow!("invalid model state").context(format!("entity {id}")))
                })
                .unwrap_err();

            assert_eq!(error.to_string(), "entity 1");
            assert_eq!(error.root_cause().to_string(), "invalid model state");
            assert!(entities.iter().all(|(_, updated)| *updated));
        }
        Ok(())
    }

    #[test]
    fn empty_phases_do_not_prevent_later_work() -> Result<()> {
        let scheduler = Scheduler::new(2)?;
        let mut entities: [usize; 0] = [];
        scheduler.run_phase(&mut entities, |_| panic!("no task should run"))?;

        let mut updates = [0; 3];
        for _ in 0..20 {
            scheduler.run_phase(&mut updates, |count| {
                *count += 1;
                Ok(())
            })?;
        }
        assert_eq!(updates, [20; 3]);
        Ok(())
    }
}
