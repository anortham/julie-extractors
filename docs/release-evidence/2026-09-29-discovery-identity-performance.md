# Discovery identity performance

Compared `v3.7.1`, `be2f8a10` before the fix, and the discovery identity fix
applied to `be2f8a10`. All binaries were local macOS ARM64 debug builds.

## Change and regression guard

Discovery lazily opens each artifact or SQLite sidecar identity once per walk.
Unix source identity checks use device/inode metadata; other platforms open
one source handle per matching basename. Handles belong to the walk and close
before discovery returns, so they do not survive into artifact writes or
replacement. Single-file selection owns and closes its own temporary handle.

The eight-file regression test failed with 16 identity handle opens before
the fix. It now allows one open on Unix, or nine on other platforms. The
sidecar test allows three opens on Unix, or 27 for 24 files elsewhere.
Replacement and late-sidecar tests verify that identities refresh between
walks and that the policy does not retain open handles.

## Local measurements

The source tree contains 4,000 directories, each holding `index.rs` containing
`pub fn a() {}\n`. Each binary has its own artifact outside the source root.
The matching case names that artifact `index.rs`; the control uses
`artifact.sqlite`. After an initial full scan and one discarded warm scan,
each binary ran ten unchanged scans, with binary order rotated each round.
Tests and builds were finished before these measurements. All measured scans
reported `no_change`; each artifact contained 4,000 files and 4,000 symbols.
P95 uses the nearest-rank definition.

| Artifact name | Build | Discovery median / p95 (ms) | Total median / p95 (ms) |
| --- | --- | --- | --- |
| `index.rs` | 3.7.1 | 202.5 / 236 | 469.54 / 793.75 |
| `index.rs` | Before | 315.5 / 328 | 593.83 / 603.78 |
| `index.rs` | After | 210 / 246 | 476.78 / 819.22 |
| `artifact.sqlite` | 3.7.1 | 203.5 / 287 | 474.32 / 871.30 |
| `artifact.sqlite` | Before | 207 / 392 | 478.19 / 971.14 |
| `artifact.sqlite` | After | 204.5 / 265 | 474.07 / 811.06 |

The matching-name discovery p95 fell 25%, and total median fell 20%, close
to the 3.7.1 baseline. Total p95 remained noisy and did not improve in the
matching-name case; these samples do not establish an overall tail-latency
improvement. These are synthetic debug-build measurements, not release-build
throughput claims. Raw local samples: `/tmp/julie-fixed-perf-clean-results.json`.

## Reproduction

Build each comparison source with `cargo build -p julie-extract-cli`, using
separate target directories, and copy its binary before changing source.
Use `--manifest-path` when building an archived comparison checkout.
Set `JULIE_BASELINE`, `JULIE_BEFORE`, and `JULIE_AFTER` to those binaries:

```python
import json, os, pathlib, subprocess, tempfile, time

base = pathlib.Path(tempfile.mkdtemp(prefix="discovery-identity-"))
root = base / "source"
root.mkdir()
for index in range(4000):
    directory = root / str(index)
    directory.mkdir()
    (directory / "index.rs").write_text("pub fn a() {}\n")
binaries = {name: os.environ[f"JULIE_{name.upper()}"]
            for name in ("baseline", "before", "after")}
results = {}
for filename in ("index.rs", "artifact.sqlite"):
    commands = {}
    results[filename] = {name: [] for name in binaries}
    for name, binary in binaries.items():
        directory = base / f"{filename}-{name}"
        directory.mkdir()
        command = [binary, "scan", "--root", str(root),
                   "--db", str(directory / filename), "--json"]
        commands[name] = command
        for _ in range(2):
            subprocess.run(command, capture_output=True, check=True)
    for index in range(10):
        names = list(binaries)
        names = names[index % 3:] + names[:index % 3]
        for name in names:
            started = time.perf_counter()
            output = subprocess.run(commands[name], capture_output=True, check=True)
            elapsed = (time.perf_counter() - started) * 1000
            report = json.loads(output.stdout)
            assert report["status"] == "no_change"
            results[filename][name].append({
                "wall_ms": elapsed,
                "discovery_ms": report["profile"]["phases"]["discovery"]})
print(json.dumps(results, indent=2))
```

## Verification

- `cargo test -p julie-extract-cli` passed.
- `cargo clippy -p julie-extract-cli --all-targets -- -D warnings` passed.
- The production identity helper definitions compiled unchanged in an isolated
  crate for `x86_64-pc-windows-msvc`, using `same-file` 1.0.6 and forbidding
  unsafe code. This checks the new platform branch, not the complete Windows
  CLI or native Windows execution. The complete cross-build remains blocked
  by C grammar compilation on the macOS host.

Differently named hard links remain outside the identity probe, as before.
