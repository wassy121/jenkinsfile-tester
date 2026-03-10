# jenkinsfile-tester — Test Suite Documentation

This document explains every test in the suite: the pipeline snippet under test
and **why** the expected outcome is correct, invalid, insecure, or otherwise
unacceptable for a production Jenkins pipeline.

---

## Table of Contents

1. [Testing Strategy](#testing-strategy)
2. [Test Counts](#test-counts)
3. [Known Grammar Limitations](#known-grammar-limitations)
4. [§1 · Parser Tests](#1--parser-tests)
5. [§2 · Validator Tests (22 rules)](#2--validator-tests-22-rules)
6. [§3 · Structural Tester Tests (22 assertions)](#3--structural-tester-tests-22-assertions)
7. [§4 · API Contract Tests](#4--api-contract-tests)
8. [§5 · Unit Tests (inline)](#5--unit-tests-inline)

---

## Testing Strategy

The test suite uses three layers:

| Layer | Location | Purpose |
|-------|----------|---------|
| **Inline unit tests** | `src/validator/rules.rs`, `src/tester/mod.rs`, `src/ast/walk.rs` | Test individual rule/assertion functions directly against hand-crafted ASTs. No parser overhead. |
| **Integration tests** | `tests/integration.rs` | Test the full parse → validate/test chain through the public JSON API using realistic pipeline snippets. |
| **API contract tests** | `tests/integration.rs §4` | Assert every public WASM function returns valid JSON for any input, including garbage. |

---

## Test Counts

```bash
./test.sh   # WSL-safe test runner — use this, never bare cargo test
```

| Binary | Tests |
|--------|-------|
| Unit tests (`lib`) | 183 |
| Integration tests | 234 |
| **Total** | **417** |

All 417 pass.

---

## Known Grammar Limitations

The PEG grammar in `src/parser/jenkinsfile.pest` deliberately does not support
all Jenkins declarative syntax. The following patterns are known limitations:

### 1 · Escaped quotes in strings

`'it\'s'` and `"say \"hi\""` are not supported. The grammar does not handle
escaped quote characters inside string literals. Low real-world impact since
most Jenkins values do not contain embedded quotes of the same delimiter type.

### 2 · `expression { }` closures containing `}`

```groovy
when { expression { myHelper(x: '}') } }
```

`expression_body` uses heuristic lookahead to find the closing `}`. A closure
body containing `}` (even as a string literal argument) may parse incorrectly.
Full Groovy expression parsing is out of scope.

### 3 · `script {}` contents not re-parsed

```groovy
steps {
    script {
        def x = sh(script: 'git rev-parse HEAD', returnStdout: true).trim()
    }
}
```

The body of a `script {}` block is captured as opaque raw text. The grammar
does not parse the Groovy inside. The `script_blocks_are_small` structural
assertion inspects line count only; it cannot inspect what the Groovy actually
does.

### 4 · `kubernetes` `yaml` body not parsed as YAML

The `yaml: '''...'''` field in a kubernetes agent block is accepted as a raw
string. Its content is not re-parsed or validated as YAML. Adding a YAML parser
dependency is out of scope.

---

## §1 · Parser Tests

**File:** `tests/integration.rs` → `mod parser`

### Agent variants

#### `agent any`

```groovy
pipeline {
    agent any
    stages { stage('Build') { steps { sh 'make' } } }
}
```

**Valid.** `any` is the permissive agent specifier — Jenkins picks any available
executor. AST: `{ "type": "Any" }`. Note: the `no_agent_any` structural
assertion will flag this as a best-practice violation — `any` is valid Groovy
but inadvisable in production.

---

#### `agent none`

```groovy
pipeline {
    agent none
    stages { stage('Build') { agent { label 'java' }  steps { sh 'mvn package' } } }
}
```

**Valid.** Explicitly declares that no pipeline-level executor is needed. Every
stage that executes shell steps must declare its own agent (validated by E002).

---

#### `agent { label '...' }`

```groovy
pipeline {
    agent { label 'linux-x64' }
    stages { stage('Build') { steps { sh 'make' } } }
}
```

**Valid.** Allocates a node matching the given label expression. AST:
`{ "type": "Label", "value": "linux-x64" }`.

---

#### `agent { docker { image '...' } }`

```groovy
pipeline {
    agent {
        docker {
            image 'node:20.11.0-alpine'
            args  '-u root'
            customWorkspace '/workspace'
            alwaysPull true
        }
    }
    stages { stage('Build') { steps { sh 'npm ci' } } }
}
```

**Valid.** Runs the pipeline inside a Docker container. All sub-fields are
captured in the AST: `image`, `args`, `custom_workspace`, `reuse_node`,
`registry_url`, `registry_credentials_id`, `always_pull`.

---

#### `agent { dockerfile { ... } }`

```groovy
pipeline {
    agent {
        dockerfile {
            filename 'Dockerfile.ci'
            dir      'docker'
            additionalBuildArgs '--build-arg VERSION=1'
            label    'builder'
        }
    }
    stages { stage('Build') { steps { sh 'make' } } }
}
```

**Valid.** Builds a Docker image from a Dockerfile in the repository and uses it
as the agent. Fields: `filename`, `dir`, `additional_build_args`, `label`.

---

#### `agent { node { ... } }`

```groovy
pipeline {
    agent {
        node {
            label           'fast-linux'
            customWorkspace '/custom/ws'
        }
    }
    stages { stage('Build') { steps { sh 'make' } } }
}
```

**Valid.** Distinct from the bare `label` shorthand — the block form allows
`customWorkspace` alongside the label. AST: `Agent::Node { label, custom_workspace }`.

---

#### `agent { kubernetes { ... } }`

```groovy
pipeline {
    agent {
        kubernetes {
            yaml '''
                apiVersion: v1
                kind: Pod
                spec:
                  containers:
                  - name: maven
                    image: maven:3.8.6
            '''
            cloud            'prod-k8s'
            namespace        'ci'
            defaultContainer 'maven'
            inheritFrom      'base-pod'
            retries          2
        }
    }
    stages { stage('Build') { steps { sh 'mvn package' } } }
}
```

**Valid.** Typed kubernetes agent with 8 named sub-fields. All are `Option<...>`.
The `yaml` value is accepted as raw text (not re-parsed). AST:
`Agent::Kubernetes { yaml, yaml_file, cloud, namespace, default_container, inherit_from, retries, label }`.

---

#### `agent { myCustomPlugin { ... } }` (Generic catch-all)

```groovy
pipeline {
    agent { ec2 { ami 'ami-12345678'; instanceType 't3.medium' } }
    stages { stage('Build') { steps { sh 'make' } } }
}
```

**Valid parse.** Unknown agent types become `Agent::Generic { agent_type: "ec2" }`.
The body is captured opaquely. Validator rule W008 fires if `ec2` is not in the
active plugin registry.

---

### Parameters

#### `choice` with list literal

```groovy
parameters {
    choice(name: 'ENV', choices: ['dev', 'staging', 'prod'], description: 'Target environment')
}
```

**Valid.** The `list_literal` grammar rule (`[ value (, value)* ,? ]`) is wired
into `param_arg`, so the array form parses correctly. Inner values are extracted
via `unquote()` and stored in `Parameter::Choice { choices: Vec<String> }`.

---

#### `choice` with trailing comma (Groovy style)

```groovy
parameters {
    choice(name: 'ENV', choices: ['dev', 'staging',], description: '...')
}
```

**Valid.** Trailing comma in list literals is explicitly allowed (`","?` in the
grammar rule).

---

#### `run` parameter with filter

```groovy
parameters {
    run(name: 'UPSTREAM', projectName: 'my-job', filter: 'SUCCESSFUL')
}
```

**Valid.** The `run` parameter type references an upstream build. `filter` is
an optional field specifying which build statuses are selectable.

---

### Step variants

#### Triple-quoted `sh` string

```groovy
steps {
    sh '''
        set -e
        npm install
        npm run build
    '''
}
```

**Valid.** Triple-quoted strings are the idiomatic way to embed multi-line shell
scripts. The grammar lists `triple_string` before `double_string` in all
alternatives so `''` is never accidentally parsed as an empty string followed by
a stray `'`.

---

#### `script {}` block

```groovy
steps {
    script {
        def version = sh(script: 'git describe --tags', returnStdout: true).trim()
        env.RELEASE_VERSION = version
    }
}
```

**Valid parse.** Body is captured as `Step::Script { body: String }`. Contents
are not re-parsed or validated. The `script_blocks_are_small` structural
assertion inspects line count.

---

#### `withCredentials` block

```groovy
steps {
    withCredentials([
        usernamePassword(credentialsId: 'docker-hub', usernameVariable: 'DOCKER_USER', passwordVariable: 'DOCKER_PASS'),
        string(credentialsId: 'npm-token', variable: 'NPM_TOKEN'),
        sshUserPrivateKey(credentialsId: 'deploy-key', keyFileVariable: 'SSH_KEY'),
    ]) {
        sh 'docker login -u $DOCKER_USER -p $DOCKER_PASS'
    }
}
```

**Valid.** `withCredentials` bodies are parsed as `Step::WithCredentials { bindings: Vec<Binding>, steps: Vec<Step> }`. Five typed `Binding` variants exist plus `Raw` as a catch-all. Walk helpers recursively descend into `withCredentials` bodies when collecting steps.

---

#### Unknown step (Generic)

```groovy
steps {
    slackSend(channel: '#builds', message: 'Build started')
}
```

**Valid parse.** Becomes `Step::Generic { name: "slackSend", args: Vec<String> }`.
Validator rule W007 fires if `slackSend` is not in the active plugin registry.
Note: unknown steps **must** use call syntax with parentheses — `slackSend(...)`.
Bare `slackSend channel: '#builds'` (without parens) is not currently supported.

---

### When conditions

#### `when { not { branch '...' } }`

```groovy
stage('Feature Work') {
    when { not { branch 'main' } }
    steps { sh 'echo feature branch' }
}
```

**Valid.** `not` negates a single child condition. AST: `Not { condition: Branch { pattern: "main" } }`.

---

#### `when { allOf { ... } }` and `anyOf { ... }`

```groovy
when {
    allOf {
        branch 'main'
        environment name: 'DEPLOY_ENV', value: 'prod'
    }
}
```

**Valid.** `allOf` requires all child conditions to be true; `anyOf` requires at
least one. Validator rule S005 fires if either has fewer than 2 child conditions
(one-child allOf/anyOf is semantically equivalent to using the condition directly).

---

#### `when { tag 'v*' }`

```groovy
stage('Release') {
    when { tag 'v*' }
    steps { sh './release.sh' }
}
```

**Valid.** Restricts execution to builds triggered by a matching git tag.

---

#### `when` modifiers: `beforeAgent`, `beforeInput`, `beforeOptions`

```groovy
stage('Deploy') {
    agent { label 'prod-runner' }
    when {
        branch 'main'
        beforeAgent true   // evaluate condition BEFORE allocating the agent
    }
    steps { sh './deploy.sh' }
}
```

**Valid.** The `before_agent`, `before_input`, `before_options` fields are boolean
flags on the `When` struct. By default they are `false`. Setting `beforeAgent true`
prevents agent allocation for stages that will be skipped by the `when` condition —
a key CloudBees best-practice (see structural assertion `when_stages_use_before_agent`).

---

### Stage body types

#### Parallel

```groovy
stage('Test') {
    parallel {
        stage('Unit')        { steps { sh 'npm run unit'        } }
        stage('Integration') { steps { sh 'npm run integration' } }
    }
}
```

**Valid.** `StageBody::Parallel { stages: Vec<Stage> }`. The `fail_fast` field on
the outer stage controls whether a failing branch aborts the others.

---

#### Sequential (stages-within-stages)

```groovy
stage('Deploy') {
    stages {
        stage('Staging') { steps { sh './deploy.sh staging' } }
        stage('Prod')    { steps { sh './deploy.sh prod'    } }
    }
}
```

**Valid.** `StageBody::Sequential { stages: Vec<Stage> }`. The container stage has
no steps of its own — only nested stages.

---

#### Matrix

```groovy
stage('Cross-platform') {
    matrix {
        axes {
            axis { name 'OS';   values 'linux', 'mac', 'windows' }
            axis { name 'ARCH'; values 'x64', 'arm64' }
        }
        excludes {
            exclude {
                axis { name 'OS';   values 'windows' }
                axis { name 'ARCH'; values 'arm64'   }
            }
        }
        stages {
            stage('Build') { steps { sh './build.sh' } }
        }
    }
}
```

**Valid.** `StageBody::Matrix(Matrix { axes, excludes, stages })`. The matrix
expands into `n_axes_combinations - n_excludes` parallel executions at runtime.

---

### Environment values

#### `credentials()` binding

```groovy
environment {
    DB_PASSWORD = credentials('prod-db-secret')
}
```

**Valid.** Becomes `EnvValue::Credentials { id: "prod-db-secret" }`. Serialises
as `{ "type": "credentials", "id": "prod-db-secret" }`. Validator rules W002 and
W005 are suppressed for credentials-backed variables (they are correct credential
bindings, not plaintext values).

---

### Post conditions

All 10 post conditions are parsed and mapped to their named fields in the `Post` struct:
`always`, `success`, `failure`, `unstable`, `aborted`, `changed`, `cleanup`,
`regression`, `fixed`, `unsuccessful`.

---

### Shared libraries

```groovy
@Library('my-shared-lib@v2.1.0') _
```

**Valid.** Becomes `SharedLibrary { name: "my-shared-lib", ref_: Some("v2.1.0") }`.
The `ref_` field is split from the `@` separator. Missing `@` leaves `ref_: None`.

---

### Parse failures (expected)

| Input | Why it fails |
|-------|-------------|
| `""` (empty) | No `pipeline` keyword found |
| `"pipeline { broken }"` | `broken` is not a valid `pipeline_body` element |
| `"pipeline { agent any\n    stages {\n"` | Unclosed brace |
| `"pipeline { agent any }"` | `stages_decl` is required — omitting it is a parse error |

---

## §2 · Validator Tests (22 rules)

**File:** `tests/integration.rs` → `mod validator`

Each rule has at least one triggering pipeline (bad pattern, explains why it is
rejected) and one clean pipeline (confirms no false positive). Rules are tested
both via unit tests (hand-crafted ASTs, `src/validator/rules.rs`) and integration
tests (full parse → validate chain).

---

### E001 — No agent defined at pipeline level

**Triggers on:**

```groovy
pipeline {
    stages {
        stage('Build') { steps { sh 'make' } }
    }
}
```

**Why invalid:** Every Declarative Pipeline must declare a top-level agent. Jenkins
uses this to know where to run the pipeline or to verify that every stage declares
its own. Without it, the pipeline cannot execute and Jenkins raises a configuration
error at startup. Diagnostic carries sentinel location `{ "line": 1, "col": 1 }`.

**Result:** `is_valid: false`, code `E001`.

**Clean form:** Add `agent any` (or a specific label/docker/kubernetes agent).

---

### E002 — Stage has no steps block

**Triggers on:**

```groovy
pipeline {
    agent none
    stages {
        stage('Build') {
            steps { sh 'make all' }
            // ↑ stage has steps but no agent — E002 fires when pipeline agent is none
        }
    }
}
```

More precisely, E002 fires when `agent none` is declared at pipeline level and
a stage has shell steps but no stage-level agent. At runtime Jenkins throws:
*"Required context class hudson.FilePath is missing"*.

Diagnostic carries the stage's source location.

**Result:** `is_valid: false`, code `E002` mentioning the stage name.

**Clean form:** Add `agent { label 'java' }` (or any agent) to the stage.

---

### E003 — Duplicate stage names at the same level

**Triggers on:**

```groovy
pipeline {
    agent any
    stages {
        stage('Build') { steps { sh 'mvn package' } }
        stage('Build') { steps { sh 'mvn install' } }  // ← duplicate
    }
}
```

**Why invalid:** Jenkins uses stage names as keys for the build UI, timing data,
and test-result attribution. Two identical names at the same nesting level cause
the second stage to overwrite the first in all reporting. The same name at
*different* nesting levels (a top-level `Build` and a nested `Build`) is permitted.

Diagnostic carries the first duplicate stage's source location.

**Result:** `is_valid: false`, code `E003` containing `"Build"`.

---

### E004 — Duplicate parameter names

**Triggers on:**

```groovy
parameters {
    string(name: 'ENV', defaultValue: 'dev')
    choice(name: 'ENV', choices: ['dev', 'prod'])  // ← same name
}
```

**Why invalid:** Jenkins uses parameter names as keys in the build parameters map.
A duplicate name causes the second definition to silently shadow the first.
Downstream steps reading `params.ENV` receive an undefined value from the second
definition while the first is lost. Diagnostic carries the first duplicate
parameter's source location.

**Result:** `is_valid: false`, code `E004`.

---

### E005 — Duplicate section declarations

**Triggers on:**

```groovy
pipeline {
    agent any
    agent { label 'linux' }   // ← second agent block
    stages { stage('Build') { steps { sh 'make' } } }
}
```

**Why invalid:** The Declarative Pipeline DSL allows at most one of each top-level
section (`agent`, `environment`, `options`, `parameters`, `triggers`, `post`).
Declaring the same section twice is a parse-level semantic error in the Jenkins
DSL. The parser records duplicates in `Pipeline.duplicate_sections`; E005 reads
that field.

**Result:** `is_valid: false`, code `E005`.

---

### E006 — Pipeline has no stages

**Triggers on:**

```groovy
pipeline {
    agent any
    stages { }
}
```

**Why invalid:** A pipeline with an empty `stages {}` block defines no work. Running
it produces a "green" build that executed nothing — a silent no-op that masks
forgotten development work. Diagnostic carries sentinel location `{ "line": 1, "col": 1 }`.

**Result:** `is_valid: false`, code `E006`.

---

### W001 — Missing global timeout option

**Triggers on** any pipeline whose `options {}` block does not contain a `timeout(...)` call.

**Why a warning:** A pipeline with no global timeout runs indefinitely if a test
hangs waiting for a network resource, a deployment stalls, or a `sleep` call was
accidentally left in. This ties up Jenkins executors and can stall an entire
team's CI queue. The `S002` suggestion also fires, so both are reported.

**Result:** Warning `W001`.

**Clean form:** Add `options { timeout(time: 1, unit: 'HOURS') }`.

---

### W002 — Plaintext credential in environment variable name

**Triggers on:**

```groovy
environment {
    DB_PASSWORD = 'hunter2'   // ← literal string, name suggests credential
}
```

**Why a warning:** An environment variable named `DB_PASSWORD` with a literal
string value means the password is in source control, in build logs, and in any
export of the pipeline configuration. The detection splits variable names on `_`
and checks each word component against a keyword list: `PASSWORD`, `SECRET`,
`TOKEN`, `API_KEY`, `APIKEY`, `PASSWD`, `CREDENTIAL`, `CRED`, `KEY`.

Word-boundary splitting prevents false positives: `STACKED_CACHE` is not flagged
(no component matches), while `API_KEY` is (`KEY` is a word component).

Variables backed by `credentials()` are exempt — `DB_PASSWORD = credentials('x')`
is the correct pattern and does not trigger this warning.

**Result:** Warning `W002`.

**Clean form:** `DB_PASSWORD = credentials('db-secret-id')`

---

### W003 — Shell step without `set -e`

**Triggers on:**

```groovy
steps {
    sh '''
        npm install
        npm run build
        npm run lint
    '''
}
```

**Why a warning:** By default, bash continues executing after a non-zero exit
status. If `npm install` fails, bash silently proceeds to `npm run build` with an
incomplete `node_modules`. The build appears green despite producing broken
artifacts. `set -e` causes immediate abort on the first failing command.

The rule only fires for multi-line `sh` blocks (triple-quoted strings). A
single-line `sh 'make'` cannot chain commands in the same way.

Diagnostic carries the `sh` step's source location.

**Result:** Warning `W003`.

**Clean form:** Begin the script with `set -e` or `set -ex`.

---

### W004 — Missing post section

**Triggers on** any pipeline without a `post {}` block.

**Why a warning:** No post block means there is no hook for cleanup, notifications,
or artifact handling after any build outcome. Failed builds produce no Slack/email
alert. Workspaces are never cleaned. Artifacts are never archived. This is
acceptable for a throwaway prototype; a shared or production pipeline must have a
`post` block. Diagnostic carries sentinel location `{ "line": 1, "col": 1 }`.

**Result:** Warning `W004`.

---

### W005 — Hardcoded secret-shaped value in script

**Triggers on:**

```groovy
environment {
    GITHUB_TOKEN = 'ghp_abc123XYZsomeLongRandomString'
}
```

Or a literal in a `sh` step body that matches a known secret pattern.

**Why a warning:** Secret-shaped values in source code are immediately accessible
to anyone with repository read access, appear in build logs, and are retained
permanently in git history. Patterns detected:
- GitHub tokens: `ghp_*`, `ghs_*`, `gho_*`, `github_pat_*`
- AWS access key IDs: `AKIA*`
- JWTs: three base64url segments separated by `.`
- Long hex strings: 32+ chars of `[0-9a-f]`

Variables backed by `credentials()` are exempt. Diagnostic carries the `sh` step's
source location.

**Result:** Warning `W005`.

---

### W006 — Tool type not in plugin registry

**Triggers on:**

```groovy
tools {
    myCustomBuildTool 'version-1.2'
}
```

**Why a warning:** The `tools {}` block declares build tools that Jenkins should
automatically install on the agent. If the tool type (`myCustomBuildTool`) is not
registered in the active plugin registry, Jenkins cannot resolve the tool at
runtime. This typically causes a build failure at the tool-installation step.

**Result:** Warning `W006`.

---

### W007 — Unknown step name

**Triggers on** any `Step::Generic { name }` where `name` is not in the plugin
registry's `steps` list.

**Why a warning (Permissive mode) / error (Strict mode):** Unknown step names most
often indicate typos, missing plugin installations, or a mismatch between the
Jenkinsfile and the Jenkins instance's installed plugins.

**Behaviour by validation mode:**

| Mode | W007 behaviour |
|------|----------------|
| `Permissive` (default) | Warning — `is_valid: true` |
| `Strict` | Error — `is_valid: false` |
| `Discovery` | Silent — W007 is not emitted (use `get_unknown_keywords()` instead) |

---

### W008 — Agent type not in plugin registry

**Triggers on** `Agent::Generic { agent_type }` where `agent_type` is not in the
plugin registry's `agent_types` list.

**Why a warning:** A build using an unrecognised agent type will fail at the agent
allocation step. This indicates a missing plugin or a registry that hasn't been
updated to include the new agent plugin.

Note: `Agent::Kubernetes` is a typed variant — it is never `Agent::Generic` and
never triggers W008, even without a kubernetes entry in the registry.

**Result:** Warning `W008`.

---

### W009 — Pipeline option not in plugin registry

**Triggers on** `PipelineOption::Raw { name }` where `name` is not in the plugin
registry's `options` list.

**Why a warning:** An unrecognised option name in the `options {}` block is silently
ignored by Jenkins at runtime — the option has no effect. This indicates a typo
or a plugin that is not installed.

**Result:** Warning `W009`.

---

### W010 — Trigger not in plugin registry

**Triggers on** `Trigger::Raw { name }` where `name` is not in the plugin
registry's `triggers` list.

**Why a warning:** An unrecognised trigger name in the `triggers {}` block means
the pipeline will never be automatically triggered by that source. This indicates
a missing plugin or a mis-spelled trigger name.

**Result:** Warning `W010`.

---

### W011 — Groovy string interpolation of credential variable

**Triggers on** any `sh` or `echo` step where:
1. The step is inside a `withCredentials` block, and
2. The step argument is a double-quoted string (`"..."` or `"""`), and
3. The string body references one of the bound credential variable names via `${VAR}` or `$VAR`.

**Why a warning:** In Groovy, `"${VAR}"` is a GString — the Groovy runtime expands the
variable's value into the string *before* Jenkins executes the step. This means:

- For `sh`: the expanded secret value appears verbatim in the process argument list and
  in shell process listings (`ps aux`), bypassing the credential masking that Jenkins
  applies to `withCredentials`-bound variables.
- For `echo`: the secret is printed as plaintext into the build log, again before masking
  can occur.

The fix in both cases is to use single quotes: `sh 'command $MY_SECRET'` or
`echo 'value is $MY_SECRET'`. Single-quoted Groovy strings are not GStrings — they pass
the literal text `$MY_SECRET` to the shell/echo, which Jenkins then masks correctly.

**Covers binding types:** `usernamePassword` (both variables), `string`, `file`,
`sshUserPrivateKey` (key file + passphrase if set), `certificate` (keystore + password if set).

| Scenario | Result |
|----------|--------|
| `sh "curl -H 'Token: ${API_TOKEN}'"` inside `withCredentials([string(variable: 'API_TOKEN', ...)])` | FAIL — secret in process args |
| `sh 'curl -H "Token: $API_TOKEN"'` (single-quoted) | PASS — shell expands, Jenkins masks |
| `echo "Deploying as ${DEPLOY_USER}"` inside `withCredentials([usernamePassword(..., usernameVariable: 'DEPLOY_USER', ...)])` | FAIL — secret in log |
| `echo 'Build complete'` | PASS — no credential reference |

**Result:** Warning `W011`.

---

### S001 — No parameters defined

**Triggers on** any pipeline with an empty or absent `parameters {}` block.

**Why a suggestion:** Parameters are how users interact with a pipeline — selecting
branches, environments, or feature flags without editing the Jenkinsfile. A
pipeline with no parameters is often a prototype. Shared or production pipelines
benefit from at least a branch or environment parameter.

**Result:** Suggestion `S001`.

---

### S002 — No timeout option defined

**Triggers on** any pipeline whose `options {}` block contains no `timeout(...)`.
(S002 and W001 fire together on the same pipeline.)

**Why a suggestion:** Redundant with W001's explanation — included as a lower-severity
complement for pipelines that intentionally omit a timeout but should be reminded.

**Result:** Suggestion `S002`.

---

### S003 — Long pipeline with no stages

**Triggers on** a pipeline that has many options/parameters/triggers configured
but no stages defined. This heuristic fires when pipeline configuration is
substantial (suggesting real intent) but no actual work stages are present.

**Why a suggestion:** Usually indicates an incomplete pipeline that was committed
mid-development. The combination of configuration without stages is suspicious.

**Result:** Suggestion `S003`.

---

### S004 — Deep stage nesting detected

**Triggers on** stage nesting depth exceeding a threshold (parallel within
sequential within parallel, etc.).

**Why a suggestion:** Deep nesting makes pipelines hard to read and reason about.
The Jenkins Blue Ocean UI also has display limitations for deeply nested pipelines.
Refactoring deeply nested pipelines into shared library functions or separate
downstream pipelines usually produces clearer results. Diagnostic carries sentinel
location `{ "line": 1, "col": 1 }`.

**Result:** Suggestion `S004`.

---

### S005 — `allOf`/`anyOf` with fewer than 2 conditions

**Triggers on:**

```groovy
when {
    allOf {
        branch 'main'    // ← only one condition inside allOf
    }
}
```

**Why a suggestion:** `allOf { branch 'main' }` is semantically identical to
`branch 'main'`. The wrapper adds no logical value and makes the intent harder
to read. Use the bare condition directly.

**Result:** Suggestion `S005`.

---

## §3 · Structural Tester Tests (22 assertions)

**File:** `tests/integration.rs` → `mod tester`

The `run_tests()` function always returns exactly 21 results. Each assertion is
**PASS** or **FAIL** — there is no partial credit. Many assertions pass vacuously
(with a note that they were "skipped") when the relevant construct is absent from
the pipeline. This prevents noise on minimal pipelines.

Tests are grouped by concern below, matching their order in `run_tests()`.

---

### Shape and naming

#### `pipeline_has_stages`

| Scenario | Result | Reason |
|----------|--------|--------|
| `stages { stage('Build') { ... } }` | PASS | At least one stage present |
| `stages { }` (empty) | FAIL | No work is defined |

---

#### `all_stages_named`

| Scenario | Result | Reason |
|----------|--------|--------|
| All stages have non-empty names | PASS | Stages are identifiable in the UI |
| Stage with empty string name | FAIL | Unnamed stages cannot be tracked, reported, or debugged |

---

#### `no_placeholder_stage_names`

Checks for exact case-insensitive match against:
`TODO`, `FIXME`, `placeholder`, `stage`, `untitled`, `new stage`.

| Scenario | Result | Reason |
|----------|--------|--------|
| `stage('Build Docker Image')` | PASS | Descriptive name |
| `stage('TODO')` | FAIL | Incomplete placeholder — merging silently no-ops the stage |
| `stage('stage')` | FAIL | IDE snippet name left unchanged |
| `stage('FIXME')` | FAIL | Explicit marker that the stage is broken |
| `stage('Stagecoach')` | PASS | Contains "stage" as substring but is NOT an exact match |

---

#### `no_empty_steps_blocks`

| Scenario | Result | Reason |
|----------|--------|--------|
| Every stage has ≥1 step | PASS | |
| `stage('Scan') { steps { } }` | FAIL | Silently succeeds; often left behind after accidentally deleting step content |

---

#### `parallel_has_multiple_branches`

| Scenario | Result | Reason |
|----------|--------|--------|
| No parallel blocks | PASS (vacuous) | Rule not applicable |
| Parallel with 2+ branches | PASS | Useful concurrency |
| Parallel with 1 branch | FAIL | `parallel { stage('X') { ... } }` with one branch is semantically a sequential stage with extra syntax |

---

### Agent and execution environment

#### `agent_declared`

| Scenario | Result | Reason |
|----------|--------|--------|
| `agent any` | PASS | Explicit pipeline-level declaration |
| `agent none` | PASS | Explicit — stages must each declare their own |
| `agent { label 'linux' }` | PASS | Block form with label |
| No agent declaration | FAIL | Jenkins cannot determine where to run |

---

#### `no_agent_any`

*Source: CloudBees top-10 best practices.*

| Scenario | Result | Reason |
|----------|--------|--------|
| `agent { label 'linux' }` | PASS | Specific execution target |
| `agent { docker { image 'node:20' } }` | PASS | Containerised, reproducible |
| `agent none` | PASS | Explicit delegation to stages |
| `agent any` | FAIL | Jenkins may assign the build to any executor including the controller itself. Produces non-reproducible builds and risks running user code on the Jenkins controller |

Message: *"'agent any' allows Jenkins to assign the build to any executor including the controller — use a specific label, docker image, or kubernetes agent instead"*

---

#### `docker_images_pinned`

*Source: Jenkins Docker with Pipeline documentation.*

Vacuously passes when no Docker agents are declared.

| Scenario | Result | Reason |
|----------|--------|--------|
| `image 'node:20.11.0-alpine'` | PASS | Specific version tag |
| `image 'node:20.11.0@sha256:abc...'` | PASS | Digest — maximally pinned |
| `image 'node:latest'` | FAIL | Mutable tag — image content can change silently between runs |
| `image 'node:stable'` | FAIL | Mutable alias |
| `image 'node'` (no tag) | FAIL | Implicit `:latest` |

Message (failure): *"Docker images using mutable or absent tags — non-reproducible builds. Pin to a specific version or digest (e.g. 'node:20.11.0' or '@sha256:...'): [...]"*

---

#### `input_stages_no_agent`

*Source: CloudBees top-10 best practices — explicitly listed as their most
impactful anti-pattern.*

Vacuously passes when no stages have an `input` directive.

| Scenario | Result | Reason |
|----------|--------|--------|
| Stage with `input` and no stage agent | PASS | Correct — no executor held during approval |
| Stage with `input` and `agent none` | PASS | Explicit non-allocation |
| Stage with `input` and `agent { label '...' }` | FAIL | The agent executor and its workspace lock are held for the entire duration of the human approval window, potentially hours or days |

Message (failure): *"Stages with input directives that also allocate an agent — the executor and workspace will be held for the entire human approval duration: [...]"*

---

#### `when_stages_use_before_agent`

*Source: Jenkins pipeline syntax documentation.*

Vacuously passes when no stage combines a `when` condition with a stage-level agent.

| Scenario | Result | Reason |
|----------|--------|--------|
| Stage has `when` + `agent` + `beforeAgent true` | PASS | Agent not allocated for skipped stages |
| Stage has `when` but no `agent` | PASS | Not applicable |
| Stage has `when` + `agent` but no `beforeAgent true` | FAIL | Jenkins allocates the agent before evaluating the `when` condition. Every run of the pipeline triggers agent allocation even if the stage will be immediately skipped |

Message (failure): *"Stages with 'when' + 'agent' that are missing 'beforeAgent true' — Jenkins allocates the agent before evaluating the condition, wasting executors on every skipped run: [...]"*

---

### Build hygiene

#### `has_build_stage`

Checks for any stage name containing `build` or `compile` (case-insensitive).

| Scenario | Result | Reason |
|----------|--------|--------|
| Stage named `"Build"` | PASS | Direct match |
| Stage named `"Compile Sources"` | PASS | Contains "compile" |
| Stage named `"BUILD DOCKER IMAGE"` | PASS | Case-insensitive |
| Only `"Test"` and `"Deploy"` | FAIL | Pipeline ships code that was never compiled |

---

#### `has_test_stage`

Checks for any stage name containing `test`, `spec`, or `check` (case-insensitive).

| Scenario | Result | Reason |
|----------|--------|--------|
| Stage named `"Test"` | PASS | Direct match |
| Stage named `"Run Specs"` | PASS | Contains "spec" |
| Stage named `"Quality Check"` | PASS | Contains "check" |
| Only `"Build"` and `"Deploy"` | FAIL | **High risk** — shipping unverified code |

---

#### `has_build_discarder`

*Source: Jenkins official best practices + CloudBees top-10.*

| Scenario | Result | Reason |
|----------|--------|--------|
| `options { buildDiscarder(logRotator(numToKeepStr: '10')) }` | PASS | Build records are bounded |
| No `buildDiscarder` in options | FAIL | Jenkins accumulates unlimited build records on the controller filesystem. On a busy pipeline, this causes disk exhaustion and degrades controller performance over days or weeks |

Message (failure): *"No buildDiscarder configured — builds accumulate indefinitely on the controller, causing disk exhaustion. Add options { buildDiscarder(logRotator(numToKeepStr: '10')) }"*

---

#### `deploy_stages_disable_concurrent`

*Source: CloudBees enterprise governance documentation.*

Vacuously passes when no stage name contains `deploy`, `release`, `publish`, or
`promote` (case-insensitive substring match).

| Scenario | Result | Reason |
|----------|--------|--------|
| Deploy stage + `disableConcurrentBuilds()` in options | PASS | Concurrent runs are prevented |
| No deploy stages | PASS (vacuous) | Rule not applicable |
| Stage named `"Deploy to Prod"`, no `disableConcurrentBuilds` | FAIL | Two pipeline runs triggered close together will both reach the deploy stage simultaneously, causing a race condition, double-deploy, or environment corruption |

Message (failure): *"Pipeline has deployment stages ([...]) without 'disableConcurrentBuilds()' — concurrent runs can cause race conditions, double-deploys, and environment corruption"*

---

#### `checkout_not_duplicated`

*Source: Jenkins Jenkinsfile guide — stash/unstash pattern.*

| Scenario | Result | Reason |
|----------|--------|--------|
| No `checkout` step | PASS | Using implicit checkout |
| Single `checkout scm` | PASS | Source checked out once |
| `checkout scm` in 2+ stages | FAIL | Re-checking out source on every stage wastes bandwidth and SCM server load. Use `stash`/`unstash` to share files between agents instead |

---

#### `script_blocks_are_small`

*Source: Jenkins pipeline best practices — Groovy runs on the controller.*

Vacuously passes when no `script {}` blocks exist.

| Scenario | Result | Reason |
|----------|--------|--------|
| `script { }` with ≤15 non-empty lines | PASS | Within limit |
| `script { }` with 16+ non-empty lines | FAIL | `script {}` content executes on the Jenkins controller JVM. Large blocks consume controller heap and CPU, causing GC pauses and degraded performance for all pipelines on the instance. Complex logic belongs in shared library steps running on agents |

---

### Security

#### `no_plaintext_secrets`

Checks environment variables whose names contain `password`, `secret`, `token`,
`api_key`, `apikey`, `passwd`, `credential`, `cred`, or `key` as a `_`-delimited
word component. If the variable's value is a literal string (not `credentials()`),
the test fails. Additionally checks literal values for secret-shaped content:
GitHub tokens (`ghp_*`), AWS key IDs (`AKIA*`), JWTs, or 32+ character hex strings.

| Scenario | Result | Reason |
|----------|--------|--------|
| `DB_PASSWORD = credentials('prod-db')` | PASS | Credential store binding — correct |
| `DB_PASSWORD = 'hunter2'` | FAIL | **Insecure** — password in source control and build logs forever |
| `GITHUB_TOKEN = 'ghp_abc123...'` | FAIL | **Insecure** — GitHub PAT visible to any repo reader |
| `APP_NAME = 'my-service'` | PASS | Non-sensitive variable |

---

#### `no_secret_string_parameters`

*Source: Jenkins credentials security documentation.*

Vacuously passes when no parameters are declared.

| Scenario | Result | Reason |
|----------|--------|--------|
| `password(name: 'API_KEY', ...)` | PASS | Correct parameter type — value masked in UI |
| `string(name: 'BRANCH', ...)` | PASS | Non-sensitive name |
| `string(name: 'API_TOKEN', ...)` | FAIL | `string` parameters are stored and displayed in plaintext in build history and the Jenkins parameter UI. Any user with read access can see the value |
| `text(name: 'DB_PASSWORD', ...)` | FAIL | Same issue — `text` is also plaintext |

Credential keywords checked (word-boundary, case-insensitive):
`password`, `secret`, `token`, `key`, `credential`, `cred`, `auth`, `passwd`, `apikey`.

Message (failure): *"String or text parameters with sensitive names expose values in the Jenkins UI and build history — use the 'password' parameter type instead: [...]"*

---

#### `no_groovy_interpolated_credentials`

*Source: Jenkins official warning — "A secret was passed to `sh` using Groovy String interpolation, which is insecure."*

Checks every `sh` and `echo` step inside a `withCredentials` block. Fails if any such step
uses a double-quoted string that references a bound credential variable by name.

Vacuously passes when no `withCredentials` blocks are present.

| Scenario | Result | Reason |
|----------|--------|--------|
| `sh "curl ${API_TOKEN}"` inside `withCredentials` | FAIL | Groovy expands secret before shell sees the command |
| `sh 'curl $API_TOKEN'` inside `withCredentials` | PASS | Single-quoted — Jenkins masks the value correctly |
| `echo "Token: ${MY_TOKEN}"` inside `withCredentials` | FAIL | Secret printed to log before masking |
| `echo 'Deploying...'` inside `withCredentials` | PASS | No credential reference |
| Pipeline with no `withCredentials` blocks | PASS | Vacuous — nothing to check |

Message (failure): *"Credential variables passed to 'sh' or 'echo' via Groovy string interpolation — secrets are exposed and bypass masking. Use single-quoted strings instead."*

---

### Post and notifications

#### `post_block_exists`

| Scenario | Result | Reason |
|----------|--------|--------|
| Pipeline has `post { ... }` | PASS | Lifecycle hooks exist |
| No `post` block | FAIL | No mechanism for cleanup, notifications, or artifact handling |

---

#### `post_failure_handler_exists`

*Source: Jenkins pipeline tour / post-notification best practices.*

| Scenario | Result | Reason |
|----------|--------|--------|
| `post { failure { slackSend(...) } }` | PASS | Failed builds produce an alert |
| `post { always { cleanWs() } }` | PASS | `always` runs on failure too |
| `post { unsuccessful { ... } }` | PASS | Covers failure + unstable |
| `post { success { archiveArtifacts(...) } }` only | FAIL | Success-only post blocks are silent on failure — the most important outcome to handle |
| No `post` block at all | FAIL | No lifecycle hooks at all |

Message (failure): *"No post block handling failure outcomes — failed builds produce no notification or cleanup. Add 'post { failure { ... } }' or 'post { always { ... } }'"*

---

### Shared libraries

#### `libraries_pinned_to_version`

*Source: Jenkins shared libraries documentation.*

Vacuously passes when no shared libraries are declared.

| Scenario | Result | Reason |
|----------|--------|--------|
| `@Library('mylib@v2.1.0') _` | PASS | Pinned to a tag |
| `@Library('mylib@abc123def456') _` | PASS | Pinned to a commit SHA |
| `@Library('mylib@main') _` | FAIL | Mutable branch — a library change can silently break this pipeline overnight without any change to the Jenkinsfile |
| `@Library('mylib@master') _` | FAIL | Same |
| `@Library('mylib') _` (no ref) | FAIL | Resolves to the default branch, which may change |

Mutable refs blocked: `master`, `main`, `develop`, `HEAD`, `trunk`.

Message (failure): *"Shared libraries pinned to mutable branches — a library change can silently break this pipeline overnight. Pin to a tag or SHA: [...]"*

---

## §4 · API Contract Tests

**File:** `tests/integration.rs` → `mod api`

These tests verify the JSON contract that all JavaScript consumers depend on.

### `parse_jenkinsfile`

| Input | Expected output |
|-------|----------------|
| Valid pipeline | `{ "success": true, "ast": { ... } }` |
| Invalid syntax | `{ "success": false, "error": { "message": "...", "line": N, "col": N } }` |
| Empty string | `{ "success": false, "error": { ... } }` — never panics |

### `validate`

| Input | Expected output |
|-------|----------------|
| Valid pipeline, no issues | `{ "is_valid": true, "errors": [], "warnings": [...], "suggestions": [...] }` |
| Pipeline with E002 | `{ "is_valid": false, "errors": [{ "code": "E002", ... }], ... }` |
| Parse error | `{ "is_valid": false, "errors": [{ "code": "PARSE", ... }], ... }` |

### `get_ast_json`

| Input | Expected output |
|-------|----------------|
| Valid pipeline | Full AST JSON object |
| Invalid/garbage | The string `"null"` — not an error, not a crash |

### `get_stage_names`

| Input | Expected output |
|-------|----------------|
| Pipeline with stages | `["Checkout", "Build", "Test", "Deploy"]` |
| Pipeline with parallel | Includes both container and branch names |
| Invalid input | `"[]"` |
| Empty `stages {}` | `"[]"` |

### `get_pipeline_summary`

| Input | Expected output |
|-------|----------------|
| Valid pipeline | `{ "success": true, "stage_count": N, "agent_type": "...", ... }` |
| Parse error | `{ "success": false }` |

### `get_builtin_registry`

Always returns a JSON object `{ "plugins": [...] }` with 18 bundled plugin
entries. Never returns `null` or throws.

### `get_validation_rules`

Returns a JSON array of exactly 21 objects, each with `code`, `severity`, and
`description`. Always valid JSON; never throws.

### `all_functions_always_return_valid_json`

Tests all public functions against:
`""`, `"garbage"`, `"pipeline { }"`, a minimal valid pipeline, and the gold-standard
kitchen-sink pipeline.

**Contract:** Every function must return a string that parses as valid JSON. None
may panic or return a non-JSON string. This is critical because a WASM function
returning invalid text causes JavaScript `JSON.parse` to throw, which is
significantly harder to handle than a structured error object.

---

## §5 · Unit Tests (inline)

**Files:** `src/validator/rules.rs`, `src/tester/mod.rs`, `src/ast/walk.rs`

Unit tests construct AST objects directly and call individual private rule or
assertion functions, bypassing the parser entirely. They are the fastest tests
and provide the most precise diagnosis when a rule changes.

### Validator rules — unit test coverage

| Rule | Unit tests | Key cases covered |
|------|-----------|-------------------|
| E001 | 2 | Missing agent → error; agent present → clean |
| E002 | 4 | Agentless step stage fails; stage with agent clean; parallel container skipped |
| E003 | 3 | Duplicate fires; different scopes clean; exactly one E003 per duplicate |
| E004 | 3 | Duplicate param fires; unique params clean |
| E005 | 2 | Duplicate section entry fires; no duplicates clean |
| E006 | 2 | Empty stages fires; stages present clean |
| W001 | 2 | No timeout fires; timeout present clean |
| W002 | 3 | Literal credential var fires; `credentials()` binding suppresses; non-credential name clean |
| W003 | 4 | Multi-line without `set -e` fires; single-line clean; `set -e` clean; `set -ex` clean |
| W004 | 2 | No post fires; post present clean |
| W005 | 4 | GitHub token fires; AWS key fires; hex string fires; `credentials()` binding suppresses |
| W006–W010 | 2 each | Unregistered name fires; registered name clean |
| W011 | 3 | Double-quoted `sh` with cred var fires; single-quoted clean; `usernamePassword` double-quoted fires |
| S001–S005 | 2 each | Trigger condition fires; clean condition suppresses |

### Structural assertions — unit test coverage

| Assertion | Unit tests | Key cases covered |
|-----------|-----------|-------------------|
| `pipeline_has_stages` | 3 | Passes with stages; fails empty; message includes count |
| `all_stages_named` | 2 | All named passes; empty name fails |
| `agent_declared` | 4 | `any`, `none`, `label` all pass; missing agent fails |
| `no_placeholder_stage_names` | 6 | Real names pass; TODO/FIXME/stage fail; case-insensitive; substring NOT flagged |
| `post_block_exists` | 2 | Post present passes; absent fails |
| `has_build_stage` | 4 | "Build" / "compile" / case-insensitive pass; only "Test" fails |
| `has_test_stage` | 4 | "Test" / "spec" / "check" pass; no test stage fails |
| `no_plaintext_secrets` | 4 | `credentials()` passes; literal password/token fails; unrelated var passes |
| `parallel_has_multiple_branches` | 3 | No parallel vacuous-pass; 2 branches passes; 1 branch fails |
| `no_empty_steps_blocks` | 2 | Non-empty passes; empty steps fails with stage name |
| `has_build_discarder` | 2 | BuildDiscarder option passes; absent fails |
| `no_agent_any` | 2 | Label agent passes; `Agent::Any` fails |
| `docker_images_pinned` | 4 | Pinned tag passes; `:latest` fails; no tag fails; no Docker agents vacuous-pass |
| `input_stages_no_agent` | 3 | No input stages vacuous-pass; input+no-agent passes; input+agent fails |
| `when_stages_use_before_agent` | 3 | No matching stages vacuous-pass; `beforeAgent true` passes; missing fails |
| `no_secret_string_parameters` | 4 | No params vacuous-pass; safe name passes; PASSWORD string param fails; password type passes |
| `libraries_pinned_to_version` | 4 | No libs vacuous-pass; SHA ref passes; `main` ref fails; no ref fails |
| `script_blocks_are_small` | 3 | No scripts vacuous-pass; small script passes; 16-line script fails |
| `checkout_not_duplicated` | 3 | No checkout passes; single passes; two checkouts fails |
| `deploy_stages_disable_concurrent` | 3 | No deploy stages vacuous-pass; deploy+option passes; deploy without option fails |
| `post_failure_handler_exists` | 4 | `failure` block passes; `always` block passes; success-only fails; no post fails |
| `no_groovy_interpolated_credentials` | 3 | No withCredentials vacuous-pass; single-quoted sh passes; double-quoted sh with cred var fails |
| Suite metadata | 3 | Always 22 tests; passed+failed+skipped=22; skipped count is always 0 |

### AST walk helpers — unit test coverage

**File:** `src/ast/walk.rs`

| Function | Tests | Cases covered |
|----------|-------|---------------|
| `collect_all_stages` | 5 | Empty slice; flat stages; parallel container + branches; sequential nesting; deep mixed nesting |
| `collect_all_steps_recursive` | 3 | Empty; flat; `withCredentials` body recursion |
| `collect_env_vars_for_stage` | 2 | Pipeline + stage env merged; stage key overrides pipeline key |
