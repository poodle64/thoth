//! Priority admission control for the shared transcription model.
//!
//! Transcription is served by a single process-wide model instance (see
//! `TRANSCRIPTION_SERVICE` in the parent module). Both live dictation and MCP
//! file jobs go through it, and a plain mutex grants no preference to either,
//! so a batch of file jobs used to queue ahead of the user's own dictation and
//! hold the microphone hostage until the batch drained (#118).
//!
//! This gate is the single serialisation point in front of that model. It
//! guarantees two things:
//!
//! 1. **Interactive pre-empts queued background work.** A live request never
//!    waits behind background jobs that have not started; it only ever waits
//!    for the one job currently holding the model. ASR inference cannot be
//!    cancelled part-way, so "wait for the current holder" is the floor on
//!    what any priority scheme can achieve here.
//! 2. **At most one background job holds the model at a time.** Submitting
//!    seven files no longer means seven jobs racing for the same resource, so
//!    the interactive wait stays bounded by one job's duration however deep
//!    the queue gets.
//!
//! Background work is deliberately starvable: under continuous dictation it
//! waits, because the interactive path is the one a human is sitting in front
//! of. Batch jobs are asynchronous by construction and poll for their result.

use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;

/// Priority tier for a request against the shared transcription model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Live dictation. A human is waiting on the result, so this tier takes
    /// the model ahead of any background work that has not already started.
    Interactive,
    /// Background file transcription (MCP/HTTP `transcribe_file` jobs).
    Background,
}

#[derive(Default)]
struct GateState {
    /// Whether some request currently holds the model.
    busy: bool,
    /// Interactive requests waiting for the model. Background requests stand
    /// aside while this is non-zero, which is what makes the priority real.
    interactive_waiting: usize,
}

/// Priority-aware admission gate for the shared transcription model.
pub struct TranscriptionGate {
    state: Mutex<GateState>,
    ready: Condvar,
}

impl TranscriptionGate {
    /// Construct an independent gate. Production code uses [`gate`]; tests
    /// build their own so they do not share the process-wide instance.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(GateState::default()),
            ready: Condvar::new(),
        }
    }

    /// Acquire the model at the given priority, blocking until it is free.
    ///
    /// The returned permit releases the gate when dropped, so the caller must
    /// hold it for as long as it is using the model.
    pub fn acquire(&self, priority: Priority) -> GatePermit<'_> {
        let mut state = self.state.lock();

        match priority {
            Priority::Interactive => {
                // Register as waiting *before* blocking, so any background
                // request that wakes up meanwhile sees us and stands aside.
                state.interactive_waiting += 1;
                while state.busy {
                    self.ready.wait(&mut state);
                }
                state.interactive_waiting -= 1;
            }
            Priority::Background => {
                // Yield to interactive work that is waiting, not just to work
                // already running; otherwise a queue of background jobs can
                // hand the model to each other and never let dictation in.
                while state.busy || state.interactive_waiting > 0 {
                    self.ready.wait(&mut state);
                }
            }
        }

        state.busy = true;
        drop(state);

        GatePermit { gate: self }
    }
}

impl Default for TranscriptionGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Proof that the holder has exclusive use of the transcription model.
///
/// Releasing happens on drop, which covers the error and panic paths as well
/// as the success path.
pub struct GatePermit<'a> {
    gate: &'a TranscriptionGate,
}

impl Drop for GatePermit<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock();
        state.busy = false;
        drop(state);
        // Wake everyone: the waiters apply the priority rule themselves, so a
        // targeted wake risks waking a background job while an interactive one
        // is queued.
        self.gate.ready.notify_all();
    }
}

/// The process-wide gate guarding the single transcription model instance.
pub fn gate() -> &'static TranscriptionGate {
    static GATE: OnceLock<TranscriptionGate> = OnceLock::new();
    GATE.get_or_init(TranscriptionGate::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// Time each simulated transcription occupies the model.
    const HOLD: Duration = Duration::from_millis(50);
    /// Number of background jobs queued ahead of the interactive request.
    const BACKGROUND_JOBS: usize = 8;

    #[test]
    fn interactive_is_not_starved_by_a_queue_of_background_jobs() {
        let gate = TranscriptionGate::new();
        let started = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for _ in 0..BACKGROUND_JOBS {
                scope.spawn(|| {
                    let _permit = gate.acquire(Priority::Background);
                    started.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(HOLD);
                });
            }

            // Let the batch queue up and one job take the model, mirroring the
            // reported bug: the user starts dictating mid-batch.
            std::thread::sleep(HOLD / 2);

            let waited = {
                let start = Instant::now();
                let _permit = gate.acquire(Priority::Interactive);
                start.elapsed()
            };

            // Serial worst case is every queued job draining first. The gate
            // must beat that by waiting for at most the current holder.
            let serial_worst_case = HOLD * BACKGROUND_JOBS as u32;
            let bound = HOLD * 3;
            assert!(
                waited < bound,
                "interactive waited {waited:?}, expected under {bound:?} \
                 (serial worst case would be {serial_worst_case:?})"
            );

            // The wait must be bounded because the queue was still draining,
            // not because it had already emptied.
            assert!(
                started.load(Ordering::SeqCst) < BACKGROUND_JOBS,
                "background queue drained before the interactive request ran, \
                 so this did not exercise pre-emption"
            );
        });
    }

    #[test]
    fn only_one_background_job_holds_the_model_at_a_time() {
        let gate = TranscriptionGate::new();
        let in_flight = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for _ in 0..BACKGROUND_JOBS {
                scope.spawn(|| {
                    let _permit = gate.acquire(Priority::Background);
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(HOLD / 5);
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });

        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "background jobs must not hold the transcription model concurrently"
        );
    }

    #[test]
    fn permit_is_released_when_the_holder_panics() {
        let gate = TranscriptionGate::new();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _permit = gate.acquire(Priority::Interactive);
            panic!("transcription blew up");
        }));
        assert!(result.is_err(), "expected the panic to propagate");

        // A leaked permit would deadlock this acquire.
        let _permit = gate.acquire(Priority::Background);
    }
}
