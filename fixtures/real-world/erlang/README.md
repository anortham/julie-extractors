# Erlang real-world corpus

Vendored sources of three hex.pm packages, used by the feature-gated corpus gate
`crates/julie-extract-cli/tests/erlang_corpus.rs`.

| Package | Version | Source tarball | License |
|---|---|---|---|
| `telemetry` | 1.3.0 | https://repo.hex.pm/tarballs/telemetry-1.3.0.tar | Apache-2.0 (`telemetry-1.3.0/LICENSE`, `telemetry-1.3.0/NOTICE`) |
| `certifi` | 2.15.0 | https://repo.hex.pm/tarballs/certifi-2.15.0.tar | BSD-3-Clause (`certifi-2.15.0/LICENSE`) |
| `unicode_util_compat` | 0.7.1 | https://repo.hex.pm/tarballs/unicode_util_compat-0.7.1.tar | Apache-2.0 (`unicode_util_compat-0.7.1/LICENSE`) |

hex.pm outer-tarball `CHECKSUM` values at download time (2026-07-31):

```
telemetry-1.3.0            FEDEBBAE410D715CF8E7062C96A1EF32EC22E764197F70CDA73D82778D61E7A2
certifi-2.15.0             0E6E882FCDAAA0A5A9F2B3DB55B1394DBA07E8D6D9BCAD08318FB604C6839712
unicode_util_compat-0.7.1  A48703A25C170EEDADCA83B11E88985AF08D35F37C6F664D6DCFB106A97782FC
```

## What is vendored

Each package contributes its `src/**/*.erl`, `src/**/*.hrl`, and `include/**/*.hrl` files plus its
`LICENSE*` file. Nothing else from the tarball is kept: no build config, no `.app.src`, no test
directory, no `priv/` data. `CHECKSUMS.sha256` records the SHA-256 of every vendored file in
`shasum -a 256` format and is verified from the fixture directory:

```
shasum -a 256 -c CHECKSUMS.sha256
```

## Gate scope

The gate copies only `.erl`/`.hrl` files into a temporary scan root, so the `LICENSE`, `NOTICE`,
`README.md`, and `CHECKSUMS.sha256` files here never enter the scanned corpus and the baseline stays
Erlang-only. This directory is not registered in `fixtures/extraction/capabilities.json` and is not
discovered by the golden or capability-matrix harnesses.

Run the gate with:

```
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus
```
