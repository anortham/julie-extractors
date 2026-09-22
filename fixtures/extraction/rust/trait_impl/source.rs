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

/// A job that also reports a name.
pub trait NamedJob: Runnable + Send {
    fn name(&self) -> String;
}

/// A job that wraps a value.
pub struct Wrapped<T> {
    pub inner: T,
}

impl<T: Clone> Wrapped<T> {
    pub fn get(&self) -> T {
        self.inner.clone()
    }
}

impl std::fmt::Display for FixedJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.run())
    }
}

/// Builds a job from parsed text.
pub fn parse_job(text: &str) -> FixedJob {
    FixedJob::new(text.parse::<u32>().unwrap_or(0))
}
