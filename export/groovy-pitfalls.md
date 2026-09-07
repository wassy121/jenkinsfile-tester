# Groovy Pitfalls in Jenkins Pipelines

## CPS Serialization Issues

### Problem: Non-serializable objects across steps
```groovy
// FAILS — JsonSlurper result is not serializable
def json = new groovy.json.JsonSlurper().parseText(response)
sh "echo ${json.version}"  // CPS checkpoint here — json can't serialize
```

### Fix: Use @NonCPS or readJSON step
```groovy
// Option 1: @NonCPS method
@NonCPS
def parseJson(String text) {
    new groovy.json.JsonSlurper().parseText(text)
}

// Option 2: Jenkins readJSON step (pipeline-utility-steps plugin)
def json = readJSON text: response
```

### Common non-serializable types
| Type | Alternative |
|------|------------|
| `JsonSlurper` result | `readJSON` step or `@NonCPS` |
| `Matcher` | `@NonCPS` method or `=~` in `@NonCPS` |
| `Iterator` | Convert to List first |
| `Stream` | Collect to List in `@NonCPS` |
| `InputStream` | Read fully before step boundary |

## Closure Issues

### Problem: each {} in CPS
```groovy
// May fail unpredictably in CPS
items.each { item ->
    sh "process ${item}"
}
```

### Fix: Use for loop
```groovy
for (def item in items) {
    sh "process ${item}"
}
// Or with index
for (int i = 0; i < items.size(); i++) {
    sh "process ${items[i]}"
}
```

## Map Iteration

### Problem
```groovy
// FAILS in CPS — entrySet() returns non-serializable Iterator
config.each { key, value ->
    echo "${key}=${value}"
}
```

### Fix
```groovy
for (def entry in config) {
    echo "${entry.key}=${entry.value}"
}
// Or
for (def key in config.keySet()) {
    echo "${key}=${config[key]}"
}
```

## GString vs String

### Problem: credential exposure
```groovy
// Groovy resolves ${TOKEN} at compile time, before Jenkins masking
sh "curl -H 'Auth: ${TOKEN}' https://api.example.com"
```

### Fix: single quotes
```groovy
sh 'curl -H "Auth: $TOKEN" https://api.example.com'
```

### When GString IS appropriate
```groovy
echo "Building branch: ${env.BRANCH_NAME}"  // non-sensitive
def msg = "Build #${env.BUILD_NUMBER} ${currentBuild.result}"  // non-sensitive
```

## Transient Closure Fields (CPS Serialization)

### Problem: Closure fields cause NotSerializableException
Groovy `Closure` objects are **not** Java-serializable. If a `Serializable` class
stores Closure fields (e.g., hook maps, callbacks), Jenkins will throw
`NotSerializableException` when it hits a CPS checkpoint (`input`, agent allocation, etc.).

```groovy
// FAILS at CPS checkpoint
class PipelineConfig implements Serializable {
    final Map<String, Closure> hooks = [:]  // Closure is NOT serializable
}
```

### Fix: mark Closure fields `transient`
```groovy
class PipelineConfig implements Serializable {
    private static final long serialVersionUID = 1L
    transient final Map<String, Closure> preStageHooks
    transient final Map<String, Closure> postStageHooks
    transient final Map<String, Closure> phaseOverrides
}
```

**Critical:** After CPS deserialization, transient fields are `null`. Every access
site must null-guard:
```groovy
// In vars/ pipeline code
def override = cfg.phaseOverrides != null ? cfg.phaseOverrides.get(key) : null
if (override != null) {
    override()
}
```

### Also applies to
- `Closure` fields on any `Serializable` class
- `Map<String, Closure>` fields (hook registries, callback maps)
- Any field whose type does not implement `Serializable`

## Parallel Closure Variable Capture

### Problem: CPS race condition in parallel branches
```groovy
// WRONG — mod variable may be mutated by loop before closure executes
for (def mod in modules) {
    branches[mod.name] = { runBuild(mod) }
}
```

### Fix: capture loop variable
```groovy
for (def mod in modules) {
    def captured = mod  // snapshot for closure
    branches[captured.name] = { runBuild(captured) }
}
```

## Variable Scope

### Problem: variables not available in parallel branches
```groovy
def result  // defined outside parallel
parallel {
    stage('A') {
        steps {
            script { result = sh(returnStdout: true, script: 'cmd') }
        }
    }
    stage('B') {
        steps {
            script { echo result }  // may be null — race condition
        }
    }
}
```

### Fix: use stash/unstash or separate variables
```groovy
parallel {
    stage('A') {
        steps {
            sh 'cmd > result.txt'
            stash name: 'result', includes: 'result.txt'
        }
    }
    stage('B') {
        steps {
            unstash 'result'
            sh 'cat result.txt'
        }
    }
}
```

## Return Values

### Problem: script block return value lost
```groovy
stage('Check') {
    steps {
        script {
            return shouldDeploy()  // return value is discarded
        }
    }
}
```

### Fix: set environment variable or use when condition
```groovy
environment {
    SHOULD_DEPLOY = ''
}
stages {
    stage('Check') {
        steps {
            script { env.SHOULD_DEPLOY = shouldDeploy() ? 'true' : 'false' }
        }
    }
    stage('Deploy') {
        when { environment name: 'SHOULD_DEPLOY', value: 'true' }
        steps { sh 'deploy.sh' }
    }
}
```
