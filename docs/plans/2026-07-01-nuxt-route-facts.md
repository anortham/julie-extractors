# Nuxt Route Facts Implementation Plan

**Goal:** Add fixture-backed Nuxt route facts before the next patch release so
the framework coverage set includes htmx, Vue, React, Next.js, and Nuxt.

**Current grounding:** Verified against official Nuxt v4 docs on 2026-07-01.
Nuxt file-system routing creates routes from files in `app/pages/` and
`pages/`, and Nuxt supports `.vue`, `.js`, `.jsx`, `.mjs`, `.ts`, and `.tsx`
page files. `<NuxtLink>` links between pages with a `to` prop. Nuxt route
groups use folders wrapped in parentheses and do not affect the URL structure.

Docs checked:

- https://nuxt.com/docs/4.x/getting-started/routing
- https://nuxt.com/docs/4.x/directory-structure/app/pages
- https://nuxt.com/docs/4.x/api/components/nuxt-link

## Existing Framework Support To Preserve

- `htmx.attribute.v1`: capability-backed for `html` and `razor`.
- `vue.route_reference.v1`: capability-backed for `vue`.
- `vue.route_definition.v1`: capability-backed for `vue`.
- `react.route_reference.v1`: capability-backed for `javascript`, `jsx`, and
  `tsx`.
- `react.route_definition.v1`: capability-backed for `javascript`, `jsx`,
  `typescript`, and `tsx`.
- `nextjs.route_reference.v1`: capability-backed for `javascript`, `jsx`, and
  `tsx`.
- `nextjs.file_route.v1`: capability-backed for `javascript`, `jsx`,
  `typescript`, and `tsx`.

## Nuxt Contract Additions

### Nuxt Route References

Add `nuxt.route_reference.v1` for static `<NuxtLink>` navigation targets.

Metadata:

- `framework = "nuxt"`
- `query_family = "frontend_navigation"`
- `target_path = "/about"`
- `attribute_name = "to"`
- `component_name = "NuxtLink"`
- `route_source = "string_literal"`
- `source_kind = "nuxt_link"`

Initial scope:

- Emit for static `<NuxtLink to="/about">` in Vue SFC templates.
- Emit for lowercase `<nuxt-link to="/about">` only when the static target is
  path-like.
- Do not emit for dynamic object targets, named-route objects, variables,
  template expressions, absolute URLs, protocol-relative URLs, or links with an
  explicit `external` attribute.

### Nuxt File Route Definitions

Add `nuxt.file_route.v1` for page routes derived from file paths.

Metadata:

- `framework = "nuxt"`
- `query_family = "frontend_navigation"`
- `router = "pages"`
- `file_convention = "page"`
- `route_path = "/blog/[slug]"`
- `normalized_route_template = "/blog/:slug"` when dynamic segments exist
- `dynamic_segments = ["slug"]` when dynamic segments exist
- `route_group_segments = ["marketing"]` when route groups are present
- `source_kind = "nuxt_file_route"`

Initial scope:

- Support `app/pages/**` and `pages/**`.
- Support `.vue`, `.js`, `.jsx`, `.mjs`, `.ts`, and `.tsx` page files.
- Map `index` pages to their parent route.
- Preserve source-faithful dynamic segment text in `route_path` and emit a
  normalized colon template for dynamic segments.
- Exclude `server/**`, `server/api/**`, `app/server/**`, and non-page
  directories.
- Exclude named-view suffixes such as `child@sidebar.vue` in this patch unless
  a later plan adds an explicit named-view contract.

## Target Files

- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs`
- Modify: `crates/julie-extractors/src/tests/react/structural_facts.rs` or add
  `crates/julie-extractors/src/tests/nuxt/structural_facts.rs`
- Modify: `crates/julie-extractors/src/tests/mod.rs` if adding a Nuxt test
  module
- Modify: `fixtures/extraction/capabilities.json`
- Add fixture coverage under `fixtures/extraction/vue/` or per-language Nuxt
  file-route fixtures as needed
- Modify: `docs/contracts/jsonl-v3.md`
- Modify: `docs/contracts/sqlite-schema-v3.md`
- Modify: `docs/plans/2026-06-09-structural-facts-design.md`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `docs/release-notes/v2.5.8.md`

## Acceptance Criteria

- [ ] Existing htmx, Vue, React, and Next.js structural-fact tests still pass.
- [ ] `<NuxtLink to="/about">` emits `nuxt.route_reference.v1`.
- [ ] `<nuxt-link to="/contact">` emits `nuxt.route_reference.v1`.
- [ ] `<NuxtLink :to="{ name: 'posts-id' }">` does not emit a static path
  reference.
- [ ] `<NuxtLink to="/example.pdf" external>` does not emit a frontend route
  reference.
- [ ] `app/pages/index.vue` emits `nuxt.file_route.v1` with `route_path=/`.
- [ ] `app/pages/(marketing)/blog/[slug].vue` emits `route_path=/blog/[slug]`,
  `normalized_route_template=/blog/:slug`, `dynamic_segments=["slug"]`, and
  `route_group_segments=["marketing"]`.
- [ ] `pages/about.ts` emits `route_path=/about`.
- [ ] `server/api/status.ts` does not emit `nuxt.file_route.v1`.
- [ ] Capability rows and golden fixtures prove the new Nuxt patterns.
- [ ] Strict language data quality remains green.
- [ ] Release preflight for `2.5.8` passes.
