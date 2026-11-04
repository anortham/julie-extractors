# Final verification gate evidence

The archive preserves the recorded default tier, writer checks, and Windows calibration follow-up for commit bd7369f043dd2c1c4ddd3a76541c1c7ef372cb56.

The default-run index records four successful runs: warmup and measured runs 1–3. Warmup is excluded from statistics. The three measured wall-clock results are 31.876s minimum, 32.054s median, and 42.818s maximum against a 180s median limit.

The writer-current-schema summary records 10,000 files and 112,605 rows per second. The writer floor and current-schema command records are included with their source indexes.

The Windows follow-up report and every coverage-windows-calibration-followup log are archived. They preserve the focused T-SQL retry, initial link failure, xtask output, capacity measurements, cleanup details, and guest shutdown status.

JSON and log artifacts are stored as deterministic gzip copies with mtime zero. The manifest records their original uncompressed byte counts and SHA-256 values alongside compressed SHA-256 values. The Markdown report remains uncompressed.

Source links are historical path locators; later edits at those paths do not alter these archived bytes. See [artifacts.json](artifacts.json) for the digests and archive paths.
