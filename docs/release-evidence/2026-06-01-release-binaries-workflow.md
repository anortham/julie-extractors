# Release Binaries Workflow Evidence

## Superseded Scope

This evidence records the original three-platform workflow-artifact staging run.
It is historical evidence, not the current release-publishing contract. The
current `Release Binaries` workflow builds four platforms, archives the staged
packages, creates or updates a GitHub Release, and uploads release assets.

## Run

- Commit: `6fedd9b8dd6690f81afd1fa5978a46bf113e62d0`
- Timestamp: `2026-06-01T07:38:12Z`
- Workflow: `Release Binaries`
- Trigger: `workflow_dispatch`
- Version input: `0.1.0`
- Run: `https://github.com/anortham/julie-extractors/actions/runs/26741443032`

## Hard Gate Result

- Result: pass
- Linux job: `x86_64-unknown-linux-gnu`, passed in `3m40s`
- macOS job: `aarch64-apple-darwin`, passed in `3m40s`
- Windows job: `x86_64-pc-windows-msvc`, passed in `5m6s`
- Every job completed:
  - `cargo build --release -p julie-extract-cli --bin julie-extract`
  - `cargo xtask release package`
  - `actions/upload-artifact`

## Artifacts

- `julie-extract-v0.1.0-x86_64-unknown-linux-gnu`
- `julie-extract-v0.1.0-aarch64-apple-darwin`
- `julie-extract-v0.1.0-x86_64-pc-windows-msvc`

## Local Replay Evidence

- `cargo fmt --check`: pass
- `git diff --check`: pass
- YAML parse for `.github/workflows/release-binaries.yml`: pass
- `cargo test -p xtask`: pass
- `cargo xtask test default`: pass
- `cargo xtask test contract`: pass
- `cargo build --release -p julie-extract-cli --bin julie-extract`: pass
- `cargo xtask release package --version 0.1.0 --target aarch64-apple-darwin --out-dir target/release-package/local-aarch64-apple-darwin --binary target/release/julie-extract`: pass

## Tradeoffs And Open Decisions

- The workflow uploads GitHub Actions artifacts only. It does not publish a
  GitHub Release or attach release assets.
- Tag pushes matching `v*` are supported but were not exercised by this run.
- The workflow pins macOS to `macos-15` and Windows to `windows-2022` to avoid
  changing `latest` aliases for release package production.
