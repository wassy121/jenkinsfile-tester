# jenkinsfile-tester

A WebAssembly library that parses, validates, and structurally tests Jenkins Declarative Pipeline files — with no Jenkins server required.

Paste a `Jenkinsfile`, get back structured JSON: parse errors with line and column, named validation diagnostics (errors, warnings, suggestions), and a suite of structural assertions. Runs in the browser, Node.js, CLI (via wasmtime), or Docker — all via WebAssembly.

---

## Quick start

### Browser demo

```bash
node server.js          # start the demo at http://localhost:3000
node server.js 8080     # or on a custom port
```

### CLI (wasmtime)

```bash
cargo build --bin jenkinsfile-tester --target wasm32-wasi --release

wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm validate        < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm validate-strict < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm run-tests       < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm parse           < Jenkinsfile
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm dump-registry

# Custom plugin registry:
wasmtime run target/wasm32-wasi/release/jenkinsfile-tester.wasm dump-registry > my-plugins.json
wasmtime run --dir=. target/wasm32-wasi/release/jenkinsfile-tester.wasm \
  --registry my-plugins.json validate < Jenkinsfile
```

Exit codes: 0 = valid/pass, 1 = invalid/fail, 2 = usage error.

### Docker

```bash
# Pull the pre-built image from GitHub Container Registry:
docker pull ghcr.io/wassy121/jenkinsfile-tester

# Or build locally:
docker build -t jenkinsfile-tester .

docker run --rm -i ghcr.io/wassy121/jenkinsfile-tester validate < Jenkinsfile
docker run --rm -i ghcr.io/wassy121/jenkinsfile-tester validate-strict < Jenkinsfile
docker run --rm -i ghcr.io/wassy121/jenkinsfile-tester run-tests < Jenkinsfile
docker run --rm    ghcr.io/wassy121/jenkinsfile-tester dump-registry

# Custom plugin registry:
docker run --rm -i -v ./my-plugins.json:/registry.json \
  ghcr.io/wassy121/jenkinsfile-tester --dir=/ --registry /registry.json validate < Jenkinsfile
```

The image is `FROM scratch` — contains only wasmtime, its shared libraries, and the `.wasm` binary. It is automatically built and published to `ghcr.io/wassy121/jenkinsfile-tester` on every push to `main` and on version tags (`v*`).

### WASM library (browser / Node.js)

The `pkg/` directory contains a pre-built WASM bundle. To rebuild from source:

```bash
wasm-pack build --target web    # browser ES module → pkg/
wasm-pack build --target nodejs # Node.js → pkg/
```

---

## Import

### Browser (ES module)

```js
import init, {
  parse_jenkinsfile,
  validate,
  validate_strict,
  validate_with_registry,
  validate_with_exact_registry,
  init_registry,
  get_unknown_keywords,
  run_tests,
  get_ast_json,
  get_stage_names,
  get_pipeline_summary,
  get_builtin_registry,
  get_validation_rules,
} from './pkg/jenkinsfile_tester.js';

await init(); // loads the .wasm binary — call once at startup
```

### Node.js

```js
const wasm = require('./pkg');
// all functions available as wasm.validate(src), etc.
```

Every function accepts and returns plain strings (JSON). None of them throw — parse failures and registry errors are returned as structured JSON.

---

## API reference

### `parse_jenkinsfile(src)`

Parse a Jenkinsfile. On success, returns the full AST. On failure, returns a structured parse error with line and column numbers.

```js
const result = JSON.parse(parse_jenkinsfile(src));

// Success
{ "success": true, "ast": { "agent": { ... }, "stages": [ ... ], ... } }

// Failure
{ "success": false, "error": { "message": "expected '{'", "line": 7, "col": 3 } }
```

---

### `validate(src)`

Validate a Jenkinsfile in **Permissive mode** (the default). Unknown steps, agent types, options, and triggers produce warnings rather than errors — useful during initial adoption when your full plugin list isn't registered yet.

```js
const result = JSON.parse(validate(src));
// {
//   "is_valid": true,
//   "errors":      [],
//   "warnings":    [ { "code": "W003", "severity": "warning", "message": "...", "location": { "line": 14, "col": 5 } } ],
//   "suggestions": [ { "code": "S001", "severity": "suggestion", "message": "...", "location": null } ]
// }
```

`is_valid` is `true` only when `errors` is empty. `location` is present on diagnostics where a specific source line is known; it is `null` for pipeline-level rules that have no single source node (E001, E006, W004).

---

### `validate_strict(src)`

Same as `validate()` but W007 (unknown step name) is promoted to an **error**, making the pipeline invalid. Use this as a CI gate once your plugin registry is fully configured.

```js
const result = JSON.parse(validate_strict(src));
// { "is_valid": false, "errors": [{ "code": "W007", "severity": "error", ... }], ... }
```

---

### `validate_with_registry(src, registryJson)`

Validate using a custom plugin registry **merged into the built-in one** for this call. The registry is not stored — it applies to this call only. Use this when you need to supplement the built-ins with your organisation's plugins.

```js
const registry = JSON.stringify({
  plugins: [{
    plugin_id: "my-deploy-plugin",
    version: "2.3.1",
    contributes: {
      steps:       [{ name: "deployToEnvironment" }, { name: "rollbackDeployment" }],
      agent_types: [],
      options:     [],
      triggers:    []
    }
  }]
});

const result = JSON.parse(validate_with_registry(src, registry));
```

---

### `validate_with_exact_registry(src, registryJson)`

Like `validate_with_registry` but uses **only** the supplied registry — the built-in plugins are excluded entirely. Use this to enforce that a pipeline uses only a curated, approved set of plugins.

```js
// Only steps in my approved registry will be considered known
const result = JSON.parse(validate_with_exact_registry(src, approvedRegistryJson));
```

---

### `init_registry(registryJson)`

Merge a custom registry into the built-in one and store it for all subsequent `validate()` and `validate_strict()` calls. Call this once at startup after loading your plugin manifest.

```js
const ok = JSON.parse(init_registry(registryJson));
// { "ok": true }
// { "ok": false, "error": "invalid JSON: ..." }
```

---

### `get_unknown_keywords(src)`

Returns a sorted, deduplicated JSON array of step names found in the pipeline that are **not** registered in the active plugin registry. Returns `[]` on parse error.

```js
const unknown = JSON.parse(get_unknown_keywords(src));
// ["deployToProduction", "notifyOnCall"]
```

Use this in discovery mode: paste a Jenkinsfile from an existing Jenkins instance, find all the unregistered steps, then add them to your registry before switching to Strict mode.

---

### `run_tests(src)`

Run 22 structural assertions and return a named test suite.

```js
const suite = JSON.parse(run_tests(src));
// {
//   "name": "Jenkins Pipeline Structural Tests",
//   "passed": 18, "failed": 3, "skipped": 0,
//   "tests": [
//     { "name": "pipeline_has_stages",    "passed": true,  "message": "Pipeline has 3 stage(s)" },
//     { "name": "has_build_discarder",    "passed": false, "message": "No buildDiscarder configured — builds accumulate indefinitely..." },
//     ...
//   ]
// }
```

The 21 tests are:

**Shape and naming**

| Test name | Passes when |
|---|---|
| `pipeline_has_stages` | At least one stage is defined |
| `all_stages_named` | No stage has an empty name |
| `no_placeholder_stage_names` | No stage is named TODO, FIXME, placeholder, stage, untitled, or new stage |
| `no_empty_steps_blocks` | No stage has a `steps {}` block with zero steps |
| `parallel_has_multiple_branches` | Every `parallel {}` block has at least 2 branches (passes vacuously when no parallel blocks exist) |

**Agent and execution environment**

| Test name | Passes when |
|---|---|
| `agent_declared` | A pipeline-level `agent` is declared |
| `no_agent_any` | Top-level agent is not `any` — a specific label, Docker image, or Kubernetes pod is declared |
| `docker_images_pinned` | All Docker agent images use an explicit version tag, not `:latest` or no tag (passes vacuously when no Docker agents exist) |
| `input_stages_no_agent` | No stage with an `input` directive also allocates an agent — avoids holding executors during human approval (passes vacuously when no input stages exist) |
| `when_stages_use_before_agent` | All stages with both a `when` condition and a stage-level `agent` include `beforeAgent true` — avoids allocating agents for stages that will be skipped (passes vacuously when no such stages exist) |

**Build hygiene (CloudBees / Jenkins official best practices)**

| Test name | Passes when |
|---|---|
| `has_build_stage` | A stage name contains "build" or "compile" (case-insensitive) |
| `has_test_stage` | A stage name contains "test", "spec", or "check" (case-insensitive) |
| `has_build_discarder` | `options {}` contains `buildDiscarder(logRotator(...))` — prevents unlimited build accumulation on the controller |
| `deploy_stages_disable_concurrent` | If any stage name contains deploy, release, publish, or promote: `disableConcurrentBuilds()` is in `options {}` (passes vacuously when no deployment stages exist) |
| `checkout_not_duplicated` | `checkout scm` appears at most once across all stages — use `stash`/`unstash` to share files between agents |
| `script_blocks_are_small` | All `script {}` blocks contain ≤ 15 non-empty lines — large scripts run on the controller and cause GC pressure (passes vacuously when no script blocks exist) |

**Security**

| Test name | Passes when |
|---|---|
| `no_plaintext_secrets` | No env variable with a sensitive name (PASSWORD, TOKEN, SECRET, KEY…) holds a literal string value |
| `no_secret_string_parameters` | No `string` or `text` parameter has a credential-like name — use the `password` parameter type instead (passes vacuously when no parameters are declared) |
| `no_groovy_interpolated_credentials` | No `sh` or `echo` step inside `withCredentials` uses a double-quoted string containing a bound credential variable — single-quoted strings prevent Groovy from expanding the secret before Jenkins can mask it |

**Post and notifications**

| Test name | Passes when |
|---|---|
| `post_block_exists` | A `post {}` block is declared |
| `post_failure_handler_exists` | The `post {}` block contains a `failure`, `unsuccessful`, or `always` condition — failed builds produce notifications or cleanup |

**Shared libraries**

| Test name | Passes when |
|---|---|
| `libraries_pinned_to_version` | All `@Library` imports are pinned to a specific tag or SHA, not a mutable branch like `main` or `master` (passes vacuously when no libraries are declared) |

---

### `get_ast_json(src)`

Returns the full parsed AST as a JSON string, or `"null"` on parse failure. Equivalent to `parse_jenkinsfile` but returns the AST directly without the `success` wrapper.

```js
const ast = JSON.parse(get_ast_json(src)); // Pipeline object, or null
```

---

### `get_stage_names(src)`

Returns a flat JSON array of all stage names, recursing into parallel, sequential, and matrix stages. Returns `[]` on parse failure.

```js
JSON.parse(get_stage_names(src))
// ["Build", "Test", "Unit Tests", "Integration Tests", "Deploy to Staging", "Deploy to Prod"]
```

---

### `get_pipeline_summary(src)`

Returns a lightweight metadata object without the full AST. Useful for dashboards and list views.

```js
const summary = JSON.parse(get_pipeline_summary(src));
// {
//   "success":         true,
//   "stage_count":     5,
//   "has_post":        true,
//   "agent_type":      "docker",
//   "parameter_count": 2,
//   "has_triggers":    true,
//   "has_environment": true
// }
// { "success": false }  — on parse error
```

`agent_type` is one of: `any`, `none`, `label`, `docker`, `dockerfile`, `node`, `kubernetes`, `generic`.

---

### `get_builtin_registry()`

Returns the full built-in plugin registry as a JSON string `{ "plugins": [...] }`. Useful for building plugin-selection UIs or for inspecting what is covered out of the box.

---

### `get_validation_rules()`

Returns all 22 rule definitions as a JSON array. Useful for building a help panel or rule reference in a UI.

```js
JSON.parse(get_validation_rules())
// [
//   { "code": "E001", "severity": "error",      "description": "No agent defined at pipeline level" },
//   { "code": "W007", "severity": "warning",    "description": "Unknown step name not in plugin registry" },
//   { "code": "S001", "severity": "suggestion", "description": "No parameters defined" },
//   ...
// ]
```

---

## Validation rules

### Errors — `is_valid: false`

| Code | Description |
|---|---|
| E001 | No `agent` declared at pipeline level |
| E002 | A stage has no `steps {}` block |
| E003 | Duplicate stage names at the same nesting level |
| E004 | Duplicate parameter names |
| E005 | Duplicate section declarations (e.g. two `agent {}` blocks in the same scope) |
| E006 | Pipeline has no stages defined |

### Warnings — pipeline runs but has risks

| Code | Description |
|---|---|
| W001 | Missing global `timeout` option — a hung build will run forever |
| W002 | Environment variable name contains a credential keyword (PASSWORD, SECRET, TOKEN, KEY…) with a literal string value |
| W003 | Multi-line `sh` step without `set -e` — a failing command will not abort the step |
| W004 | No `post {}` block — build outcome handling (cleanup, notifications) is missing |
| W005 | Hardcoded secret-shaped value in a script (GitHub tokens `ghp_*`, AWS key IDs `AKIA*`, JWTs, 32-character hex strings) |
| W006 | `tools {}` entry references a tool type not in the plugin registry |
| W007 | Step name is not found in the plugin registry (warning in Permissive mode, error in Strict mode, silent in Discovery mode) |
| W008 | Agent type is not found in the plugin registry |
| W009 | Pipeline option name is not found in the plugin registry |
| W010 | Trigger name is not found in the plugin registry |
| W011 | Credential variable referenced in a double-quoted `sh` or `echo` string — Groovy interpolation embeds the raw secret value and bypasses credential masking |

### Suggestions — style and best-practice notes

| Code | Description |
|---|---|
| S001 | No `parameters {}` block defined |
| S002 | No `timeout` option defined |
| S003 | Pipeline has stages but none contain steps |
| S004 | Stage nesting depth is unusually deep |
| S005 | `allOf` or `anyOf` when condition has fewer than 2 child conditions |

---

## Plugin registry

The built-in registry covers 18 common Jenkins plugins. It includes steps, agent types, options, and triggers contributed by: Pipeline, Git, Docker, Kubernetes, Slack, JUnit, Cobertura, Artifactory, SonarQube, Jira, and others.

If your Jenkins instance uses additional plugins, supply them in the registry JSON format and pass to `validate_with_registry()` or `init_registry()`:

```json
{
  "plugins": [
    {
      "plugin_id": "my-deploy-plugin",
      "version": "2.3.1",
      "contributes": {
        "steps":       [{ "name": "deployToEnvironment" }, { "name": "rollbackDeployment" }],
        "agent_types": ["myCloud"],
        "options":     ["myBuildOption"],
        "triggers":    ["myWebhookTrigger"]
      }
    }
  ]
}
```

---

## What the parser handles

### Agent declarations

```groovy
agent any
agent none
agent { label 'linux' }
agent { docker { image 'node:20'; args '-u root'; customWorkspace '/workspace'; alwaysPull true } }
agent { dockerfile { filename 'Dockerfile.ci'; dir 'docker'; additionalBuildArgs '--build-arg VERSION=1' } }
agent { node { label 'linux'; customWorkspace '/workspace' } }
agent { kubernetes {
  yaml '''
    apiVersion: v1
    kind: Pod
    spec: ...
  '''
  cloud 'prod'
  namespace 'ci'
  defaultContainer 'maven'
  inheritFrom 'base'
  retries 2
} }
agent { anyOtherPlugin { ... } }   // → captured as Agent::Generic; W008 fires if unregistered
```

### Parameters

```groovy
parameters {
  string(name: 'BRANCH',     defaultValue: 'main',  description: 'Branch to build')
  booleanParam(name: 'SKIP_TESTS', defaultValue: false)
  choice(name: 'ENV', choices: ['dev', 'staging', 'prod'], description: 'Target environment')
  text(name: 'RELEASE_NOTES', defaultValue: '')
  password(name: 'API_KEY',  defaultValue: '', description: 'Auth token')
  file(name: 'CONFIG_FILE',  description: 'Upload a config file')
  run(name: 'UPSTREAM',      projectName: 'my-job', filter: 'SUCCESSFUL')
}
```

### Stage bodies

```groovy
// Simple
stage('Build') {
  steps { sh 'make build' }
}

// Parallel branches
stage('Test') {
  parallel {
    stage('Unit')        { steps { sh 'make test-unit' } }
    stage('Integration') { steps { sh 'make test-int'  } }
  }
}

// Sequential stages (stages-within-stages)
stage('Deploy') {
  stages {
    stage('Staging') { steps { sh './deploy.sh staging' } }
    stage('Prod')    { steps { sh './deploy.sh prod'    } }
  }
}

// Matrix
stage('Cross-platform') {
  matrix {
    axes {
      axis { name 'OS';   values 'linux', 'mac', 'windows' }
      axis { name 'ARCH'; values 'x64', 'arm64' }
    }
    excludes {
      exclude {
        axis { name 'OS'; values 'windows' }
        axis { name 'ARCH'; values 'arm64' }
      }
    }
    stages {
      stage('Build') { steps { sh './build.sh' } }
    }
  }
}
```

### When conditions

```groovy
when {
  branch 'main'
  tag 'v*'
  environment name: 'DEPLOY_ENV', value: 'production'
  expression { env.BRANCH_NAME ==~ /release\/.+/ }
  not { branch 'main' }
  allOf { branch 'main'; environment name: 'X', value: 'Y' }
  anyOf { branch 'main'; branch 'release/*' }
  changeRequest()
  buildingTag()
  changelog '.*\\[ci skip\\].*'
  changeset '**/*.java'
  triggeredBy 'TimerTrigger'
  equals expected: 'true', actual: env.FLAG
  beforeAgent  true   // evaluate when condition before allocating an agent
  beforeInput  true
  beforeOptions true
}
```

### Steps

```groovy
sh 'single line'
sh '''
  set -e
  echo "multi-line step"
  make build
'''
echo 'a message'
checkout scm
withCredentials([
  usernamePassword(credentialsId: 'my-cred', usernameVariable: 'USER', passwordVariable: 'PASS'),
  string(credentialsId: 'api-token', variable: 'TOKEN'),
  sshUserPrivateKey(credentialsId: 'ssh-key', keyFileVariable: 'KEY_FILE'),
]) {
  sh 'curl -u $USER:$PASS ...'
}
// Any step registered in the active plugin registry:
junit 'target/surefire-reports/**/*.xml'
slackSend(channel: '#builds', message: 'Build complete')
```

### Options

```groovy
options {
  timeout(time: 1, unit: 'HOURS')
  buildDiscarder(logRotator(numToKeepStr: '10', daysToKeepStr: '30'))
  retry(3)
  disableConcurrentBuilds()
  skipDefaultCheckout()
  skipStagesAfterUnstable()
  preserveStashes(buildCount: 5)
  timestamps()
  ansiColor('xterm')
  newContainerPerStage()
  quietPeriod(30)
  checkoutToSubdirectory('src')
  disableResume()
}
```

### Environment

```groovy
environment {
  APP_NAME    = 'my-service'                           // EnvValue::Literal — validated for secret patterns
  DB_PASSWORD = credentials('db-secret')              // EnvValue::Credentials — W002/W005 are suppressed
  VERSION     = "${BASE_VERSION}".replace('-SNAPSHOT', '') // arbitrary Groovy expressions supported
}
```

### Triggers

```groovy
triggers {
  cron('H/15 * * * *')
  pollSCM('H/5 * * * *')
  upstream(upstreamProjects: 'other-job', threshold: hudson.model.Result.SUCCESS)
  githubPush()
}
```

### Post conditions

`always`, `success`, `failure`, `unstable`, `aborted`, `changed`, `fixed`, `regression`, `cleanup`, `unsuccessful`

### Shared libraries

```groovy
@Library('my-shared-lib@main') _
```

---

## Known limitations

| Limitation | Notes |
|---|---|
| `script {}` bodies | Captured as opaque raw text with brace-balanced matching — nested `{}` blocks work, but Groovy is not re-parsed |
| Escaped quotes in strings | `'it\'s'` and `"say \"hi\""` are not supported by the grammar |
| `expression { }` containing `}` | Uses heuristic lookahead; closures with embedded `}` may not parse correctly |
| `kubernetes` `yaml` body | Accepted as a raw string; not re-parsed as YAML |
| Pretty-printing | No `format_pipeline` function — a Groovy-aware formatter is out of scope |

---

## Building from source

**Prerequisites:** Rust 1.73.0, [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)

```bash
./test.sh            # run all 417 tests (WSL-safe — use this, not bare cargo test)
./test.sh my_filter  # run a subset

wasm-pack build --target web     # → pkg/ for browsers
wasm-pack build --target nodejs  # → pkg/ for Node.js
cargo build                      # native debug build
```

> **WSL users:** Always use `./test.sh`. Bare `cargo test` on the `/mnt/c/` filesystem can pick up stale build artifacts and produce false failures.

---

## Project layout

```
jenkinsfile-tester/
├── src/
│   ├── ast/
│   │   ├── mod.rs       # All AST types: Pipeline, Stage, Step, Agent, Parameter, …
│   │   └── walk.rs      # Traversal helpers: collect_all_stages, walk_steps_with_stage, …
│   ├── parser/
│   │   ├── jenkinsfile.pest  # PEG grammar (pest 2.7.15)
│   │   └── mod.rs            # Grammar → AST builder; parse() returns Result<Pipeline, ParseError>
│   ├── plugins/
│   │   └── mod.rs       # PluginRegistry with 18 built-in plugins; merge, has_step, has_agent_type, …
│   ├── validator/
│   │   ├── mod.rs        # validate(), validate_with_context(), ValidationMode
│   │   ├── context.rs    # ValidationContext holding Arc<PluginRegistry> + ValidationMode
│   │   └── rules.rs      # 22 individual diagnostic rules (E001–E006, W001–W011, S001–S005)
│   ├── tester/
│   │   └── mod.rs       # 22 structural assertions (run_tests)
│   ├── main.rs          # CLI binary (wasm32-wasi); --registry flag, dump-registry command
│   └── lib.rs           # WASM entry point; 13 public functions; THREAD_REGISTRY thread-local
├── tests/
│   └── integration.rs   # 234 integration tests (417 total with unit tests)
├── docs/                # GitHub Pages output (generated by build-pages.sh)
├── pkg/                 # Pre-built WASM bundle (committed for demo convenience)
├── demo/
│   └── index.html       # Browser playground with plugin drawer and localStorage persistence
├── .github/workflows/
│   └── docker-publish.yml  # GitHub Actions: build + push Docker image to ghcr.io
├── Dockerfile           # Multi-stage: Alpine builder → FROM scratch runtime
├── build-pages.sh       # Build WASM + assemble docs/ for GitHub Pages
├── server.js            # Minimal static file server for the demo
├── test.sh              # WSL-safe test runner
├── ARCHITECTURE.md      # Programmatic architecture and design decisions
├── CLAUDE.md            # Internal architecture reference for AI agents
└── BACKLOG.md           # Development history and sprint log
```
