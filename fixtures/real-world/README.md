# Corpus provenance

The all-language corpus uses `coverage-manifest.json` for immutable source identities, hashes, license evidence, and SQLite fact assertions. Its runtime downloads nothing. Existing specialized fixtures remain useful controls, but their names alone do not establish reproducible upstream provenance.

## Historical JSON, QML, and R inputs

The following inventory records the inherited bytes without inventing missing source revisions. These entries do not substitute for a pinned sample in the all-language corpus.

| Path | SHA-256 | Provenance and scope |
| --- | --- | --- |
| `fixtures/real-world/json/memories.jsonl` | `a8e216c5a7a9f9c01a3c183dc915ef981f620e6ab6c6d33879708d9193803a4b` | Historical synthetic memory records; no upstream revision or authorship provenance is recorded. This is a JSONL control, not a pinned upstream corpus. |
| `fixtures/qml/real-world/cool-retro-term-main.qml` | `697c60914d5bcfd44d505fb7e7af1d5f90af732c398d5daa1ccee1562ddebd53` | Historical upstream sample attributed in its header to Swordfish90/cool-retro-term, GPL-3.0-or-later. The original upstream revision is not recorded. |
| `fixtures/qml/real-world/kde-plasma-desktop-main.qml` | `d065ea14014362d228f7f841b3426f53938e15e9357e94987d9796172ade5f6e` | Historical upstream sample attributed to KDE/plasma-desktop, with GPL-2.0-or-later SPDX header. The original upstream revision is not recorded. |
| `fixtures/r/real-world/ggplot2-geom-point.R` | `cc6f01363b5821dec91e85770500ab4e0e7b0a974d46d7c069e775b39fde93b5` | Adapted/simplified ggplot2 control, as stated in its header; no exact upstream revision is recorded. It is not an unmodified pinned upstream source. |

The inline QML style examples in `crates/julie-extractors/src/tests/qml/real_world.rs` and R patterns in `crates/julie-extractors/src/tests/r/real_world.rs` are simplified controls. They do not prove extraction of an immutable upstream checkout. Keep their functional checks while using the pinned corpus for reproducible real-source validation.

The existing [Erlang corpus](erlang/README.md) records package versions, checksums, and source license files independently.
