# Declarative Pipeline Syntax Reference

Derived from the jenkinsfile-tester PEG grammar and Jenkins documentation.

## Top-level structure

```
pipeline {
    agent <agent-spec>
    [environment { ... }]
    [options { ... }]
    [parameters { ... }]
    [triggers { ... }]
    [tools { ... }]
    [libraries { lib('name@version') ... }]
    stages {
        stage('Name') { ... }
        ...
    }
    [post { ... }]
}
```

All sections are optional except `agent` (E001) and `stages` (E006).

## Agent specifications

```groovy
agent any
agent none
agent { label 'worker' }
agent { docker { image 'node:20'; args '-v /tmp:/tmp' } }
agent { dockerfile { filename 'Dockerfile.ci'; dir 'docker'; label 'docker-builder' } }
agent { node { label 'linux'; customWorkspace '/opt/build' } }
agent { kubernetes { yaml '''...'''; defaultContainer 'jnlp' } }
```

## Environment

```groovy
environment {
    FOO = 'bar'                              // literal
    SECRET = credentials('cred-id')          // credential binding
    COMPUTED = "${env.BUILD_NUMBER}-suffix"   // expression (caution: GString)
}
```

Stage-level environment overrides/extends pipeline-level.

## Options

Common options (from builtin plugin registry):
- `timeout(time: N, unit: 'MINUTES'|'HOURS'|'SECONDS')`
- `buildDiscarder(logRotator(numToKeepStr: 'N'))`
- `disableConcurrentBuilds()`
- `retry(N)`
- `timestamps()`
- `skipDefaultCheckout()`
- `skipStagesAfterUnstable()`
- `checkoutToSubdirectory('dir')`
- `preserveStashes(buildCount: N)`
- `quietPeriod(N)`
- `ansiColor('xterm')`

## Parameters

```groovy
parameters {
    string(name: 'BRANCH', defaultValue: 'main', description: '...')
    booleanParam(name: 'DEPLOY', defaultValue: false, description: '...')
    choice(name: 'ENV', choices: ['dev', 'staging', 'prod'], description: '...')
    text(name: 'CONFIG', defaultValue: '', description: '...')
    password(name: 'TOKEN', defaultValue: '', description: '...')
}
```

## Triggers

```groovy
triggers {
    cron('H/15 * * * *')
    pollSCM('H/5 * * * *')
    upstream(upstreamProjects: 'job-name', threshold: hudson.model.Result.SUCCESS)
}
```

## Branch scanning and build trigger control

GitHub Organization and Multibranch projects automatically trigger builds when
branches are discovered during periodic scanning. This is usually desirable for
application CI pipelines, but can cause problems for infrastructure, ad-hoc, or
utility jobs that should only run on-demand or on a schedule.

### When to suppress branch scanning triggers
- **Infrastructure/provisioning jobs** — should only run manually or on schedule
- **Ad-hoc utility pipelines** — database migrations, cache invalidation, one-off scripts
- **Release/deploy-only pipelines** — should only trigger from manual approval or upstream
- **Expensive jobs** — long-running performance suites, full integration environments

For standard application CI (build + test on every push), the default behavior
(build on branch discovery + webhooks) is typically correct.

### `overrideIndexTriggers(false)`
Suppresses builds triggered by branch scanning. Builds will only run from explicit
triggers (webhooks, `triggers {}` block, manual start).
```groovy
options {
    overrideIndexTriggers(false)
}
```

### Combining with explicit triggers
```groovy
options {
    overrideIndexTriggers(false)
}
triggers {
    cron('H 2 * * *')  // nightly only — no branch scan, no webhook
}
```
When `overrideIndexTriggers(false)` is set and no `triggers {}` block is present,
builds are only triggered by SCM webhooks or manual runs.

### `triggeredBy` when condition
Run or skip stages based on what caused the build:
```groovy
stage('Nightly Tests') {
    when { triggeredBy 'TimerTrigger' }
    steps { sh 'make test-full' }
}
stage('Deploy') {
    when { triggeredBy 'UserIdCause' }  // manual only
    steps { sh 'make deploy' }
}
```
Available causes: `TimerTrigger`, `SCMTrigger`, `UpstreamCause`, `UserIdCause`,
`BranchIndexingCause`.

### `beforeAgent true`
When using `when` to skip stages, add `beforeAgent true` to avoid allocating an
executor just to evaluate the condition:
```groovy
stage('Deploy') {
    when {
        branch 'main'
        beforeAgent true
    }
    steps { sh 'make deploy' }
}
```

## Stage body variants

### Steps (default)
```groovy
stage('Name') {
    steps { sh 'command' }
}
```

### Parallel
```groovy
stage('Parallel') {
    parallel {
        stage('A') { steps { ... } }
        stage('B') { steps { ... } }
    }
}
```

### Sequential (nested stages)
```groovy
stage('Parent') {
    stages {
        stage('Child 1') { steps { ... } }
        stage('Child 2') { steps { ... } }
    }
}
```

### Matrix
```groovy
stage('Matrix') {
    matrix {
        axes {
            axis { name 'OS'; values 'linux', 'mac' }
        }
        [excludes { exclude { axis { name 'OS'; values 'mac' } } }]
        stages {
            stage('Build') { steps { ... } }
        }
    }
}
```

## When conditions

```groovy
when {
    branch 'main'
    branch pattern: 'release-*', comparator: 'GLOB'
    expression { return params.DEPLOY == true }
    environment name: 'ENV', value: 'production'
    tag 'v*'
    changeRequest()
    buildingTag()
    allOf { branch 'main'; environment name: 'ENV', value: 'prod' }
    anyOf { branch 'main'; branch 'develop' }
    not { branch 'experimental' }
    triggeredBy 'TimerTrigger'
    changeset 'src/**'
    equals expected: 'hello', actual: "${env.GREETING}"
}
```

`beforeAgent true` — evaluate `when` before allocating an agent.

## Post conditions

```groovy
post {
    always { ... }
    success { ... }
    failure { ... }
    unstable { ... }
    aborted { ... }
    changed { ... }
    fixed { ... }
    regression { ... }
    cleanup { ... }
    unsuccessful { ... }
}
```

## Common steps (builtin registry)

### Shell/batch
- `sh 'command'` / `sh script: '...', returnStdout: true`
- `bat 'command'` / `powershell 'command'`

### SCM
- `checkout scm` / `checkout([$class: 'GitSCM', ...])`
- `git url: '...', branch: 'main'`

### Archiving
- `archiveArtifacts artifacts: '**/*.jar', fingerprint: true`
- `stash name: 'build', includes: 'dist/**'`
- `unstash 'build'`

### Testing
- `junit '**/test-results/*.xml'`

### Credentials
- `withCredentials([string(credentialsId: 'id', variable: 'VAR')]) { ... }`
- `withCredentials([usernamePassword(credentialsId: 'id', usernameVariable: 'U', passwordVariable: 'P')]) { ... }`

### Notifications
- `echo 'message'`
- `mail to: '...', subject: '...', body: '...'`
- `slackSend channel: '#ci', message: '...'`

### Flow control
- `retry(N) { ... }`
- `timeout(time: N, unit: 'MINUTES') { ... }`
- `waitUntil { ... }`
- `sleep time: N, unit: 'SECONDS'`
- `input message: 'Proceed?', ok: 'Deploy'`
- `error 'message'`

### Docker
- `docker.build('image:tag')`
- `docker.image('image:tag').inside { ... }`

## Validation rules (22 total)

### Errors (E001-E006) — pipeline will not work
### Warnings (W001-W011) — risks or bad practices
### Suggestions (S001-S005) — nice to have

See `get_validation_rules` tool for full list.
