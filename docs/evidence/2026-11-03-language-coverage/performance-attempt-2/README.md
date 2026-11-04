# Performance comparison attempt 2

Outcome: **inconclusive, 5 of 6 workloads**. The comparison reports passed=false and the command exit code is 1.

Five workloads have no gate failures. The failed workload was all / references / single_file_update. Its timing gate reports: timing: baseline sample spread is inconclusive; timing: a baseline batch sample was shorter than two seconds. This attempt does not pass the performance gate.

The attempt used candidate binary SHA-256 c236e36111785c9ac6bcab6220f07403ad7ad54d02b00590fbe27f6358e40dcc from source commit bd7369f043dd2c1c4ddd3a76541c1c7ef372cb56 with source tree SHA-256 bd8794bd18e2c9ba771592d0ef4e3f187768123781101cce5ed6ae374a149262. Candidate metadata SHA-256 is fc4aaa61fbaa9fb29e9c9f44234f1052bd337004b1aa2277950afa264762f73c; its build-record SHA-256 is a07c3e0033c69924b34dc15d4bd1cd33cbea875438900f0666187a054dca5642.

The comparison, command log, and candidate metadata are preserved as deterministic gzip copies with mtime zero. The exit-code file and candidate build record remain uncompressed. The manifest records original uncompressed byte counts and hashes plus gzip hashes.

Source links are historical path locators; later edits at those paths do not alter these archived bytes. See [artifacts.json](artifacts.json) for the digests and archive paths.
