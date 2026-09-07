# Pipeline Testing Reference

## JenkinsPipelineUnit

[JenkinsPipelineUnit](https://github.com/jenkinsci/JenkinsPipelineUnit) is a Groovy
testing framework that loads and executes Jenkinsfiles in a sandboxed environment.

### Setup (Gradle)
```groovy
dependencies {
    testImplementation 'com.lesfurets:jenkins-pipeline-unit:1.9'
    testImplementation 'junit:junit:4.13.2'
}
```

### Setup (Maven)
```xml
<dependency>
    <groupId>com.lesfurets</groupId>
    <artifactId>jenkins-pipeline-unit</artifactId>
    <version>1.9</version>
    <scope>test</scope>
</dependency>
```

### Key classes
- `BasePipelineTest` — base class, provides `helper`, `binding`, `runScript()`
- `PipelineTestHelper` — manages call stack, registered methods, library loading
- `MethodCall` — represents a step invocation with method name and arguments

### Common helper methods
```groovy
// Register a method mock
helper.registerAllowedMethod('sh', [String], { cmd -> '' })
helper.registerAllowedMethod('sh', [Map], { map -> '' })

// Register shared library
helper.registerSharedLibrary(
    library()
        .name('my-lib')
        .defaultVersion('main')
        .allowOverride(true)
        .implicit(false)
        .targetPath('libs')
        .retriever(localSource('libs'))
        .build()
)

// Set environment variables
binding.setVariable('env', [BUILD_NUMBER: '1', BRANCH_NAME: 'main'])
binding.setVariable('params', [DEPLOY: true])
binding.setVariable('currentBuild', [result: 'SUCCESS'])

// Assert status
assertJobStatusSuccess()
assertJobStatusFailure()
assertJobStatusUnstable()

// Inspect call stack
helper.callStack  // List<MethodCall>
helper.callStack.findAll { it.methodName == 'sh' }
MethodCall.callArgsToString(call)  // String representation of args
```

### Mocking credentials
```groovy
helper.registerAllowedMethod('withCredentials', [List, Closure], { list, body ->
    list.each { cred ->
        if (cred instanceof Map) {
            binding.setVariable(cred.variable ?: 'CRED', 'mock-secret')
        }
    }
    body()
})

helper.registerAllowedMethod('credentials', [String], { id -> "mock-${id}" })
```

### Mocking Docker
```groovy
def mockDocker = [
    image: { String name -> [inside: { Closure body -> body() }, pull: { -> }] },
    build: { String tag -> [push: { -> }] }
]
binding.setVariable('docker', mockDocker)
```

### Testing parallel stages
```groovy
@Test
void 'parallel stages execute'() {
    def script = runScript('Jenkinsfile')
    assertJobStatusSuccess()

    // Both parallel branches should have run
    def shCalls = helper.callStack.findAll { it.methodName == 'sh' }
    assert shCalls.any { callArgsToString(it).contains('unit-test') }
    assert shCalls.any { callArgsToString(it).contains('integration-test') }
}
```

## Spock Framework

[Spock](http://spockframework.org) is a Groovy testing framework with expressive
given/when/then blocks.

### Setup (Gradle)
```groovy
dependencies {
    testImplementation 'org.spockframework:spock-core:2.3-groovy-4.0'
}
```

### Basic spec structure
```groovy
import spock.lang.Specification

class MySpec extends Specification {
    def 'descriptive test name'() {
        given: 'preconditions'
        def input = 'value'

        when: 'action'
        def result = myMethod(input)

        then: 'assertion'
        result == 'expected'
    }

    def 'data-driven test'() {
        expect:
        Math.max(a, b) == c

        where:
        a | b | c
        1 | 3 | 3
        7 | 4 | 7
    }
}
```

### Testing shared library vars/
```groovy
class DeployStepTest extends Specification {
    def helper = new PipelineTestHelper()

    def setup() {
        helper.setUp()
        helper.registerAllowedMethod('sh', [String], { '' })
        helper.registerAllowedMethod('echo', [String], { '' })
        helper.registerAllowedMethod('error', [String], { msg -> throw new RuntimeException(msg) })
    }

    def script = helper.loadScript('vars/deploy.groovy')
}
```

## CPS-Safe Testing Patterns

Jenkins Pipeline uses Continuation-Passing Style (CPS) for serialization.
Test code must account for CPS constraints:

1. **`@NonCPS` methods** can use non-serializable objects but cannot call CPS-transformed steps
2. **Closures** in CPS context must be serializable
3. **Try/catch in CPS** has limitations — use `catchError` step instead when possible

### Testing @NonCPS methods
```groovy
def 'NonCPS method works with non-serializable types'() {
    given:
    def script = helper.loadScript('vars/utils.groovy')

    when:
    def result = script.parseJson('{"key": "value"}')

    then:
    result.key == 'value'
}
```

## Structural Assertions (jenkinsfile-tester)

The jenkinsfile-tester `run_structural_tests` runs 22 builtin assertions.
Custom assertions can check:

| Assertion type | What it checks |
|---------------|----------------|
| `stage_exists` | Named stage present in pipeline |
| `agent_type` | Pipeline or stage agent matches expected type |
| `has_post` | Post section exists with specified condition |
| `has_timeout` | Timeout option configured |
| `param_exists` | Named parameter with expected default |
| `env_exists` | Environment variable defined (literal or credential) |
| `has_when` | Stage has when condition |
| `step_used` | Specific step type appears in pipeline |
