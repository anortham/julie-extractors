# Scan Report Profiling Evidence

Date: 2026-06-02

Binary: `target/release/julie-extract`

Profile command shape:

```bash
target/release/julie-extract scan --root <repo> --db /tmp/<name>.sqlite --force --json
```

## Results

| Repo | Status | Files scanned | Files changed | Failed files | Wall time | Report total | Extraction/spool | Artifact write | Top language |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `/Users/murphy/source/julie-extractors` | `ok` | 1,036 | 1,032 | 0 | 6.73s | 5,602ms | 2,823ms | 2,747ms | `rust`: 787 files, 1,822ms extract |
| `/Users/murphy/source/openclaw` | `ok` | 13,560 | 12,781 | 0 | 90.61s | 90,539ms | 52,364ms | 37,947ms | `typescript`: 10,961 files, 36,541ms extract |
| `/Users/murphy/source/Newtonsoft.Json` | `ok` | 1,172 | 981 | 0 | 6.89s | 6,875ms | 4,245ms | 2,587ms | `csharp`: 945 files, 3,465ms extract |
| `/Users/murphy/source/hermes-agent` | `ok` | 2,746 | 2,588 | 0 | 31.48s | 31,444ms | 18,430ms | 12,875ms | `python`: 1,465 files, 12,233ms extract |

## Observations

- `hermes-agent` did not reproduce the prior SQLite `too many SQL variables`
  failure with this binary.
- Large scans are split between extraction/spool and SQLite artifact write. On
  `openclaw`, extraction/spool was 52.4s and artifact write was 37.9s.
- The profile covers non-TypeScript-heavy repos: Rust fixture-heavy
  `julie-extractors`, C# `Newtonsoft.Json`, and Python-heavy `hermes-agent`.
- `read_duration_ms` and `spool_write_duration_ms` were small compared with
  parser extraction and artifact write in these runs.
