/// A unit of work the scheduler can run.
pub trait Runnable {
    fn run(&self) -> u32;
}

/// A job with a fixed result.
pub struct FixedJob {
    pub result: u32,
}

impl Runnable for FixedJob {
    fn run(&self) -> u32 {
        self.result
    }
}

impl FixedJob {
    pub fn new(result: u32) -> Self {
        FixedJob { result }
    }
}
