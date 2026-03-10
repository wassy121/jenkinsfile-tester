# jenkinsfile-tester — Backlog

Consolidated from architect reviews, reviewer verdicts, testing-agent gap analysis,
and Jenkins Declarative Pipeline spec research.

## Status summary (as of 2026-03-02 — documentation + tester expansion complete)

- **407 tests passing (177 unit + 230 integration); 0 FAILING**
- Sprint 1: COMPLETED — correctness & security fixes
- Sprint 2: COMPLETED — ast::walk, PipelineOption enum, grammar ordering, nested grammar (GL-002), Typed Trigger
- Sprint 3: COMPLETED — PluginRegistry, StageBody enum, builtin JSON
- Sprint 4a: COMPLETED — 7 failure fixes, OnceLock, Stage.options/tools, ParseError, get_pipeline_summary
- Test consolidation (Option B original): COMPLETED — closed all high/medium TGAPs
- Sprint 4b: COMPLETED — Typed Binding, init_registry WASM, input stage directive, GL-002 nested grammar
- Sprint 4c: COMPLETED — full matrix directive, failFast on Stage, Typed Trigger enum
- Option A (validator depth): COMPLETED — E005, W002 word-boundary fix, S005, TGAP-001/003
- Option B (AST completeness): COMPLETED — when conditions/modifiers, post unsuccessful, file/run params, libraries block
- ARC-003-TASK: COMPLETED (285→289 tests) — `PluginContributions.steps` → `Vec<StepEntry { name, args }>` with backwards-compat deserialization
- Option C (plugin DSL extensibility): COMPLETED (289→300 tests) — PLUGIN-005/006 already done; added PLUGIN-011 `ValidationMode`, PLUGIN-012 `get_unknown_keywords`, `validate_strict` WASM
- PLUGIN-007/008/009: COMPLETED (300→314 tests) — `Agent::Generic` catch-all + W008; `PipelineOption::Raw.name` + W009; `Trigger::Raw.name` + W010; `has_agent_type()`
- **choice param fix**: COMPLETED (314→318 tests) — `list_literal` grammar rule; `param_arg` extended; 4 new integration tests covering array form, legacy newline string, empty list, trailing comma
- **PLUGIN-013/014/015**: COMPLETED (318→324 tests) — `get_builtin_registry()`, `validate_with_exact_registry()`, frontend plugin drawer in `demo/index.html`
- **Sprint 5 D-001–D-005**: COMPLETED (324→333 tests) — `Agent::Docker` full fields (customWorkspace, reuseNode, registryUrl, registryCredentialsId, alwaysPull), `Agent::Dockerfile` full fields (additionalBuildArgs, label), `Agent::Node { label, custom_workspace }`, TGAP-016 unit test, TGAP-021 comment
- **Sprint 6 E-001–E-005**: COMPLETED (333→340 tests) — `EnvValue` typed enum (Literal/Credentials), W002 guard updated to use pattern match, tester `no_plaintext_secrets` guard updated, `collect_env_vars_for_stage` walk helper, `PipelineOption` camelCase serde renames
- **Sprint 7 K-001–K-005**: COMPLETED (340→349 tests) — `Agent::Kubernetes` typed variant with 8 sub-fields (`yaml`, `yamlFile`, `cloud`, `namespace`, `defaultContainer`, `inheritFrom`, `retries`, `label`); `agent_kubernetes_entry` grammar rule before `agent_generic_entry`; `get_pipeline_summary` updated; 9 new integration tests
- **Sprint 8 S-001–S-006**: COMPLETED (349→355 tests) — `Location { line, col }` AST type; `location: Option<Location>` on `Stage`, `Parameter` variants, `Step::Sh`; `extract_location()` parser helper; E002/E003/E004/W005 diagnostics now carry real line/col; 6 new integration tests. Rules still with `location: None`: E001, W001-W004, W006-W010, S001-S005.
- **Sprint 9 LOC/E006/API/UI**: COMPLETED (355→369 tests) — E001 repurposed to "no agent" with sentinel loc; E006 new rule "no stages" with sentinel loc; S004 sentinel loc; `get_validation_rules()` WASM fn; localStorage plugin persistence (UI-002); custom plugin add form (UI-001).
- **Tester expansion + documentation**: COMPLETED (369→407 tests) — 11 new structural assertions based on CloudBees/Jenkins best practices research (has_build_discarder, no_agent_any, docker_images_pinned, input_stages_no_agent, when_stages_use_before_agent, no_secret_string_parameters, libraries_pinned_to_version, script_blocks_are_small, checkout_not_duplicated, deploy_stages_disable_concurrent, post_failure_handler_exists). run_tests() now runs 21 assertions. README.md, ARCHITECTURE.md, TESTS.md fully rewritten as user-facing documentation.

---

## Completed work (summary)

### Validator rules (18 total)

| Rule | Description |
|------|-------------|
| E001 | No agent defined at pipeline level |
| E002 | Required steps block missing (recurses into parallel branches) |
| E003 | Duplicate stage names at same level |
| E004 | Duplicate parameter names |
| E005 | Duplicate section declarations (e.g. two `agent` blocks) |
| W001 | Missing timeout option |
| W002 | Plaintext credential in environment variable name (word-boundary aware) |
| W003 | Shell steps without `set -e` |
| W004 | Missing post section |
| W005 | Hardcoded secret-like value in script (scans names + values; detects ghp_, AKIA, JWT, hex) |
| W006 | Tool type not in plugin registry |
| W007 | Unknown step name (`Step::Generic`) not in registry; Warning/Error/silent per `ValidationMode` |
| W008 | `Agent::Generic` type not in registry `agent_types` |
| W009 | `PipelineOption::Raw` name not in registry `options` |
| W010 | `Trigger::Raw` name not in registry `triggers` |
| S001 | No parameters defined |
| S002 | No timeout option defined |
| S003 | Long pipeline with no stages |
| S004 | Deep nesting detected |
| S005 | `allOf`/`anyOf` with fewer than 2 child conditions |

### AST types implemented

- `Pipeline`: agent, environment, options (`Vec<PipelineOption>`), triggers (`Vec<Trigger>`), parameters, stages, post, libraries (`Vec<SharedLibrary>`), duplicate_sections
- `Stage`: name, agent, environment, when, options, tools, input (`Option<StageInput>`), body (`StageBody`), post, fail_fast, duplicate_sections
- `StageBody`: Steps | Parallel { stages } | Sequential { stages } | Matrix { axes, excludes, stages }
- `PipelineOption`: 14 typed variants (Timeout, BuildDiscarder with typed logRotator fields, Retry, DisableConcurrentBuilds, SkipDefaultCheckout, etc.) + Raw fallback
- `Trigger`: Cron { spec } | PollScm { spec } | Upstream { projects, threshold } | GithubPush | Raw
- `Binding`: UsernamePassword | StringBinding | FileBinding | SshUserPrivateKey | Certificate | Raw
- `Parameter`: String | Boolean | Choice | Text | Password | File | Run (with filter) — all with name/default/description
- `WhenCondition`: Branch | Tag | Environment | Expression | Not | AllOf | AnyOf | ChangeRequest | BuildingTag | Changelog | Changeset | TriggeredBy | Equals | Generic
- `When`: conditions, before_agent, before_input, before_options
- `PostCondition`: Always | Success | Failure | Unstable | Aborted | Changed | Fixed | Regression | Cleanup | Unsuccessful
- `Binding` (withCredentials): 5 typed variants + Raw
- `SharedLibrary`: name, ref_ (from `lib('name@ref')`)
- `StageInput`: message, ok, submitter, submitter_parameter, parameters

### WASM API (10 functions)

| Function | Description |
|----------|-------------|
| `parse_jenkinsfile(src)` | Parse → AST JSON, with line/col error |
| `validate(src)` | Validate using thread-local or builtin registry (Permissive mode) |
| `validate_strict(src)` | Like `validate()` but W007 unknown steps become errors |
| `validate_with_registry(src, json)` | Per-call registry merge (Permissive mode) |
| `init_registry(json)` | Store merged registry in thread-local |
| `get_unknown_keywords(src)` | Sorted JSON array of unregistered step names found in pipeline |
| `run_tests(src)` | 21 structural assertions |
| `get_ast_json(src)` | Full AST as JSON |
| `get_stage_names(src)` | Recursive stage name list |
| `get_pipeline_summary(src)` | Summary metadata object |

---

## Open test gaps (low severity only)

| ID | Area | Description | Status |
|----|------|-------------|--------|
| TGAP-016 | ast_walk | `collect_all_stages` not tested with empty slice | COMPLETED (Sprint 5 D-004) |
| TGAP-021 | wasm_api | `validate()` serialization-failure fallback path is dead code with no test | COMPLETED (Sprint 5 D-005 — resolved-by-comment) |

---

## Architect items — open

| ID | Title | Effort | Notes |
|----|-------|--------|-------|
| ~~ARC-003-TASK~~ | ~~Replace `PluginContributions.steps Vec<String>` with `Vec<StepEntry { name, args }>`~~ | medium | **DONE** — backwards-compat string deserialization included |
| ~~ARC-008-TASK~~ | ~~Add `collect_env_vars_for_stage` walk helper for effective env with inheritance~~ | small | **DONE** — Sprint 6 E-004 |
| ~~ARC-014~~ | ~~Consider `#[serde(rename)]` on PipelineOption variants to match Jenkins canonical names~~ | low | **DONE** — Sprint 6 E-005 |

---

## Plugin-aware DSL — remaining work

Foundation (PLUGIN-001–004, PLUGIN-010) is complete. Remaining items:

| # | ID | Title | Effort | Status |
|---|----|----|--------|--------|
| P5 | PLUGIN-005 | Extend step parsing to treat unknown steps as `Step::Generic { name, args }` | medium | **DONE** |
| P6 | PLUGIN-006 | Add validator rule W007: unknown step name warning | small | **DONE** |
| P7 | PLUGIN-007 | Extend agent parser to support plugin-registered agent types | medium | **DONE** |
| P8 | PLUGIN-008 | Extend options parser to validate plugin-registered option names | small | **DONE** |
| P9 | PLUGIN-009 | Extend trigger parser to accept plugin-registered trigger names | small | **DONE** |
| P11 | PLUGIN-011 | Add validation mode flag (strict / permissive / discovery) | small | **DONE** |
| P12 | PLUGIN-012 | Add `get_unknown_keywords(src: String)` WASM function for discovery mode | small | **DONE** |

---

## Backlog — remaining low priority / future

| ID | Title | Effort | Notes |
|----|-------|--------|-------|
| ARC-008-TASK | Add `collect_env_vars_for_stage` walk helper for effective env with inheritance | small | Useful for credential-aware rules |
| ARC-014 | Consider `#[serde(rename)]` on PipelineOption variants to match Jenkins canonical names | low | API surface polish |
| ~~REC-013~~ | ~~Implement span-aware location reporting in `Diagnostic.location`~~ | large | **DONE** — Sprint 8 S-001–S-006; partial (E002/E003/E004/W005 have location; others remain None) |
| NEW / AGENT | Expand `docker` fields: `customWorkspace`, `reuseNode`, `registryUrl`, `registryCredentialsId`, `alwaysPull` | small | Grammar rules for most already exist |
| NEW / AGENT | Expand `dockerfile` fields: `additionalBuildArgs` | trivial | |
| NEW / AGENT | Parse `Agent::Generic` body for `kubernetes` sub-fields | medium | Currently opaque; needs nested grammar |
| NEW / AGENT | Add `node { label; customWorkspace }` agent form | small | Distinct from bare label shorthand |
| NEW / ENV | Model `credentials()` in environment as typed `CredentialsRef` | small | Currently raw string; generates `_USR`/`_PSW` vars |
| TGAP-016 | `collect_all_stages` not tested with empty slice | trivial | |
| TGAP-021 | `validate()` serialization-failure fallback path is dead code | trivial | |

---

## Sprint plan — remaining backlog

Sizing reference (from session history): trivial ≈ 15 min, small ≈ 30–45 min, medium ≈ 1–2 hr, large ≈ 3–4 hr.
A pro-plan session comfortably fits ~1 large + 2–3 small items, or 4–6 small/trivial items.

---

### Sprint 5 — Agent completeness  `[COMPLETED]`

**Goal:** Round out the agent block — fill in all `docker` / `dockerfile` sub-fields that are
already stubbed in the grammar, add the `node` agent form, and close the two trivial test gaps.

| ID | Item | Effort | Notes |
|----|------|--------|-------|
| D-001 | Populate `Agent::Docker` with full field set: `customWorkspace`, `reuseNode`, `registryUrl`, `registryCredentialsId`, `alwaysPull` | small | COMPLETED |
| D-002 | Populate `Agent::Dockerfile` with `additionalBuildArgs` and `label` | trivial | COMPLETED |
| D-003 | Add `Agent::Node { label, custom_workspace }` — the `node { label '…'; customWorkspace '…' }` agent form | small | COMPLETED |
| D-004 | Close TGAP-016: `collect_all_stages` with empty slice | trivial | COMPLETED |
| D-005 | Close TGAP-021: `validate()` serialization-fallback path | trivial | COMPLETED — resolved-by-comment |

**Estimated new tests:** +8–10
**Files touched:** `jenkinsfile.pest` (D-003), `src/ast/mod.rs`, `src/parser/mod.rs`, `tests/integration.rs`

**Start here:** read `docker_field` alternatives in `jenkinsfile.pest` and `build_agent` in
`src/parser/mod.rs` to see exactly which fields are already parsed but dropped.

---

### Sprint 6 — Environment depth & API polish `[COMPLETED]`

**Goal:** Make credential values in `environment {}` first-class typed objects so W002/W005
can suppress false positives; add the env-inheritance walk helper; clean up serde names.

| ID | Item | Effort | Notes |
|----|------|--------|-------|
| E-001 | `EnvValue::Credentials { id }` typed variant — replace the current raw-string fallback for `credentials('...')` in `env_entry` | small | COMPLETED |
| E-002 | Update W002 to suppress false-positive on `EnvValue::Credentials` — a `DEPLOY_KEY = credentials('x')` is not a plaintext credential | small | COMPLETED |
| E-003 | Update W005 (hardcoded secret scan) to skip `EnvValue::Credentials` values | trivial | COMPLETED |
| E-004 | ARC-008: Add `collect_env_vars_for_stage(stage, pipeline)` walk helper — returns effective `HashMap<String, EnvValue>` for a stage including inherited pipeline-level env | small | COMPLETED |
| E-005 | ARC-014: Add `#[serde(rename = "...")]` to `PipelineOption` variants to match Jenkins canonical names (e.g. `BuildDiscarder` → `"buildDiscarder"`) | small | COMPLETED |

**Estimated new tests:** +10–12
**Files touched:** `src/ast/mod.rs`, `src/ast/walk.rs`, `src/parser/mod.rs`, `src/validator/rules.rs`, `tests/integration.rs`

**Key constraint:** `EnvValue` currently serializes as a plain string for `credentials()`
(the whole call text).  Changing it to a typed enum is a **breaking API change** for
consumers reading the AST JSON — document in CLAUDE.md.

---

### Sprint 7 — Kubernetes agent body parsing `[COMPLETED]`

**Goal:** Replace the opaque `Agent::Generic` catch-all for `kubernetes {}` with a proper
typed struct, enabling W008 to give actionable feedback and future rules to inspect the pod spec.

| ID | Item | Effort | Notes |
|----|------|--------|-------|
| K-001 | Grammar: add `agent_kubernetes_entry` rule with named sub-fields (`yaml`, `yamlFile`, `cloud`, `namespace`, `defaultContainer`, `inheritFrom`, `retries`, `label`) | medium | COMPLETED |
| K-002 | AST: `Agent::Kubernetes { yaml, yaml_file, cloud, namespace, default_container, inherit_from, retries, label }` — all `Option<String>` | small | COMPLETED |
| K-003 | Parser: `build_agent` dispatches `kubernetes` to new builder branch | small | COMPLETED |
| K-004 | Update W008 to not fire for `kubernetes` (now a typed variant, not `Agent::Generic`) | trivial | COMPLETED — no code change needed; W008 fires only on `Agent::Generic` |
| K-005 | Integration tests: `yaml` field, `yamlFile` field, `cloud` + `namespace`, unknown sub-field is ignored gracefully | medium | COMPLETED — 9 new tests in `mod sprint7_kubernetes` |

**Estimated new tests:** +8–12
**Files touched:** `jenkinsfile.pest`, `src/ast/mod.rs`, `src/parser/mod.rs`, `src/validator/rules.rs`, `tests/integration.rs`

**Open question before starting:** Does `yaml: '...'` in a real kubernetes block use a
triple-quoted string (multi-line YAML)?  Check real-world Jenkinsfiles.  If yes, the grammar
must allow `triple_string` in the kubernetes sub-field values, which `value` already covers.

---

### Sprint 8 — Span-aware diagnostics  `[COMPLETED]`

**Goal:** Populate `Diagnostic.location` with real line/col numbers so users can click through
errors to the offending line.  Previously `location` was always `null`.

| ID | Item | Effort | Notes |
|----|------|--------|-------|
| S-001 | Add `Location { line: u32, col: u32 }` type to `src/ast/mod.rs` | small | COMPLETED |
| S-002 | Add `location: Option<Location>` to `Stage`, all `Parameter` variants, and `Step::Sh` | medium | COMPLETED — `#[serde(skip_serializing_if = "Option::is_none")]` on each field |
| S-003 | Add `extract_location(pair: &Pair) → Option<Location>` helper in `src/parser/mod.rs` | small | COMPLETED — uses `pair.as_span().start_pos().line_col()` |
| S-004 | Populate `location` in parser builders: `build_stage`, `build_parameters`, `build_step` (sh) | medium | COMPLETED |
| S-005 | Update diagnostic rules E002, E003, E004, W005 to copy location from AST node | medium | COMPLETED — other rules still emit `location: None` |
| S-006 | Integration tests: assert `location.line >= 1` for E002, E003, E004, W005, AST stage | small | COMPLETED — 6 new tests in `mod sprint8_locations` |

**Estimated new tests:** +8–12
**Files touched:** `src/ast/mod.rs` (location field on nodes), `src/parser/mod.rs` (span extraction), `src/validator/context.rs`, `src/validator/rules.rs` (all rules), `tests/integration.rs`

**This is the most architecturally invasive sprint.** Two viable approaches:

- **Approach A (AST-embedded spans):** Add `location: Option<Location>` to every AST node
  at parse time.  Rules read `node.location`.  Pros: clean; cons: inflates AST struct size,
  changes serialised JSON shape.

- **Approach B (side-table):** Keep AST unchanged; build a `HashMap<NodeId, Location>` in
  the parser alongside the AST.  Pass it into `ValidationContext`.  Pros: no AST JSON change;
  cons: requires node IDs or a parallel structure.

**Recommendation:** Approach A, but make `location` `#[serde(skip_serializing_if = "Option::is_none")]`
so it only appears in JSON when set.  Start with Stage (for E003) and Step (for W003/W005)
as the first two nodes.

---

## Remaining future work

All planned sprints (5–8) are complete. The project is in a stable, well-tested state.
Items below are quality-of-life improvements with no blocking dependencies.

### High value / low effort — ALL COMPLETED (Sprint 9)

| ID | Title | Effort | Status |
|----|-------|--------|--------|
| LOC-001 | Extend `location` population to W005 (sh without set -e) | small | **DONE** — W005 carries Step::Sh location (Sprint 8+9) |
| LOC-002 | Sentinel locations for E001 (no agent), S004 (no post) | small | **DONE** — both carry `Location { line: 1, col: 1 }` |
| UI-001 | Frontend: custom plugin entry form | small | **DONE** — `#plugin-add-row` + `addCustomPlugin()` |
| UI-002 | Frontend: `localStorage` persistence for plugin selection | trivial | **DONE** — `savePluginState()` / `initPluginDrawer()` restore |
| API-001 | `get_validation_rules()` WASM fn | small | **DONE** — returns 21 rules as JSON array |
| E006 | Validator: E006 — no stages defined | trivial | **DONE** — `check_e006_empty_stages`; E001 now = no agent |

### Low value / large effort (likely won't fix)

| ID | Title | Effort | Notes |
|----|-------|--------|-------|
| — | `format_pipeline(src)` WASM fn — pretty-print/normalise a Jenkinsfile | large | Needs a Groovy-aware pretty-printer; out of scope |
| — | Escaped quotes in strings (`'it\'s'`, `"say \"hi\""`) | medium | PEG grammar limitation; low real-world impact |
| — | Re-parse `kubernetes` agent `yaml` body as actual YAML | large | Requires a YAML parser dep; out of scope |
| — | Span-aware `location` for S001–S005 suggestion rules | small | Low diagnostic value; suggestions are pipeline-level |

---

## Known permanent limitations (won't fix)

| Limitation | Reason |
|-----------|--------|
| `script {}` body content not re-parsed | Contents are arbitrary Groovy; full Groovy parser out of scope |
| Escaped quotes in strings (`'it\'s'`) | PEG grammar limitation; low real-world impact |
| `expression { ... }` when condition with closures containing `}` | `expression_body` uses heuristic lookahead; full Groovy expression parsing out of scope |
