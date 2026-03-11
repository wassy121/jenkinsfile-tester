# jenkinsfile-tester

Rust library that parses Jenkinsfile declarative pipeline DSL, validates it,
and runs structural tests — compiled to WebAssembly via `wasm-bindgen`.

## Build commands

```bash
cargo build                                                      # native debug
cargo build --target wasm32-unknown-unknown                      # verify WASM compiles
./test.sh                                                        # run all tests (handles WSL stale-cache)
./test.sh <filter>                                               # run subset, e.g. ./test.sh parses_matrix
wasm-pack build --target web                                     # browser ESModule → pkg/
wasm-pack build --target nodejs                                  # Node.js → pkg/
cargo build --bin jenkinsfile-tester --target wasm32-wasi --release  # CLI binary for wasmtime
./build-pages.sh                                                    # build WASM + assemble docs/ for GitHub Pages
docker build -t jenkinsfile-tester .                                # Docker image (FROM scratch + wasmtime)
```

**CLI usage (wasmtime):**
```bash
# Commands: validate | validate-strict | run-tests | parse | summary | stage-names | unknown-steps | dump-registry
# Exit 0 = valid/pass, 1 = invalid/fail, 2 = usage error

wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm validate        < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm validate-strict < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm run-tests       < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm parse           < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm validate          path/to/Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm dump-registry

# Custom plugin registry (--registry flag):
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm dump-registry > my-plugins.json
# Edit my-plugins.json to match your Jenkins instance, then:
wasmtime run --dir=. target/wasm32-wasi/release/jenkinsfile-tester.wasm \
  --registry my-plugins.json validate < Jenkinsfile
```

**Docker usage:**
```bash
docker run --rm -i jenkinsfile-tester validate < Jenkinsfile
docker run --rm -i jenkinsfile-tester validate-strict < Jenkinsfile
docker run --rm -i jenkinsfile-tester run-tests < Jenkinsfile
docker run --rm    jenkinsfile-tester dump-registry

# Custom plugin registry (mount file):
docker run --rm -i -v ./my-plugins.json:/registry.json \
  jenkinsfile-tester --dir=/ --registry /registry.json validate < Jenkinsfile
```

**GitHub Pages:**
```bash
./build-pages.sh   # runs wasm-pack build --target web, copies output to docs/
# Commit docs/ and push; configure GitHub Pages to serve from docs/ on main branch
```

## Environment constraints

- **Rust toolchain:** `rustc 1.73.0` (system Rust on WSL — do not upgrade without testing)
- **`pest` is pinned to `=2.7.15`** in `Cargo.toml` — newer versions require rustc ≥ 1.83
- **WSL + `/mnt/c/` filesystem:** Use `./test.sh` — it deletes stale `target/debug/deps/integration-*`
  before every run. Do NOT use bare `cargo test` after edits on this filesystem.
- **NO `std::sync::LazyLock`** — requires 1.80+. Use `std::sync::OnceLock` instead.
- **NO new Cargo.toml dependencies** without strong justification.

## Architecture

All public API functions return `String` (JSON) and **never panic**.
Parse failures are returned as structured JSON error objects, not Rust panics.

| Module | Purpose |
|--------|---------|
| `src/parser/jenkinsfile.pest` | PEG grammar for declarative Jenkinsfile DSL |
| `src/parser/mod.rs` | pest → AST builder; `parse()` returns `Result<Pipeline, ParseError { message, line, col }>` |
| `src/ast/mod.rs` | All AST types |
| `src/ast/walk.rs` | AST traversal helpers: `collect_all_stages`, `collect_all_steps_recursive`, `walk_steps_with_stage`, `stage_steps/parallel/sequential` |
| `src/plugins/mod.rs` | `PluginRegistry`: `from_json`, `merge`, `has_tool/step/option/trigger/agent_type`, `all_tools/steps`, `builtin_arc()` (OnceLock), 18 bundled plugins |
| `src/validator/` | 22 diagnostic rules (E001–E006, W001–W011, S001–S005); `ValidationContext` holds `Arc<PluginRegistry>` + `ValidationMode` |
| `src/tester/mod.rs` | 22 structural assertions |
| `src/lib.rs` | WASM entry + public API (10 functions); `THREAD_REGISTRY` thread-local for persistent registry |
| `tests/integration.rs` | 234 integration tests, all passing |
| `TESTS.md` | Test documentation with pipeline snippets and explanations |
| `test.sh` | Convenience test runner — clears stale deps cache before `cargo test` |
| `src/main.rs` | CLI binary for wasmtime (wasm32-wasi); `--registry` flag, `dump-registry` command |
| `build-pages.sh` | Builds WASM via wasm-pack, assembles `docs/` for GitHub Pages |
| `.github/workflows/docker-publish.yml` | GitHub Actions: build + push Docker image to ghcr.io on push to main or version tags |
| `Dockerfile` | Multi-stage: Alpine builder (rustup + wasmtime) → FROM scratch runtime |
| `docs/` | GitHub Pages output: `index.html` + `pkg/` (generated by `build-pages.sh`) |

### Public WASM API

| Function | Returns |
|----------|---------|
| `parse_jenkinsfile(src)` | `{ success, ast? }` or `{ success: false, error: { message, line, col } }` |
| `validate(src)` | `{ is_valid, errors, warnings, suggestions }` — uses `THREAD_REGISTRY` if set, else builtin |
| `validate_with_registry(src, registry_json)` | Same as validate but merges a user-supplied registry (per-call, does not update `THREAD_REGISTRY`) |
| `init_registry(registry_json)` | `{ ok: true }` or `{ ok: false, error }` — merges user JSON into builtin and stores in `THREAD_REGISTRY` |
| `run_tests(src)` | `{ name, tests, passed, failed, skipped }` |
| `get_ast_json(src)` | JSON string of full AST, or `"null"` |
| `get_stage_names(src)` | JSON string array of all stage names (recursive), or `"[]"` |
| `get_pipeline_summary(src)` | `{ success, stage_count, has_post, agent_type, parameter_count, has_triggers, has_environment }` |
| `validate_strict(src)` | Same as `validate()` but W007 unknown-step diagnostics are errors (pipeline invalid) |
| `get_unknown_keywords(src)` | JSON string array of unregistered step names found in the pipeline, sorted + deduplicated |
| `get_validation_rules()` | JSON array of all rule metadata: `{ code, severity, description }` for all 22 rules |

## Current test status (as of Sprint 9 — LOC/E006/API-001/UI)

- **183 unit tests: ALL PASSING**
- **234 integration tests: ALL PASSING**
- **Total: 417 tests, 0 failures**

## Known grammar limitations

1. **`script {}` bodies** — captured as opaque raw text with brace-balanced matching; nested `{}` blocks (closures, if/else, try/catch) are handled correctly, but contents are Groovy and not re-parsed.
2. **Escaped quotes in strings** — `'it\'s'` and `"say \"hi\""` are not supported.
3. **`interpolated_string` rule is dead code** — rule definition retained but not reachable from any alternative.
4. **`expression { ... }` when condition** — `expression_body` uses heuristic lookahead; closures containing `}` may not parse correctly.
5. **`list_literal` is only wired into `param_arg`** — `option_arg`, `trigger_arg`, `call_arg`, and `named_arg` do not accept lists (intentional; those contexts don't need them).
6. **`env_value` accepts arbitrary Groovy expressions** — `env_expression` captures everything up to the newline, handling quoted strings and balanced parentheses (e.g. `"${BAR}".replace("baz", "quuz")`). The raw text is stored as `EnvValue::Literal`.

## AST coverage

### Implemented

- `Pipeline`: agent, environment, options, triggers, parameters, stages, post, libraries, duplicate_sections
- `Stage`: name, agent, environment, when (`before_agent/input/options`), options, tools, input, body, post, fail_fast, duplicate_sections
- `StageBody`: Steps | Parallel { stages } | Sequential { stages } | Matrix { axes, excludes, stages }
- `Agent`: Any | None | Label(String) | Docker { image, args, custom_workspace, reuse_node, registry_url, registry_credentials_id, always_pull } | Dockerfile { filename, dir, additional_build_args, label } | **Node { label, custom_workspace }** | **Kubernetes { yaml, yaml_file, cloud, namespace, default_container, inherit_from, retries, label }** | **Generic { agent_type }** (catch-all for unknown plugin agents)
- `PipelineOption`: Timeout, BuildDiscarder (typed logRotator fields), Retry, DisableConcurrentBuilds, SkipDefaultCheckout, SkipStagesAfterUnstable, PreserveStashes, Timestamps, ParallelsAlwaysFailFast, NewContainerPerStage, QuietPeriod, CheckoutToSubdirectory, DisableResume, AnsiColor, **Raw { name, text }**
- `Trigger`: Cron { spec } | PollScm { spec } | Upstream { projects, threshold } | GithubPush | GitlabPush | GenericTrigger | **Raw { name, text }**
- `Parameter`: String | Boolean | Choice | Text | Password | File | Run { filter }
- `WhenCondition`: Branch | Tag | Environment | Expression | Not | AllOf | AnyOf | ChangeRequest | BuildingTag | Changelog | Changeset | TriggeredBy | Equals | Generic
- `PostCondition`: Always | Success | Failure | Unstable | Aborted | Changed | Fixed | Regression | Cleanup | Unsuccessful
- `Binding` (withCredentials): UsernamePassword | StringBinding | FileBinding | SshUserPrivateKey | Certificate | Raw
- `SharedLibrary`: name, ref_ (split from `lib('name@ref')`)
- `StageInput`: message, ok, submitter, submitter_parameter, parameters

### Still missing (future work)

- Span-aware `location` for rules E001, W001–W004, W006–W010, S001–S005 (still emit `location: None` — these reference pipeline-level constructs with no obvious single source node)
- `docker` agent sub-fields beyond what's implemented: `customWorkspace`/`reuseNode`/`registryUrl`/`registryCredentialsId`/`alwaysPull` are **done**; nothing remaining
- `kubernetes` agent `yaml` field accepts triple-quoted strings but does not re-parse the YAML body

## Validator rules

| Rule | Description |
|------|-------------|
| E001 | No agent defined at pipeline level (`check_no_pipeline_agent`; sentinel location line=1, col=1) |
| E002 | Required steps block missing in a stage — recurses into parallel branches |
| E003 | Duplicate stage names at same level |
| E004 | Duplicate parameter names |
| E005 | Duplicate section declarations (e.g. two `agent` blocks in same scope) |
| E006 | Pipeline has no stages defined (`check_e006_empty_stages`; sentinel location line=1, col=1) |
| W001 | Missing timeout option |
| W002 | Plaintext credential in env variable name (word-boundary split on `_`, not substring) |
| W003 | Shell steps without `set -e` — carries `Step::Sh` location |
| W004 | Missing post section (sentinel location line=1, col=1) |
| W005 | Hardcoded secret in script — scans variable names AND values (ghp_, AKIA, JWT, 32-char hex) — carries `Step::Sh` location |
| W006 | Tool type not in plugin registry |
| W007 | Unknown step name (`Step::Generic`) not in registry — Warning (Permissive) or Error (Strict) or silent (Discovery) |
| W008 | `Agent::Generic` type not in registry `agent_types` |
| W009 | `PipelineOption::Raw` name not in registry `options` |
| W010 | `Trigger::Raw` name not in registry `triggers` |
| W011 | Credential variable referenced in double-quoted `sh` or `echo` string — Groovy interpolation exposes the secret value and bypasses credential masking |
| S001 | No parameters defined |
| S002 | No timeout option defined |
| S003 | Long pipeline with no stages |
| S004 | Deep nesting detected |
| S005 | `allOf`/`anyOf` with fewer than 2 child conditions |

## PluginRegistry notes

- `builtin_arc()` uses `std::sync::OnceLock` — parsed exactly once, shared via `Arc`
- `validate_with_registry(src, json)` merges user JSON into builtin per call
- `init_registry(json)` stores merged registry in `thread_local! THREAD_REGISTRY`; `validate()` checks it first
- `merge()` does **not** deduplicate by `plugin_id`; `all_tools()` can return duplicates (documented — TGAP-005/018)
- Methods: `has_step`, `has_tool`, `has_option`, `has_trigger`, `has_agent_type`, `all_steps`, `all_tools`

## Agent review cycle history

| Date | Agent | Summary |
|------|-------|---------|
| 2026-02-27 | architect | Full architecture review — 17 findings |
| 2026-02-27 | testing | Added 4 gap tests; documented VGAP-001/004 as sentinel tests |
| 2026-02-27 | developer | Fixed REC-002, REC-004, REC-010 |
| 2026-02-27 | reviewer | Approved. Produced Sprint 1 task list. |
| 2026-02-28 | developer | Sprint 1: correctness fixes (E004, W006, VGAP-001/003/004) |
| 2026-02-28 | developer | Sprint 2 items 8–10: ast::walk, PipelineOption enum, grammar ordering |
| 2026-02-28 | developer | Sprint 3 PLUGIN-001–004: PluginRegistry, builtin JSON, W006 registry |
| 2026-02-28 | architect | Sprint 2+3 architecture review — 19 findings (ARC-001–ARC-019) |
| 2026-02-28 | testing | 19 test gaps identified (TGAP-004–TGAP-022) |
| 2026-02-28 | reviewer | Sprint 2+3 review — 7 integration test failures. Changes requested. |
| 2026-02-28 | developer | Sprint 4a: 7 failures fixed, OnceLock registry, Stage.options/tools, walk helpers, ParseError, get_pipeline_summary, validate_with_registry. 195→217 tests. |
| 2026-02-28 | testing | Test consolidation: closed TGAP-005/008/009/010/011/012/018/022. 217→236 tests. |
| 2026-02-28 | developer | Sprint 4b: Typed Binding, init_registry WASM, input stage directive, GL-002 nested grammar. 236→247 tests. |
| 2026-03-01 | developer | Sprint 4c: full matrix directive, failFast, Typed Trigger enum. 247→255 tests. |
| 2026-03-01 | developer | Option A: E005, W002 word-boundary fix, S005, TGAP-001/003. 255→271 tests. |
| 2026-03-01 | developer | Option B: when conditions/modifiers, post unsuccessful, file/run params, libraries. 271→285 tests. |
| 2026-03-01 | developer | ARC-003-TASK: PluginContributions.steps → Vec<StepEntry { name, args }>; backwards-compat string deserialization; all_steps(). 285→289 tests. |
| 2026-03-01 | developer | Option C PLUGIN-011/012: ValidationMode enum (Strict/Permissive/Discovery), validate_strict() WASM, get_unknown_keywords() WASM. 289→300 tests. |
| 2026-03-01 | developer | PLUGIN-007/008/009: Agent::Generic catch-all + W008, PipelineOption::Raw name field + W009, Trigger::Raw name field + W010. has_agent_type(). 300→314 tests. |
| 2026-03-01 | developer | choice param list literal fix: `list_literal` grammar rule, `param_arg` extended, 4 new integration tests. 314→318 tests. |
| 2026-03-01 | architect | Frontend plugin declaration plan: PLUGIN-013/014/015. Full plan in `.local/handoff/PLAN-frontend-plugin-declaration.md`. |
| 2026-03-01 | developer | PLUGIN-013/014/015: get_builtin_registry(), validate_with_exact_registry(), plugin drawer UI. 318→324 tests. |
| 2026-03-01 | developer | Sprint 5 D-001–D-005: Docker/Dockerfile full fields, Agent::Node, TGAP-016, TGAP-021 comment. 324→333 tests. |
| 2026-03-01 | developer | Sprint 6 E-001–E-005: EnvValue typed enum (Literal/Credentials), W002/tester guard, collect_env_vars_for_stage walk helper, PipelineOption camelCase serde renames. 333→340 tests. |
| 2026-03-01 | developer | Sprint 7 K-001–K-005: Agent::Kubernetes typed variant with 8 sub-fields (yaml, yamlFile, cloud, namespace, defaultContainer, inheritFrom, retries, label); grammar rule agent_kubernetes_entry before agent_generic_entry. 340→349 tests. |
| 2026-03-01 | developer | Sprint 8 S-001–S-006: Location type added to AST; location field on Stage, Parameter variants, Step::Sh; extract_location() helper; E002/E003/E004/W005 rules now populate location; 6 new integration tests. 349→355 tests. |
| 2026-03-01 | developer | Sprint 9 LOC-001/002/E006/API-001/UI-001/UI-002: E001 now = no agent (sentinel loc), E006 = no stages; S004 sentinel loc; get_validation_rules() WASM; localStorage plugin persistence; custom plugin add form. 355→369 tests. |
| 2026-03-02 | developer | Tester expanded: 11 new structural assertions based on CloudBees/Jenkins best practices research. run_tests() now runs 21 tests. 369→407 tests (177 unit + 230 integration). README.md and ARCHITECTURE.md written as user-facing documentation. |
| 2026-03-03 | developer | W011 extended: `Step::Echo` gains `is_double_quoted: bool` (same pattern as `Step::Sh`); W011 rule and `no_groovy_interpolated_credentials` tester assertion now cover both `sh` and `echo`. `choice` param list-literal fix confirmed complete. 407→417 tests (183 unit + 234 integration). |

Handoff files: `.local/handoff/`

## Tests

- **Unit tests** live inline in `src/validator/rules.rs` and `src/tester/mod.rs`.
  Build AST objects directly — no parser overhead — test each rule in isolation.
- **Integration tests** in `tests/integration.rs` test the full parse → validate/tester chain
  through the public JSON API.
- See `TESTS.md` for annotated pipeline snippets and explanations.

## StageBody enum JSON shape (reference)

```json
{ "body": { "type": "steps",      "steps": [...] } }
{ "body": { "type": "parallel",   "stages": [...] } }
{ "body": { "type": "sequential", "stages": [...] } }
{ "body": { "type": "matrix",     "axes": [...], "excludes": [...], "stages": [...] } }
```

Stage fields: `name`, `agent`, `environment`, `when`, `options`, `tools`, `input`, `body`, `post`, `fail_fast` (omitted when false), `duplicate_sections` (omitted when empty).

`StageBody::Parallel` and `::Sequential` use named struct variants `{ stages: Vec<Stage> }` —
NOT tuple variants — to serialize correctly with serde's internally-tagged enum.

## Key implementation decisions (for future agents)

- **`std::sync::OnceLock`** — use for lazy statics on Rust 1.73. `LazyLock` requires 1.80+.
- **`parse()` return type** — `Result<Pipeline, ParseError>` where `ParseError { message, line: Option<u32>, col: Option<u32> }`.
- **`ValidationContext`** — holds `Arc<PluginRegistry>`. Use `ValidationContext::with_registry(arc)` in tests.
- **`call_expr` grammar** — `option_arg` tries `call_expr` before `value`; enables `buildDiscarder(logRotator(...))`.
- **`BuildDiscarder`** — typed fields `num_to_keep`, `days_to_keep`, `artifact_num_to_keep`, `artifact_days_to_keep` + `raw: Option<String>` fallback.
- **`Trigger` enum** — named args extracted via `trigger_arg = { (identifier ~ ":")? ~ value }` grammar rule.
- **`Stage.input: Option<StageInput>`** — `input_submitter_param` placed before `input_submitter` in alternatives (PEG prefix matching).
- **`Stage.fail_fast: bool`** — `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`.
- **`When` modifiers** — `before_agent/input/options` parsed via `when_field*` which dispatches modifier rules before `when_condition`.
- **`Step::WithCredentials { bindings: Vec<Binding> }`** — typed Binding enum with Raw fallback.
- **`THREAD_REGISTRY`** — `thread_local! { static THREAD_REGISTRY: RefCell<Option<Arc<PluginRegistry>>> }` in `src/lib.rs`.
- **`duplicate_sections`** — populated by parser using a `HashSet<&str>` in `build_pipeline()`/`build_stage()`; read by E005.
- **W002** — uses `split('_')` + exact component matching against keyword list, not `.contains()`.
- **`merge()` deduplication** — does NOT deduplicate by `plugin_id`; this is documented behavior.
- **`ValidationMode`** — `Permissive` (default) / `Strict` / `Discovery` enum on `ValidationContext`. Use `.with_mode(mode)` builder. `Strict` → W007 is Error; `Discovery` → W007 is silent.
- **`get_unknown_keywords`** — parses pipeline, walks all steps, collects `Step::Generic` names not in registry, returns sorted+deduped JSON array. Returns `"[]"` on parse error.
- **`validate_strict`** — same as `validate()` but sets `ValidationMode::Strict` on the context.
- **`Agent::Node { label, custom_workspace }`** — `agent { node { label '…'; customWorkspace '…' } }` form. Grammar rule: `agent_node_entry` with `node_field*` alternatives (`node_label`, `node_customWorkspace`). Added before `agent_generic_entry` in `agent_block_entry` to prevent the generic catch-all from swallowing it.
- **`Agent::Docker` full fields** — `custom_workspace`, `reuse_node`, `registry_url`, `registry_credentials_id`, `always_pull` added. Grammar rule `docker_customWorkspace` added; other rules (`docker_registryUrl`, `docker_registryCredentialsId`, `docker_reuseNode`, `docker_alwaysPull`) already existed. All fields `#[serde(skip_serializing_if = "Option::is_none")]`.
- **`Agent::Dockerfile` full fields** — `additional_build_args`, `label` added. Grammar rules `dockerfile_additionalBuildArgs`, `dockerfile_label` already existed.
- **`EnvValue` enum** — `EnvVar.value` is now `EnvValue` (not `String`). `EnvValue::Literal(String)` serialises as a plain string (backwards-compatible). `EnvValue::Credentials { id }` serialises as `{ "type": "credentials", "id": "..." }`. Custom `Serialize`/`Deserialize` impls handle the mixed-type JSON. Parser extracts the inner `quoted_string` from `credentials_call` to populate `id`.
- **`PipelineOption` camelCase renames** — `#[serde(rename = "...")]` added to `BuildDiscarder`, `DisableConcurrentBuilds`, `SkipDefaultCheckout`, `SkipStagesAfterUnstable`, `PreserveStashes`, `ParallelsAlwaysFailFast`, `NewContainerPerStage`, `CheckoutToSubdirectory`, `DisableResume`, `AnsiColor`, `QuietPeriod`. Existing integration tests updated to expect camelCase JSON type names.
- **`collect_env_vars_for_stage`** — new public function in `src/ast/walk.rs`; returns `HashMap<&str, &EnvValue>` merging pipeline-level env (lower priority) with stage-level env (wins on conflict).
- **`Agent::Kubernetes { yaml, yaml_file, cloud, namespace, default_container, inherit_from, retries, label }`** — typed kubernetes agent variant. Grammar rule `agent_kubernetes_entry` placed **before** `agent_generic_entry` in `agent_block_entry` so PEG ordered choice picks it first. `kubernetes_yaml` accepts `triple_string | quoted_string` for multi-line pod specs. All fields `Option`; serialises with `#[serde(rename = "kubernetes")]` tag. W008 never fires for kubernetes (it is no longer `Agent::Generic`).
- **`Agent::Generic { agent_type }`** — catch-all for unknown block agent types in grammar (e.g. `agent { myCustomAgent { ... } }`). `agent_generic_entry` is the grammar rule; body captured opaquely.
- **`PipelineOption::Raw { name, text }`** — added `name` field (option identifier) for W009 rule.
- **`Trigger::Raw { name, text }`** — added `name` field (trigger identifier) for W010 rule.
- **`Location { line: u32, col: u32 }`** — 1-based line/col source location. Added to `Stage`, `Parameter` (all variants), and `Step::Sh`. Populated by `extract_location(pair)` in the parser using `pair.as_span().start_pos().line_col()`. All fields use `#[serde(skip_serializing_if = "Option::is_none")]` so consumers see no JSON change when location is absent. Rules E002, E003, E004, and W005 now populate `Diagnostic.location` from the AST node. Rules E001, W001-W004, W006-W010, S001-S005 still emit `location: None` (omitted from JSON).
- **`PluginRegistry::has_agent_type()`** — checks `PluginContributions.agent_types` list.
- **W008** — `Agent::Generic` with type not in `registry.agent_types` → Warning.
- **W009** — `PipelineOption::Raw` with name not in `registry.options` → Warning.
- **W010** — `Trigger::Raw` with name not in `registry.triggers` → Warning.
- **W011** — `Step::Sh { is_double_quoted: true }` or `Step::Echo { is_double_quoted: true }` inside `withCredentials`, referencing a bound credential variable name via `${VAR}` or `$VAR` → Warning. `is_double_quoted` captured from raw pest `Pair` before `unquote()` strips delimiters (`starts_with('"')`). Covers all `Binding` variants except `Raw`. `echo` extended to same treatment as `sh` — both can expose secrets via Groovy GString interpolation before Jenkins masking occurs.
- **`PluginContributions.steps`** — `Vec<StepEntry { name, args: Vec<StepArg> }>`. Deserializer accepts both old string format (`"sh"`) and new object format (`{ "name": "sh" }`) via `StepEntryInput` untagged enum. `has_step(name)` checks `step.name`. `all_steps()` returns flat `Vec<&StepEntry>` across all plugins.
- **`list_literal` grammar rule** — `"[" ~ (value ~ ("," ~ value)* ~ ","?)? ~ "]"`. Only wired into `param_arg` (before `value` in PEG ordered choice). Enables `choice(choices: ['a','b','c'])`. Inner nodes of `list_literal` are `value` nodes; `unquote(item.as_str())` extracts string content.
- **`param_arg` with list** — when `val_node.as_rule() == Rule::list_literal`, iterate `list.into_inner()` and push each `unquote(item.as_str())` into `choices`. Scalar path unchanged.
- **`get_builtin_registry()`** — PLUGIN-013; returns `PluginRegistry::builtin_arc()` as JSON `{ "plugins": [...] }`. `PluginRegistry` needed `#[derive(Serialize)]` added (inner types already had it).
- **`validate_with_exact_registry(src, json)`** — PLUGIN-014; like `validate_with_registry` but does NOT merge into builtins — uses only the caller's registry. Needed for frontend plugin selector.
- **`demo/index.html` plugin drawer** — PLUGIN-015; collapsible section between textarea and toolbar. Populated from `get_builtin_registry()`. Checkboxes per plugin; "Validate (selected plugins)" calls `validate_with_exact_registry` with only checked entries. "Reset" re-enables all. Badge shows `(N / 18 active)` when subset selected.
- **`check_no_pipeline_agent`** — Sprint 9 LOC-002; fires E001 with sentinel `location: Some(Location { line: 1, col: 1 })` when `pipeline.agent.is_none()`. Old `check_has_stages` (E001 for no stages) renamed to E006.
- **`check_e006_empty_stages`** — Sprint 9 E006; fires E006 with sentinel location when `pipeline.stages.is_empty()`.
- **`S004` location** — Sprint 9 LOC-002; `check_post_exists` now emits `location: Some(Location { line: 1, col: 1 })`.
- **`get_validation_rules()`** — Sprint 9 API-001; WASM function returning all rule codes with severity and description as a JSON array (currently 22 rules).
- **`demo/index.html` localStorage** — Sprint 9 UI-002; `savePluginState()` persists to `localStorage` key `jenkinsfile-tester:plugins`. Restored on `initPluginDrawer()`.
- **`demo/index.html` custom plugin form** — Sprint 9 UI-001; `#plugin-add-row` inputs and `addCustomPlugin()`. Custom entries render with `[custom]` badge.
- **CLI `--registry` flag** — `parse_args` in `src/main.rs` extracts `--registry <path>` before positional args. Registry loaded via `PluginRegistry::from_json()` and threaded as `Arc<PluginRegistry>` into `run_command`/`do_validate`. Exit 2 on read or parse failure.
- **`dump-registry` command** — CLI command that prints `serde_json::to_string_pretty(&*builtin)` to stdout. Round-trips with `from_json()`.
- **`env_expression` grammar rule** — atomic rule that captures arbitrary Groovy expressions in environment values. Handles quoted strings (with braces/parens inside) and balanced parentheses for method calls. Stops at newline or unbalanced `}`. Replaces the old `quoted_string | bare_word` alternative in `env_value`.
- **`script_body` brace-balanced** — grammar changed from `(!"}" ~ ANY)*` to recursive `script_body_token*` where `script_body_token = { "{" ~ script_body_token* ~ "}" | (!"}" ~ !"{" ~ ANY) }`. Correctly handles nested braces in Groovy closures, if/else, try/catch.
- **`when_changeRequest` / `when_buildingTag`** — grammar updated to accept optional trailing `()` parentheses (Jenkins allows both forms).
- **`when_generic` grammar rule** — catch-all for plugin-contributed when conditions. Matches `identifier ~ (("(" ~ ")") | quoted_string | "{" ~ when_condition+ ~ "}")?`. Parser extracts the identifier as `name` and any quoted string as `args`. Placed last in `when_condition` alternatives.
- **W002 `contains_whole_word`** — replaced `script.contains(var)` with word-boundary matching to avoid false positives where a credential var name is a substring of a longer identifier (e.g. `DEPLOY_KEY` inside `DEPLOY_KEY_USR`).
- **Dockerfile** — multi-stage: Alpine 3.21 builder installs rustup + wasmtime, builds WASM binary, collects wasmtime + shared libraries via `ldd`. Final `FROM scratch` stage contains only wasmtime, its `.so` deps, and the `.wasm` file. `HOME=/tmp` prevents wasmtime config file errors. Pipeline piped via stdin, command via CMD args.
- **`build-pages.sh`** — runs `wasm-pack build --target web`, copies `jenkinsfile_tester.js` and `.wasm` to `docs/pkg/`, rewrites import path in `demo/index.html` from `../pkg/` to `./pkg/` and writes to `docs/index.html`. GitHub Pages serves from `docs/` folder.
- **Project renamed** from `jenkins-tester` to `jenkinsfile-tester` — Cargo.toml package/bin name, crate imports in tests, demo HTML title/imports/localStorage key, all documentation.
- **GitHub Actions Docker publish** — `.github/workflows/docker-publish.yml` builds and pushes the Docker image to `ghcr.io/wassy121/jenkinsfile-tester` on push to `main`, version tags (`v*`), and manual dispatch. Uses `docker/metadata-action` for tag/label generation and `type=gha` layer caching. No secrets needed — uses built-in `GITHUB_TOKEN`.
