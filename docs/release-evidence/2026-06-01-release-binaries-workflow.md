# Release Binaries Workflow Evidence

## Run

- Commit: `d6b92496552aa6c5226baf972f500f417afc6400`
- Timestamp: `2026-06-01T07:16:21Z`
- Workflow: `Release Binaries`
- Trigger: `workflow_dispatch`
- Version input: `0.1.0`
- Run: `https://github.com/anortham/julie-extractors/actions/runs/26740577600`

## Hard Gate Result

- Result: pass
- Linux job: `x86_64-unknown-linux-gnu`, passed in `3m33s`
- macOS job: `aarch64-apple-darwin`, passed in `4m28s`
- Windows job: `x86_64-pc-windows-msvc`, passed in `4m37s`
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
- macOS currently uses `macos-latest`, which GitHub documents as arm64. If that
  hosted-runner mapping changes, update the matrix target label with the runner.
