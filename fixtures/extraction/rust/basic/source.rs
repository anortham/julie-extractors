use std::collections::HashMap;

pub mod fixture {
    #[derive(Debug)]
    pub struct Worker {
        pub id: i32,
    }

    impl Worker {
        pub fn run(&self) -> i32 {
            record_run(self.id);
            helper(self.id)
        }
    }

    /// Emits a worker-run marker for observability hooks.
    pub fn record_run(id: i32) {
        observe_run("worker-run", id);
    }

    /// Records a named worker event for downstream hooks.
    pub fn observe_run(event: &str, id: i32) {
        let _ = (event, id);
    }

    pub fn helper(value: i32) -> i32 {
        value + 1
    }

    /// Doubles a worker id.
    pub fn double(value: i32) -> i32 {
        value * 2
    }

    /// Checks the worker service health endpoint.
    pub fn fetch_status() {
        fetch_url("https://api.example.com/workers/status");
    }

    fn fetch_url(url: &str) {
        let _ = url;
    }

    pub fn build_index() -> HashMap<String, Vec<u8>> {
        HashMap::new()
    }

    pub fn evaluate(count: i32, enabled: bool) -> i32 {
        let mut total = 0;
        if enabled {
            for i in 0..count {
                total += i;
            }
        }
        total
    }
}
