# Offline license evidence

`coverage-manifest.json` maps every sample to `provenance.license_evidence_files`. Each entry records the local path, SHA-256, evidence kind, and pinned upstream URL. Paths under a project slug mirror the selected repository path; the corpus runner verifies every file hash before extraction.

The files retain project-specific scope and attribution:

- Locally authored controls use the workspace `license = "MIT"` declaration from the pinned Julie Extractors `Cargo.toml`; the MIT license text is retained separately as the license evidence.
- Blazor code uses `blazor-samples/LICENSE-CODE.md`; the repository's documentation license does not cover the sample code.
- Godot uses the selected demo's `2d/dodge_the_creeps/LICENSE`.
- ggplot2 keeps `DESCRIPTION`, `LICENSE`, and `LICENSE.md` together to preserve the package license declaration and text.
- QML Material keeps its README software-license statement and LGPL text. Its separate CC-BY icon license is excluded because the selected qmldir sample contains no icon assets.
- Qt Declarative keeps the SPDX alternatives' BSD-3-Clause and commercial license texts.
- Apache Commons Lang, Elixir, PowerShell, Requests, Roslyn, Scala, and Telemetry include their upstream notice files when present.
