# Architecture

This document describes the internal design of jenkinsfile-tester: how the code is structured, why key decisions were made, and what constraints govern future changes.

---

## Compilation target

The library compiles to **WebAssembly** via `wasm-bindgen`. Every public function is annotated `#[wasm_bindgen]` and has this signature shape:

```rust
#[wasm_bindgen]
pub fn validate(src: &str) -> String { ... }
```

All inputs are `&str` or `String`. All outputs are `String` (JSON). The functions **never panic** — parse failures, registry errors, and serialisation errors are all returned as structured JSON.

This contract means the library can be called from any JavaScript environment with zero special handling on the caller side.

---

## Module structure

```
src/
├── parser/
│   ├── jenkinsfile.pest   # Grammar
│   └── mod.rs             # Grammar → AST
├── ast/
│   ├── mod.rs             # Type definitions
│   └── walk.rs            # Traversal helpers
├── plugins/
│   └── mod.rs             # Plugin registry
├── validator/
│   ├── mod.rs             # Orchestration
│   ├── context.rs         # ValidationContext
│   └── rules.rs           # Individual rules
├── tester/
│   └── mod.rs             # Structural assertions
└── lib.rs                 # WASM API surface
```

Data flows in one direction: **source text → parser → AST → validator/tester → JSON output**. No module below `lib.rs` depends on any module above it.

---

## Parser

### Grammar

The grammar is written in [pest](https://pest.rs/) PEG format, pinned to version `=2.7.15` in `Cargo.toml`. Newer pest versions require Rust ≥ 1.83; the system Rust is 1.73.0.

`src/parser/jenkinsfile.pest` defines the complete Declarative Pipeline grammar. A few design points:

**Ordered choice is load-bearing.** PEG grammars try alternatives left to right and stop at the first match. This means specific rules must be listed before catch-alls. For example, `agent_kubernetes_entry` and `agent_node_entry` must appear before `agent_generic_entry` in the `agent_block_entry` alternatives, otherwise the generic catch-all swallows them. The same applies to `input_submitter_param` before `input_submitter`.

**`script {}` bodies are opaque.** The content of `script {}` is captured as raw text. Re-parsing arbitrary Groovy is out of scope.

**`list_literal` is scoped.** The grammar rule `list_literal = { "[" ~ (value ~ ("," ~ value)* ~ ","?)? ~ "]" }` is only wired into `param_arg`. Options, triggers, and call arguments do not accept lists — those contexts don't need them.

**`expression {}` uses heuristic lookahead.** `expression_body` uses a lookahead to find the closing `}`. Closures containing `}` characters may not parse correctly. Full Groovy expression parsing is out of scope.

### Builder (`src/parser/mod.rs`)

`parse(src: &str) -> Result<Pipeline, ParseError>` is the only public function. It calls `pest::Parser::parse()` and then walks the resulting parse tree via recursive builder functions (`build_pipeline`, `build_stage`, `build_step`, `build_agent`, etc.).

Each builder function takes a `Pair<Rule>` (a pest parse node) and returns the corresponding AST type.

**`unquote(s)`** is a shared helper that strips surrounding single quotes, double quotes, or triple single-quotes from string values. It is used throughout the builder.

**`extract_location(pair)`** reads the span start position from a pest `Pair` and returns `Some(Location { line, col })` using `pair.as_span().start_pos().line_col()`. Coordinates are 1-based and cast to `u32`. This function is called at the point where a pair is consumed — it must be called **before** `into_inner()` because consuming the pair destroys its span.

**`ParseError`** carries `message: String`, `line: Option<u32>`, `col: Option<u32>`. Line and col are `None` for errors that don't have a meaningful source position.

---

## AST (`src/ast/mod.rs`)

The AST types are plain Rust structs and enums, all deriving `Serialize`/`Deserialize`. The JSON shape is the primary external API contract — changes to field names or enum variant names are breaking changes.

### Key types

**`Pipeline`** is the root. Fields: `agent`, `environment`, `options`, `parameters`, `triggers`, `tools`, `stages`, `post`, `libraries`, `duplicate_sections`.

**`Stage`** carries a `body: StageBody` discriminated union instead of separate optional fields for steps/parallel/sequential/matrix. This keeps stage matching exhaustive.

**`StageBody`** is an internally-tagged enum:
```json
{ "body": { "type": "steps",      "steps": [...] } }
{ "body": { "type": "parallel",   "stages": [...] } }
{ "body": { "type": "sequential", "stages": [...] } }
{ "body": { "type": "matrix",     "axes": [...], "excludes": [...], "stages": [...] } }
```
`Parallel` and `Sequential` use named struct variants (`{ stages: Vec<Stage> }`) rather than tuple variants — a requirement of serde's internally-tagged enum representation.

**`Agent`** is an enum with fully typed variants for every known agent form, plus `Generic { agent_type: String }` as a catch-all for plugin-registered agent types:

| Variant | Groovy form |
|---|---|
| `Any` | `agent any` |
| `None` | `agent none` |
| `Label(String)` | `agent { label 'linux' }` |
| `Docker { image, args, custom_workspace, reuse_node, registry_url, registry_credentials_id, always_pull }` | `agent { docker { ... } }` |
| `Dockerfile { filename, dir, additional_build_args, label }` | `agent { dockerfile { ... } }` |
| `Node { label, custom_workspace }` | `agent { node { ... } }` |
| `Kubernetes { yaml, yaml_file, cloud, namespace, default_container, inherit_from, retries, label }` | `agent { kubernetes { ... } }` |
| `Generic { agent_type }` | `agent { anyOtherType { ... } }` |

**`EnvValue`** is a typed enum replacing the previous raw `String` for environment variable values:

```rust
pub enum EnvValue {
    Literal(String),
    Credentials { id: String },
}
```

`Literal` serialises as a plain JSON string (backwards-compatible). `Credentials` serialises as `{ "type": "credentials", "id": "..." }` via a custom `Serialize` implementation. This allows W002 and W005 to suppress false positives on `credentials()` usages, since those are correct credential bindings, not plaintext values.

**`Location`** provides source positions for diagnostics:

```rust
pub struct Location {
    pub line: u32,
    pub col:  u32,
}
```

`Location` is present on `Stage`, all `Parameter` variants, and `Step::Sh`. Fields use `#[serde(skip_serializing_if = "Option::is_none")]` so JSON consumers see no change when location is absent. Pipeline-level rules that have no specific source node (E001, E004, E006) emit a sentinel `Location { line: 1, col: 1 }`.

**`PipelineOption`** has 14 typed variants plus `Raw { name, text }` for plugin-contributed options. The typed variants use `#[serde(rename = "...")]` to match Jenkins canonical camelCase names (e.g. `BuildDiscarder` serialises as `"buildDiscarder"`).

**`Trigger`** has typed variants for Cron, PollScm, Upstream, GithubPush, GitlabPush, GenericTrigger, and `Raw { name, text }` for plugin triggers.

**`Binding`** (used in `withCredentials`) has typed variants for UsernamePassword, StringBinding, FileBinding, SshUserPrivateKey, Certificate, and `Raw`.

**`duplicate_sections`** on `Pipeline` and `Stage` is a `Vec<String>` populated by the parser using a `HashSet<&str>` during `build_pipeline` and `build_stage`. It lists the names of sections that appeared more than once (e.g. `["agent", "environment"]`). Rule E005 reads this field.

### Walk helpers (`src/ast/walk.rs`)

| Function | Returns |
|---|---|
| `collect_all_stages(stages)` | All stages at all nesting levels (depth-first) |
| `collect_all_steps_recursive(stage)` | All steps in a stage, recursing into parallel/sequential/matrix |
| `walk_steps_with_stage(stages)` | `Vec<(&Stage, &Step)>` for all steps with their containing stage |
| `stage_steps(stage)` | Steps directly in a steps stage body |
| `stage_parallel(stage)` | Parallel branches |
| `stage_sequential(stage)` | Sequential nested stages |
| `collect_env_vars_for_stage(stage, pipeline)` | `HashMap<&str, &EnvValue>` — effective env for a stage, with stage-level vars winning over pipeline-level on conflict |

---

## Plugin registry (`src/plugins/mod.rs`)

`PluginRegistry` holds a `Vec<PluginEntry>`. Each `PluginEntry` has a `plugin_id`, a `version`, and a `PluginContributions` struct listing steps, agent types, options, triggers, and tools.

### Builtin registry

18 plugins are embedded as a JSON literal compiled into the binary. The registry is parsed once at first use via `std::sync::OnceLock` and then shared as `Arc<PluginRegistry>`:

```rust
pub fn builtin_arc() -> Arc<PluginRegistry> {
    static REGISTRY: OnceLock<Arc<PluginRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Arc::new(from_json(BUILTIN_JSON).unwrap())).clone()
}
```

`OnceLock` is used instead of `LazyLock` because `LazyLock` requires Rust 1.80+.

### Thread-local state

`lib.rs` holds a thread-local:

```rust
thread_local! {
    static THREAD_REGISTRY: RefCell<Option<Arc<PluginRegistry>>> = RefCell::new(None);
}
```

`init_registry(json)` parses the user-supplied JSON, merges it into the built-in registry, and stores the result in `THREAD_REGISTRY`. Subsequent calls to `validate()` check `THREAD_REGISTRY` first and fall back to the built-in if it is unset.

### Merge behaviour

`merge(other: PluginRegistry)` appends the other registry's plugins to `self.plugins`. It does **not** deduplicate by `plugin_id`. Calling `merge` twice with the same registry produces duplicate entries. `has_step`, `has_tool`, and similar methods scan the entire list, so duplicates have no effect on correctness — only on performance for very large registries.

### Steps: `Vec<StepEntry { name, args }>`

`PluginContributions.steps` holds `Vec<StepEntry>`, not `Vec<String>`. The deserialiser accepts both the old plain-string format (`"sh"`) and the new object format (`{ "name": "sh" }`) via an untagged intermediate enum. This preserves backwards compatibility with registry JSON files written before the `StepEntry` type was introduced.

---

## Validator (`src/validator/`)

### ValidationContext

Every validation call creates a `ValidationContext` that carries:

- `&Pipeline` — the AST to validate
- `&str` — the original source (for diagnostic messages)
- `Arc<PluginRegistry>` — the registry to check against
- `ValidationMode` — `Permissive` (default), `Strict`, or `Discovery`

```rust
// Permissive (default): W007 is a warning
let ctx = ValidationContext::new(&ast, src);

// Strict: W007 becomes an error
let ctx = ValidationContext::new(&ast, src).with_mode(ValidationMode::Strict);

// With a custom registry
let ctx = ValidationContext::with_registry(&ast, src, arc);
```

### Rules

`rules.rs` contains one function per rule. Each function takes `&ValidationContext` and returns `Vec<Diagnostic>`. The orchestration in `mod.rs` calls every rule function and concatenates the results.

`Diagnostic` has fields: `code`, `severity`, `message`, `location: Option<Location>`.

Rules that target specific AST nodes (E002, E003, E004, W003, W005) copy the node's `location` field directly into the diagnostic. Pipeline-level rules that have no specific source node (E001, E006, W004) emit `location: Some(Location { line: 1, col: 1 })`.

Rules that emit no diagnostics return an empty `Vec` — there are no special "skipped" or "passed" states.

### W002 — Credential keyword detection

W002 detects environment variable names that suggest a plaintext credential. The check uses **word-boundary splitting on `_`**, not substring matching:

```rust
let parts: Vec<&str> = var_name.split('_').collect();
parts.iter().any(|p| CREDENTIAL_KEYWORDS.contains(&p.to_uppercase().as_str()))
```

This prevents false positives on names like `STACKED_OVERFLOW` (contains "KEY" as a substring but not as a word component) while correctly flagging `API_KEY` or `DB_PASSWORD`.

W002 is suppressed for `EnvValue::Credentials` — using `credentials('x')` is the correct way to bind a credential.

### W007 — Unknown step and ValidationMode

W007 is the only rule whose severity changes based on `ValidationMode`:

| Mode | W007 behaviour |
|---|---|
| `Permissive` | Warning — pipeline is still valid |
| `Strict` | Error — pipeline is invalid |
| `Discovery` | Silent — diagnostic is not emitted |

Discovery mode is intended for the `get_unknown_keywords()` workflow: parse, collect unknowns, register them, then switch to Strict.

---

## Structural tester (`src/tester/mod.rs`)

`run_tests(pipeline: &Pipeline) -> TestSuite` runs exactly 10 tests and always returns a suite with 10 entries. There are no skipped results in the current implementation (the `skipped` counter is always 0, though the field is retained for forward compatibility).

Tests operate directly on the AST — they do not call the validator. They are independent checks that answer structural questions about pipeline shape, naming conventions, and potential security issues.

The secret detection in `test_no_plaintext_secrets` uses the same word-boundary splitting as W002 for names, plus a separate value heuristic for:

- GitHub tokens: `ghp_*`, `ghs_*`, `gho_*`, `github_pat_*`
- AWS access key IDs: `AKIA*`
- JWTs: three base64url segments separated by dots
- Long hex strings: 32+ characters of `[0-9a-f]`

---

## JSON API stability

The following are **breaking changes** that would affect consumers of the JSON API:

- Renaming a field on any AST type
- Changing an enum variant's serialised tag value
- Changing the type of a field (e.g. `EnvVar.value` from `String` to `EnvValue` was a breaking change, mitigated by the `Literal` variant serialising as a plain string)
- Removing a function from `lib.rs`

Non-breaking changes include:

- Adding a new optional field with `#[serde(skip_serializing_if = "Option::is_none")]`
- Adding a new function to `lib.rs`
- Adding a new diagnostic rule (new `code` values in the output)
- Adding a new `Agent` variant (new `agent_type` value in `get_pipeline_summary`)

---

## Constraints and non-negotiables

| Constraint | Reason |
|---|---|
| Rust 1.73.0 | System Rust on the WSL development environment |
| `pest = "=2.7.15"` | Pinned; newer versions require rustc ≥ 1.83 |
| No `std::sync::LazyLock` | Requires Rust 1.80+; use `OnceLock` instead |
| No new `Cargo.toml` dependencies | Binary size and compilation time — every dep adds to the WASM bundle |
| `./test.sh`, not bare `cargo test` | WSL + `/mnt/c/` filesystem caching; the script clears stale `target/debug/deps/integration-*` before each run |
| All public functions return `String` | WASM ABI constraint — only `Copy` types and `String`/`&str` cross the WASM boundary cleanly |

---

## Permanent limitations

| Limitation | Reason it won't be fixed |
|---|---|
| `script {}` body re-parsing | Arbitrary Groovy; a full Groovy parser is out of scope |
| Escaped quotes in strings (`'it\'s'`) | PEG grammar limitation; low real-world impact |
| `expression { }` with embedded `}` | Full Groovy expression parsing is out of scope |
| `kubernetes` `yaml` body as YAML | Would require adding a YAML parser dependency |
| `format_pipeline` pretty-printer | Requires a Groovy-aware formatter; out of scope |
| `list_literal` in non-param contexts | Options, triggers, call args don't need list values |
