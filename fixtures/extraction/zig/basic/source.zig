pub const Worker = struct {
    id: i32,

    pub fn run(self: Worker) i32 {
        record_run(self.id);
        return helper(self.id);
    }
};

fn record_run(id: i32) void {
    observe_run("worker-run", id);
}

fn observe_run(event: []const u8, id: i32) void {}

pub fn helper(value: i32) i32 {
    return value + 1;
}

pub fn fetch_status() void {
    fetch_url("https://api.example.com/workers/status");
}

fn fetch_url(url: []const u8) void {}

pub fn runWorker(worker: Worker) i32 {
    return helper(worker.id);
}

pub fn evaluate(count: i32, enabled: bool) i32 {
    var total: i32 = 0;
    if (enabled) {
        for (0..count) |i| {
            total += i;
        }
    } else if (count > 0) {
        total = if (count > 10) 1 else 0;
    }
    return total;
}
