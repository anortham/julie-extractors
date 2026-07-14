import fs from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const MODULE_PATH = fileURLToPath(import.meta.url)
const ROOT = path.resolve(path.dirname(MODULE_PATH), "..")
const MANIFEST_PATH = "crates/julie-extractors/Cargo.toml"
const LOCK_PATH = "Cargo.lock"
const FULL_COMMIT = /^[0-9a-f]{40}$/
const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/
const USAGE =
  "usage: node scripts/grammar-freshness-report.mjs [--format text|json]"

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0
}

function stripTomlComment(line) {
  let quoted = false
  let escaped = false
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index]
    if (escaped) {
      escaped = false
      continue
    }
    if (character === "\\" && quoted) {
      escaped = true
      continue
    }
    if (character === '"') {
      quoted = !quoted
      continue
    }
    if (character === "#" && !quoted) {
      return line.slice(0, index)
    }
  }
  return line
}

function quotedField(value, field) {
  const match = value.match(new RegExp(`(?:^|[,\\s])${field}\\s*=\\s*"([^"]*)"`))
  return match?.[1]
}

function dependencyEntries(manifestText) {
  const lines = manifestText.split(/\r?\n/)
  const entries = []
  let inDependencies = false
  let pending = ""
  let braces = 0

  for (const originalLine of lines) {
    const line = stripTomlComment(originalLine).trim()
    if (!pending && /^\[[^[]/.test(line)) {
      inDependencies = line === "[dependencies]"
      continue
    }
    if (!inDependencies || (!line && !pending)) {
      continue
    }
    pending = pending ? `${pending} ${line}` : line
    braces += [...line].filter((value) => value === "{").length
    braces -= [...line].filter((value) => value === "}").length
    if (braces > 0) {
      continue
    }
    const match = pending.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/)
    if (!match) {
      throw new Error(`Cargo.toml: malformed dependency entry: ${pending}`)
    }
    entries.push({ dependency: match[1], value: match[2].trim() })
    pending = ""
  }

  if (pending) {
    throw new Error(`Cargo.toml: unterminated dependency entry: ${pending}`)
  }
  return entries
}

export function parseManifestParserDependencies(manifestText) {
  const dependencies = dependencyEntries(manifestText)
    .map(({ dependency, value }) => {
      const packageName = quotedField(value, "package") ?? dependency
      if (packageName !== "tree-sitter" && !packageName.startsWith("tree-sitter-")) {
        return null
      }
      const simpleVersion = value.match(/^"([^"]+)"$/)?.[1]
      const git = quotedField(value, "git")
      if (git) {
        const rev = quotedField(value, "rev")
        if (!FULL_COMMIT.test(rev ?? "")) {
          throw new Error(
            `Cargo.toml: Git parser ${dependency} must use a lowercase 40-character rev`,
          )
        }
        return {
          dependency,
          package: packageName,
          source: "git",
          remote: git,
          rev,
        }
      }
      const requirement = simpleVersion ?? quotedField(value, "version")
      if (!requirement) {
        throw new Error(
          `Cargo.toml: registry parser ${dependency} has no version requirement`,
        )
      }
      return {
        dependency,
        package: packageName,
        source: "registry",
        requirement,
      }
    })
    .filter(Boolean)
    .sort((left, right) => compareText(left.dependency, right.dependency))

  if (dependencies.length === 0) {
    throw new Error("Cargo.toml: no Tree-sitter parser dependencies")
  }
  return dependencies
}

function lockPackage(block) {
  const field = (name) =>
    block.match(new RegExp(`^${name}\\s*=\\s*"([^"]*)"`, "m"))?.[1]
  const packageName = field("name")
  if (!packageName) {
    throw new Error("Cargo.lock: package entry has no name")
  }
  return {
    package: packageName,
    version: field("version"),
    source: field("source"),
  }
}

export function parseCargoLock(lockText) {
  return lockText
    .split(/^\[\[package\]\]\s*$/m)
    .slice(1)
    .map(lockPackage)
    .filter(({ package: packageName }) =>
      packageName === "tree-sitter" || packageName.startsWith("tree-sitter-"),
    )
    .sort(
      (left, right) =>
        compareText(left.package, right.package) ||
        compareText(left.source ?? "", right.source ?? "") ||
        compareText(left.version ?? "", right.version ?? ""),
    )
}

export function normalizeGitHubRepository(remote) {
  let value = remote.trim().replace(/^git\+/, "")
  value = value.split(/[?#]/, 1)[0]
  const scpMatch = value.match(/^git@github\.com:([^/]+\/[^/]+)$/i)
  if (scpMatch) {
    value = `https://github.com/${scpMatch[1]}`
  }

  let parsed
  try {
    parsed = new URL(value)
  } catch {
    throw new Error(`GitHub remote is invalid: ${remote}`)
  }
  if (parsed.hostname.toLowerCase() !== "github.com") {
    throw new Error(`GitHub remote must use github.com: ${remote}`)
  }
  const parts = parsed.pathname.replace(/^\/+|\/+$/g, "").split("/")
  if (parts.length !== 2 || parts.some((part) => !part)) {
    throw new Error(`GitHub remote must identify owner/repository: ${remote}`)
  }
  parts[1] = parts[1].replace(/\.git$/i, "")
  return parts.join("/")
}

function parseSemanticVersion(version) {
  const match = version.match(SEMVER)
  if (!match) {
    throw new Error(`invalid semantic version: ${version}`)
  }
  return {
    core: [Number(match[1]), Number(match[2]), Number(match[3])],
    prerelease: match[4]?.split(".") ?? [],
  }
}

export function compareSemanticVersions(leftVersion, rightVersion) {
  const left = parseSemanticVersion(leftVersion)
  const right = parseSemanticVersion(rightVersion)
  for (let index = 0; index < left.core.length; index += 1) {
    if (left.core[index] !== right.core[index]) {
      return left.core[index] - right.core[index]
    }
  }
  if (left.prerelease.length === 0 || right.prerelease.length === 0) {
    return left.prerelease.length === right.prerelease.length
      ? 0
      : left.prerelease.length === 0
        ? 1
        : -1
  }
  const length = Math.max(left.prerelease.length, right.prerelease.length)
  for (let index = 0; index < length; index += 1) {
    const leftPart = left.prerelease[index]
    const rightPart = right.prerelease[index]
    if (leftPart === undefined || rightPart === undefined) {
      return leftPart === rightPart ? 0 : leftPart === undefined ? -1 : 1
    }
    if (leftPart === rightPart) {
      continue
    }
    const leftNumeric = /^\d+$/.test(leftPart)
    const rightNumeric = /^\d+$/.test(rightPart)
    if (leftNumeric && rightNumeric) {
      return Number(leftPart) - Number(rightPart)
    }
    if (leftNumeric !== rightNumeric) {
      return leftNumeric ? -1 : 1
    }
    return compareText(leftPart, rightPart)
  }
  return 0
}

export function latestStableVersion(versions) {
  return versions
    .filter((version) => !version.yanked)
    .map((version) => version.num)
    .filter(
      (version) =>
        typeof version === "string" &&
        parseSemanticVersion(version).prerelease.length === 0,
    )
    .sort(compareSemanticVersions)
    .at(-1)
}

function lockResolution(dependency, lockPackages) {
  const candidates = lockPackages.filter((candidate) => {
    if (candidate.package !== dependency.package) {
      return false
    }
    if (dependency.source === "registry") {
      return candidate.source?.startsWith("registry+")
    }
    if (!candidate.source?.startsWith("git+")) {
      return false
    }
    try {
      return (
        normalizeGitHubRepository(candidate.source) ===
        normalizeGitHubRepository(dependency.remote)
      )
    } catch {
      return false
    }
  })
  if (candidates.length !== 1) {
    throw new Error(
      `Cargo.lock: ${candidates.length === 0 ? "no resolution" : "multiple resolutions"} for ${dependency.package}`,
    )
  }
  return candidates[0]
}

function gitLockedRev(source) {
  const rev = source?.match(/#([0-9a-f]{40})$/)?.[1]
  if (!rev) {
    throw new Error("Cargo.lock: Git parser source has no exact locked commit")
  }
  return rev
}

export async function createFreshnessReport({
  manifestText,
  lockText,
  generatedAt,
  manifestPath = MANIFEST_PATH,
  lockPath = LOCK_PATH,
  getLatestStable,
  getGitDefaultHead,
}) {
  const dependencies = parseManifestParserDependencies(manifestText)
  const lockPackages = parseCargoLock(lockText)
  const runtimeDependencies = dependencies.filter(
    ({ package: packageName }) => packageName === "tree-sitter",
  )
  if (runtimeDependencies.length !== 1) {
    throw new Error("Cargo.toml: expected exactly one tree-sitter runtime")
  }

  const registryRows = []
  const gitRows = []
  let runtime
  for (const dependency of dependencies) {
    const locked = lockResolution(dependency, lockPackages)
    if (dependency.source === "registry") {
      if (!locked.version) {
        throw new Error(`Cargo.lock: ${dependency.package} has no locked version`)
      }
      const latest = await getLatestStable(dependency.package)
      parseSemanticVersion(latest)
      const row = {
        dependency: dependency.dependency,
        package: dependency.package,
        declared_requirement: dependency.requirement,
        locked_version: locked.version,
        latest_stable_version: latest,
        status: locked.version === latest ? "current" : "drift",
      }
      if (dependency.package === "tree-sitter") {
        runtime = row
      } else {
        registryRows.push(row)
      }
      continue
    }

    const repository = normalizeGitHubRepository(dependency.remote)
    const lockedRev = gitLockedRev(locked.source)
    if (lockedRev !== dependency.rev) {
      throw new Error(
        `Cargo.lock: ${dependency.package} locked commit does not match Cargo.toml rev`,
      )
    }
    const remote = await getGitDefaultHead(repository)
    if (!remote?.defaultBranch || !FULL_COMMIT.test(remote?.head ?? "")) {
      throw new Error(`GitHub ${repository}: malformed default-head metadata`)
    }
    gitRows.push({
      dependency: dependency.dependency,
      package: dependency.package,
      remote: dependency.remote,
      repository,
      pinned_rev: dependency.rev,
      locked_rev: lockedRev,
      remote_default_branch: remote.defaultBranch,
      remote_head: remote.head,
      status: dependency.rev === remote.head ? "current" : "drift",
    })
  }

  registryRows.sort((left, right) => compareText(left.dependency, right.dependency))
  gitRows.sort((left, right) => compareText(left.dependency, right.dependency))
  return {
    schema_version: 1,
    audit: {
      generated_at: generatedAt,
      manifest_path: manifestPath,
      lock_path: lockPath,
    },
    runtime,
    registry_grammars: registryRows,
    git_grammars: gitRows,
  }
}

export function renderReport(report, format) {
  if (format === "json") {
    return `${JSON.stringify(report, null, 2)}\n`
  }
  if (format !== "text") {
    throw new Error(`unsupported report format: ${format}`)
  }
  const lines = [
    `Grammar freshness audit: ${report.audit.generated_at}`,
    "Runtime",
    `  ${report.runtime.dependency} [${report.runtime.package}] declared ${report.runtime.declared_requirement}, locked ${report.runtime.locked_version}, latest stable ${report.runtime.latest_stable_version}: ${report.runtime.status}`,
    "Registry grammars",
    ...report.registry_grammars.map(
      (row) =>
        `  ${row.dependency} [${row.package}] declared ${row.declared_requirement}, locked ${row.locked_version}, latest stable ${row.latest_stable_version}: ${row.status}`,
    ),
    "Git grammars",
    ...report.git_grammars.map(
      (row) =>
        `  ${row.dependency} [${row.repository}] pinned ${row.pinned_rev}, locked ${row.locked_rev}, ${row.remote_default_branch} ${row.remote_head}: ${row.status}`,
    ),
  ]
  return `${lines.join("\n")}\n`
}

export function parseCliArgs(args) {
  if (args.length === 0) {
    return { format: "text" }
  }
  if (
    args.length === 2 &&
    args[0] === "--format" &&
    (args[1] === "text" || args[1] === "json")
  ) {
    return { format: args[1] }
  }
  throw new Error(USAGE)
}

async function fetchJson({ fetchImpl, headers, source, timeoutMs, url }) {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), timeoutMs)
  let response
  try {
    response = await fetchImpl(url, { headers, signal: controller.signal })
  } catch (error) {
    if (controller.signal.aborted) {
      throw new Error(`${source}: timed out after ${timeoutMs}ms`)
    }
    throw new Error(`${source}: ${error instanceof Error ? error.message : String(error)}`)
  } finally {
    clearTimeout(timeout)
  }
  if (!response?.ok) {
    throw new Error(`${source}: HTTP ${response?.status ?? "unknown"}`)
  }
  try {
    return await response.json()
  } catch (error) {
    throw new Error(
      `${source}: invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
    )
  }
}

export function createCratesIoAdapter({
  fetchImpl = globalThis.fetch,
  timeoutMs = 10_000,
} = {}) {
  if (typeof fetchImpl !== "function") {
    throw new Error("crates.io adapter requires fetch")
  }
  return async (packageName) => {
    const source = `crates.io ${packageName}`
    const metadata = await fetchJson({
      fetchImpl,
      headers: {
        Accept: "application/json",
        "User-Agent": "julie-extractors grammar-freshness-report",
      },
      source,
      timeoutMs,
      url: `https://crates.io/api/v1/crates/${encodeURIComponent(packageName)}`,
    })
    if (!Array.isArray(metadata?.versions)) {
      throw new Error(`${source}: malformed package metadata`)
    }
    let latest
    try {
      latest = latestStableVersion(metadata.versions)
      if (latest) {
        parseSemanticVersion(latest)
      }
    } catch (error) {
      throw new Error(
        `${source}: malformed version metadata: ${
          error instanceof Error ? error.message : String(error)
        }`,
      )
    }
    if (!latest) {
      throw new Error(`${source}: metadata has no stable release`)
    }
    return latest
  }
}

export function createGitHubAdapter({
  fetchImpl = globalThis.fetch,
  timeoutMs = 10_000,
  token = process.env.GITHUB_TOKEN ?? process.env.GH_TOKEN,
} = {}) {
  if (typeof fetchImpl !== "function") {
    throw new Error("GitHub adapter requires fetch")
  }
  const headers = {
    Accept: "application/vnd.github+json",
    "User-Agent": "julie-extractors grammar-freshness-report",
    "X-GitHub-Api-Version": "2026-03-10",
  }
  if (token) {
    headers.Authorization = `Bearer ${token}`
  }
  return async (repository) => {
    const source = `GitHub ${repository}`
    const metadata = await fetchJson({
      fetchImpl,
      headers,
      source,
      timeoutMs,
      url: `https://api.github.com/repos/${repository}`,
    })
    if (typeof metadata?.default_branch !== "string" || !metadata.default_branch) {
      throw new Error(`${source}: repository metadata has no default branch`)
    }
    const commit = await fetchJson({
      fetchImpl,
      headers,
      source,
      timeoutMs,
      url: `https://api.github.com/repos/${repository}/commits/${encodeURIComponent(
        metadata.default_branch,
      )}`,
    })
    if (!FULL_COMMIT.test(commit?.sha ?? "")) {
      throw new Error(`${source}: default-branch commit metadata has no full commit ID`)
    }
    return {
      defaultBranch: metadata.default_branch,
      head: commit.sha,
    }
  }
}

async function loadRepositoryInputs() {
  const [manifestText, lockText] = await Promise.all([
    fs.readFile(path.join(ROOT, MANIFEST_PATH), "utf8"),
    fs.readFile(path.join(ROOT, LOCK_PATH), "utf8"),
  ])
  return { manifestText, lockText }
}

export async function runCli(
  args,
  {
    getGitDefaultHead = createGitHubAdapter(),
    getLatestStable = createCratesIoAdapter(),
    loadInputs = loadRepositoryInputs,
    now = () => new Date(),
    stderr = (value) => process.stderr.write(value),
    stdout = (value) => process.stdout.write(value),
  } = {},
) {
  let options
  try {
    options = parseCliArgs(args)
  } catch (error) {
    stderr(`${error instanceof Error ? error.message : String(error)}\n`)
    return 2
  }
  try {
    const { manifestText, lockText } = await loadInputs()
    const report = await createFreshnessReport({
      manifestText,
      lockText,
      generatedAt: now().toISOString(),
      manifestPath: MANIFEST_PATH,
      lockPath: LOCK_PATH,
      getLatestStable,
      getGitDefaultHead,
    })
    stdout(renderReport(report, options.format))
    return 0
  } catch (error) {
    stderr(`error: ${error instanceof Error ? error.message : String(error)}\n`)
    return 1
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === MODULE_PATH) {
  process.exitCode = await runCli(process.argv.slice(2))
}
