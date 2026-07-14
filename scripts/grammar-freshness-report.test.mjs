import assert from "node:assert/strict"
import test from "node:test"

import {
  compareSemanticVersions,
  createCratesIoAdapter,
  createFreshnessReport,
  createGitHubAdapter,
  latestStableVersion,
  normalizeGitHubRepository,
  parseCargoLock,
  parseCliArgs,
  parseManifestParserDependencies,
  renderReport,
  runCli,
} from "./grammar-freshness-report.mjs"

const MANIFEST_FIXTURE = `
[dependencies]
tree-sitter = "=0.26.11"
swift-parser = { package = "tree-sitter-swift", version = "=0.7.3" }
csharp-parser = { package = "tree-sitter-c-sharp", git = "https://github.com/anortham/tree-sitter-c-sharp", rev = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
serde = "1"
`

const LOCK_FIXTURE = `
[[package]]
name = "tree-sitter"
version = "0.26.11"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "tree-sitter-swift"
version = "0.7.3"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "tree-sitter-c-sharp"
version = "0.23.5"
source = "git+https://github.com/anortham/tree-sitter-c-sharp?rev=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
`

test("parses parser dependencies and exact lock resolutions", () => {
  assert.deepEqual(parseManifestParserDependencies(MANIFEST_FIXTURE), [
    {
      dependency: "csharp-parser",
      package: "tree-sitter-c-sharp",
      source: "git",
      remote: "https://github.com/anortham/tree-sitter-c-sharp",
      rev: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    },
    {
      dependency: "swift-parser",
      package: "tree-sitter-swift",
      source: "registry",
      requirement: "=0.7.3",
    },
    {
      dependency: "tree-sitter",
      package: "tree-sitter",
      source: "registry",
      requirement: "=0.26.11",
    },
  ])
  assert.deepEqual(parseCargoLock(LOCK_FIXTURE), [
    {
      package: "tree-sitter",
      version: "0.26.11",
      source: "registry+https://github.com/rust-lang/crates.io-index",
    },
    {
      package: "tree-sitter-c-sharp",
      version: "0.23.5",
      source:
        "git+https://github.com/anortham/tree-sitter-c-sharp?rev=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    },
    {
      package: "tree-sitter-swift",
      version: "0.7.3",
      source: "registry+https://github.com/rust-lang/crates.io-index",
    },
  ])
})

test("normalizes supported GitHub remote URL forms", () => {
  for (const remote of [
    "https://github.com/anortham/tree-sitter-sql",
    "https://github.com/anortham/tree-sitter-sql.git",
    "git+https://github.com/anortham/tree-sitter-sql?rev=abc",
    "git@github.com:anortham/tree-sitter-sql.git",
    "ssh://git@github.com/anortham/tree-sitter-sql.git",
  ]) {
    assert.equal(normalizeGitHubRepository(remote), "anortham/tree-sitter-sql")
  }
  assert.throws(
    () => normalizeGitHubRepository("https://gitlab.com/anortham/tree-sitter-sql"),
    /GitHub remote/,
  )
})

test("orders semantic versions correctly and selects the latest non-yanked stable release", () => {
  assert.ok(compareSemanticVersions("1.10.0", "1.9.99") > 0)
  assert.ok(compareSemanticVersions("1.0.0-beta.11", "1.0.0-rc.1") < 0)
  assert.ok(compareSemanticVersions("1.0.0-1", "1.0.0-alpha") < 0)
  assert.equal(compareSemanticVersions("1.0.0+build.2", "1.0.0+build.1"), 0)
  assert.equal(
    latestStableVersion([
      { num: "1.0.9", yanked: false },
      { num: "1.1.0-beta.1", yanked: false },
      { num: "1.0.10", yanked: false },
      { num: "1.0.11+build-1", yanked: false },
      { num: "1.0.12-rc.1+build-2", yanked: false },
      { num: "2.0.0", yanked: true },
    ]),
    "1.0.11+build-1",
  )
})

test("builds a deterministically ordered versioned report without semantic claims", async () => {
  const report = await createFreshnessReport({
    manifestText: MANIFEST_FIXTURE,
    lockText: LOCK_FIXTURE,
    generatedAt: "2026-07-14T14:00:00.000Z",
    manifestPath: "crates/julie-extractors/Cargo.toml",
    lockPath: "Cargo.lock",
    getLatestStable: async (packageName) =>
      new Map([
        ["tree-sitter", "0.26.11"],
        ["tree-sitter-swift", "0.8.0"],
      ]).get(packageName),
    getGitDefaultHead: async () => ({
      defaultBranch: "master",
      head: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    }),
  })

  assert.deepEqual(report, {
    schema_version: 1,
    audit: {
      generated_at: "2026-07-14T14:00:00.000Z",
      manifest_path: "crates/julie-extractors/Cargo.toml",
      lock_path: "Cargo.lock",
    },
    runtime: {
      dependency: "tree-sitter",
      package: "tree-sitter",
      declared_requirement: "=0.26.11",
      locked_version: "0.26.11",
      latest_stable_version: "0.26.11",
      status: "current",
    },
    registry_grammars: [
      {
        dependency: "swift-parser",
        package: "tree-sitter-swift",
        declared_requirement: "=0.7.3",
        locked_version: "0.7.3",
        latest_stable_version: "0.8.0",
        status: "drift",
      },
    ],
    git_grammars: [
      {
        dependency: "csharp-parser",
        package: "tree-sitter-c-sharp",
        remote: "https://github.com/anortham/tree-sitter-c-sharp",
        repository: "anortham/tree-sitter-c-sharp",
        pinned_rev: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        locked_rev: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        remote_default_branch: "master",
        remote_head: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        status: "drift",
      },
    ],
  })
  assert.equal("semantic_support" in report.registry_grammars[0], false)
})

test("renders fixture-backed JSON and text byte-identically for an injected timestamp", async () => {
  const input = {
    manifestText: MANIFEST_FIXTURE,
    lockText: LOCK_FIXTURE,
    generatedAt: "2026-07-14T14:00:00.000Z",
    manifestPath: "crates/julie-extractors/Cargo.toml",
    lockPath: "Cargo.lock",
    getLatestStable: async (packageName) =>
      packageName === "tree-sitter" ? "0.26.11" : "0.7.3",
    getGitDefaultHead: async () => ({
      defaultBranch: "master",
      head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    }),
  }
  const first = await createFreshnessReport(input)
  const second = await createFreshnessReport(input)

  assert.equal(renderReport(first, "json"), renderReport(second, "json"))
  assert.equal(renderReport(first, "text"), renderReport(second, "text"))
  assert.equal(
    renderReport(first, "text"),
    `Grammar freshness audit: 2026-07-14T14:00:00.000Z
Runtime
  tree-sitter [tree-sitter] declared =0.26.11, locked 0.26.11, latest stable 0.26.11: current
Registry grammars
  swift-parser [tree-sitter-swift] declared =0.7.3, locked 0.7.3, latest stable 0.7.3: current
Git grammars
  csharp-parser [anortham/tree-sitter-c-sharp] pinned aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, locked aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, master aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: current
`,
  )
})

test("parses only the supported CLI shape", () => {
  assert.deepEqual(parseCliArgs([]), { format: "text" })
  assert.deepEqual(parseCliArgs(["--format", "text"]), { format: "text" })
  assert.deepEqual(parseCliArgs(["--format", "json"]), { format: "json" })
  assert.throws(() => parseCliArgs(["--format"]), /usage:/)
  assert.throws(() => parseCliArgs(["--format", "yaml"]), /usage:/)
  assert.throws(() => parseCliArgs(["--json"]), /usage:/)
})

test("crates.io adapter sends required headers and rejects source-labelled failures", async () => {
  const requests = []
  const getLatestStable = createCratesIoAdapter({
    timeoutMs: 50,
    fetchImpl: async (url, options) => {
      requests.push({ url, options })
      return {
        ok: true,
        json: async () => ({
          versions: [
            { num: "1.3.0", yanked: false },
            { num: "1.4.0-beta.1", yanked: false },
          ],
        }),
      }
    },
  })

  assert.equal(await getLatestStable("tree-sitter-r"), "1.3.0")
  assert.equal(requests[0].url, "https://crates.io/api/v1/crates/tree-sitter-r")
  assert.match(requests[0].options.headers["User-Agent"], /julie-extractors/)
  assert.equal(requests[0].options.headers.Accept, "application/json")
  assert.ok(requests[0].options.signal instanceof AbortSignal)

  const failing = createCratesIoAdapter({
    fetchImpl: async () => ({ ok: false, status: 503 }),
  })
  await assert.rejects(() => failing("tree-sitter-r"), /crates.io tree-sitter-r: HTTP 503/)

  const malformed = createCratesIoAdapter({
    fetchImpl: async () => ({ ok: true, json: async () => ({ versions: [] }) }),
  })
  await assert.rejects(
    () => malformed("tree-sitter-r"),
    /crates.io tree-sitter-r: metadata has no stable release/,
  )

  const invalidVersion = createCratesIoAdapter({
    fetchImpl: async () => ({
      ok: true,
      json: async () => ({ versions: [{ num: "next", yanked: false }] }),
    }),
  })
  await assert.rejects(
    () => invalidVersion("tree-sitter-r"),
    /crates.io tree-sitter-r: malformed version metadata/,
  )
})

test("GitHub adapter resolves a default head with required headers and source-labelled failures", async () => {
  const requests = []
  const responses = [
    { ok: true, json: async () => ({ default_branch: "master" }) },
    {
      ok: true,
      json: async () => ({ sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }),
    },
  ]
  const getGitDefaultHead = createGitHubAdapter({
    timeoutMs: 50,
    fetchImpl: async (url, options) => {
      requests.push({ url, options })
      return responses.shift()
    },
  })

  assert.deepEqual(await getGitDefaultHead("anortham/tree-sitter-sql"), {
    defaultBranch: "master",
    head: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  })
  assert.deepEqual(
    requests.map(({ url }) => url),
    [
      "https://api.github.com/repos/anortham/tree-sitter-sql",
      "https://api.github.com/repos/anortham/tree-sitter-sql/commits/master",
    ],
  )
  for (const { options } of requests) {
    assert.match(options.headers["User-Agent"], /julie-extractors/)
    assert.equal(options.headers.Accept, "application/vnd.github+json")
    assert.equal(options.headers["X-GitHub-Api-Version"], "2026-03-10")
    assert.ok(options.signal instanceof AbortSignal)
  }

  const failing = createGitHubAdapter({
    fetchImpl: async () => ({ ok: false, status: 403 }),
  })
  await assert.rejects(
    () => failing("anortham/tree-sitter-sql"),
    /GitHub anortham\/tree-sitter-sql: HTTP 403/,
  )

  const malformed = createGitHubAdapter({
    fetchImpl: async () => ({ ok: true, json: async () => ({ default_branch: null }) }),
  })
  await assert.rejects(
    () => malformed("anortham/tree-sitter-sql"),
    /GitHub anortham\/tree-sitter-sql: repository metadata has no default branch/,
  )

  const invalidCommitResponses = [
    { ok: true, json: async () => ({ default_branch: "main" }) },
    { ok: true, json: async () => ({ sha: "short" }) },
  ]
  const invalidCommit = createGitHubAdapter({
    fetchImpl: async () => invalidCommitResponses.shift(),
  })
  await assert.rejects(
    () => invalidCommit("anortham/tree-sitter-sql"),
    /GitHub anortham\/tree-sitter-sql: default-branch commit metadata has no full commit ID/,
  )
})

test("CLI maps invalid arguments and adapter failures to source-labelled nonzero results", async () => {
  const invalidErrors = []
  assert.equal(
    await runCli(["--json"], {
      stderr: (value) => invalidErrors.push(value),
    }),
    2,
  )
  assert.match(invalidErrors.join(""), /usage:/)

  const cratesErrors = []
  assert.equal(
    await runCli(["--format", "json"], {
      loadInputs: async () => ({
        manifestText: MANIFEST_FIXTURE,
        lockText: LOCK_FIXTURE,
      }),
      getLatestStable: async (packageName) => {
        throw new Error(`crates.io ${packageName}: rate limited`)
      },
      getGitDefaultHead: async () => ({
        defaultBranch: "master",
        head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      }),
      stderr: (value) => cratesErrors.push(value),
    }),
    1,
  )
  assert.match(cratesErrors.join(""), /crates.io tree-sitter-swift: rate limited/)

  const githubErrors = []
  assert.equal(
    await runCli([], {
      loadInputs: async () => ({
        manifestText: MANIFEST_FIXTURE,
        lockText: LOCK_FIXTURE,
      }),
      getLatestStable: async (packageName) =>
        packageName === "tree-sitter" ? "0.26.11" : "0.7.3",
      getGitDefaultHead: async (repository) => {
        throw new Error(`GitHub ${repository}: unavailable`)
      },
      stderr: (value) => githubErrors.push(value),
    }),
    1,
  )
  assert.match(githubErrors.join(""), /GitHub anortham\/tree-sitter-c-sharp: unavailable/)
})

test("malformed local metadata identifies Cargo.toml or Cargo.lock", async () => {
  await assert.rejects(
    () =>
      createFreshnessReport({
        manifestText: "[dependencies]\nserde = \"1\"\n",
        lockText: LOCK_FIXTURE,
        generatedAt: "2026-07-14T14:00:00.000Z",
        getLatestStable: async () => "1.0.0",
        getGitDefaultHead: async () => ({
          defaultBranch: "main",
          head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }),
      }),
    /Cargo.toml: no Tree-sitter parser dependencies/,
  )
  await assert.rejects(
    () =>
      createFreshnessReport({
        manifestText: MANIFEST_FIXTURE,
        lockText: "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n",
        generatedAt: "2026-07-14T14:00:00.000Z",
        getLatestStable: async () => "1.0.0",
        getGitDefaultHead: async () => ({
          defaultBranch: "main",
          head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }),
      }),
    /Cargo.lock: no resolution for tree-sitter-c-sharp/,
  )
})

test("importing the report module does not call fetch or run the CLI", async () => {
  const originalFetch = globalThis.fetch
  let calls = 0
  globalThis.fetch = async () => {
    calls += 1
    throw new Error("network call during import")
  }
  try {
    await import(`./grammar-freshness-report.mjs?import-test=${Date.now()}`)
  } finally {
    globalThis.fetch = originalFetch
  }
  assert.equal(calls, 0)
})
