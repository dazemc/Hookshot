//! Opt-in stage timings for the build pipeline. `HOOKSHOT_BUILD_TIMING=1`
//! prints one line per stage to stderr; nothing is printed (and no clock is
//! read) otherwise. The lines are diagnostic only and never affect output.

use std::time::Instant;

pub fn enabled() -> bool {
    std::env::var_os("HOOKSHOT_BUILD_TIMING").is_some()
}

pub struct Stage {
    label: &'static str,
    start: Option<Instant>,
}

impl Stage {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            start: enabled().then(Instant::now),
        }
    }

    /// Print this stage's time now. The stage keeps running; `done` prints it
    /// again, so a long stage can be split without moving its start.
    pub fn mark(&self, what: &str) {
        if let Some(start) = self.start {
            eprintln!(
                "build timing: {:<24} {:>8.3}s  {what}",
                self.label,
                start.elapsed().as_secs_f64()
            );
        }
    }

    pub fn done(self) {
        if let Some(start) = self.start {
            eprintln!(
                "build timing: {:<24} {:>8.3}s",
                self.label,
                start.elapsed().as_secs_f64()
            );
        }
    }
}

/// Time one expression, labelled by the stage around it.
pub fn span<T>(label: &'static str, body: impl FnOnce() -> T) -> T {
    let stage = Stage::new(label);
    let value = body();
    stage.done();
    value
}
