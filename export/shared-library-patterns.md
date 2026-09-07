# Shared Library Patterns

## Language Rule

**Use Kotlin for `src/`. Use Groovy only where Jenkins forces it (`vars/`,
`Jenkinsfile`).**

Groovy is the wrong language for type-safe `src/` logic:

- No compile-time type checking — typos in field names, wrong argument types,
  and missing branches all fail at runtime, often deep inside a pipeline run.
- No exhaustive `when` / `switch` — adding a new enum value silently leaves
  branches unhandled.
- No null-safety — `?.` is opt-in and easy to forget.
- CPS surprises — Groovy classes in `src/` aren't CPS-transformed, but Groovy
  closures stored on them aren't serializable; CPS pitfalls in `vars/`
  (`each {}`, `Map.entrySet()`, GString masking) leak into thinking about
  `src/` even when they shouldn't.
- Anaemic interop story — to call Groovy from Groovy you must already know the
  shape of the object; refactoring is mechanical and unsafe.

Kotlin in `src/`:

- Compiles to plain JVM bytecode — **not** CPS-transformed, every Kotlin
  feature is available (`when`, sealed classes, data classes, lambdas,
  collection ops, null-safety).
- Synthesizes Java getters automatically, so Groovy `vars/` and Spock tests
  consume Kotlin classes via property syntax (`obj.foo`) with no extra effort.
- `@JvmStatic` + `@JvmOverloads` give clean call sites from Groovy.
- Exhaustive `when` on a typed enum makes "I added a new tool and forgot to
  handle it somewhere" a compile error.

The rest of this file presents the Kotlin-first patterns. Groovy `src/`
samples appear only where explicitly marked as legacy.

## Directory Structure

```
my-pipeline-lib/
├── vars/                              # Groovy — CPS-transformed pipeline DSL (required)
│   ├── deployToEnv.groovy             # Thin orchestration; delegates to src/
│   ├── notifySlack.groovy
│   └── standardPipeline.groovy
├── src/com/myorg/                     # Kotlin — typed business logic
│   ├── PipelineConfig.kt              # Typed config + validation
│   ├── BuildPhase.kt                  # enum class : Serializable
│   ├── ModuleBuilder.kt               # Returns commands; never calls steps
│   ├── IacTool.kt                     # Strategy interface
│   ├── IacToolFactory.kt              # object IacToolFactory
│   └── iac/
│       ├── TerraformTool.kt
│       └── HelmTool.kt
├── resources/                         # Non-code resources
├── test/
│   ├── com/myorg/                     # Layer 1 — pure Spock over Kotlin SUTs
│   │   ├── PipelineConfigTest.groovy
│   │   └── ModuleBuilderTest.groovy
│   └── vars/                          # Layer 2 — JenkinsPipelineUnit over vars/
│       ├── PipelineSpec.groovy        # Shared base class
│       └── StandardPipelineTest.groovy
├── build.gradle.kts                   # Kotlin + Groovy compilation
└── Jenkinsfile                        # CI for the library itself
```

## vars/ — Groovy Orchestration

`vars/*.groovy` is the only place Groovy is required. Keep these files thin:
they call Jenkins DSL steps and delegate every decision to a Kotlin class in
`src/`. No `switch`, no string-building, no business logic.

### The `call()` convention
```groovy
// vars/deployToEnv.groovy
import com.myorg.DeploymentManager

def call(Map config = [:]) {
    // Construction validates inputs and throws with a clear error.
    def mgr = new DeploymentManager(config.env, config.tag)

    withCredentials([string(credentialsId: "${mgr.environment}-deploy-key", variable: 'KEY')]) {
        sh mgr.deployCommand()    // Kotlin returns the command string
        sh mgr.verifyCommand()    // vars/ calls the DSL step
    }
}
```

### Multiple methods in one file
```groovy
// vars/utils.groovy — thin wrappers; logic lives in Kotlin
def parseJson(String text) {
    readJSON text: text
}
```

### @NonCPS in vars/
```groovy
// vars/transform.groovy — use sparingly; prefer extracting to Kotlin instead
@NonCPS
def call(List items) {
    items.collect { it.toUpperCase() }.sort()
}
```

Whenever `@NonCPS` is tempting, that's a signal the logic belongs in a Kotlin
class — Kotlin in `src/` isn't CPS-transformed, so the constraint doesn't
apply and the code becomes testable without runtime tricks.

## src/ — Kotlin (Default)

`src/` classes are pure Kotlin. They take inputs, return data (shell command
strings, typed value objects, contract data), and never call Jenkins pipeline
steps. This makes every class testable with plain Spock — no Jenkins runtime,
no step mocking.

```kotlin
// src/com/myorg/ModuleBuilder.kt
package com.myorg

import java.io.Serializable

class ModuleBuilder : Serializable {

    /** Returns the shell command string — does NOT execute it. */
    fun buildCommand(modulePath: String, type: ModuleType): String {
        Preconditions.sanitizePath(modulePath)
        return when (type) {
            ModuleType.JAVA   -> "./gradlew :${modulePath}:build -x test"
            ModuleType.NODE   -> "cd '${modulePath}' && npm ci && npm run build"
            ModuleType.PYTHON -> "cd '${modulePath}' && python -m build"
            // exhaustive when — adding a new ModuleType is a compile error here
        }
    }

    companion object { private const val serialVersionUID: Long = 1L }
}
```

Usage in `vars/`:
```groovy
// vars/buildModule.groovy — orchestration only
import com.myorg.ModuleBuilder
import com.myorg.ModuleType

def call(Map config) {
    def builder = new ModuleBuilder()
    sh builder.buildCommand(config.path, ModuleType.parse(config.type))
}
```

Test with plain Spock (no Jenkins mocking, no `BasePipelineTest`):
```groovy
class ModuleBuilderTest extends Specification {
    def builder = new ModuleBuilder()

    def 'java build command uses gradlew'() {
        expect:
        builder.buildCommand('api', ModuleType.JAVA) == './gradlew :api:build -x test'
    }

    def 'rejects path traversal'() {
        when:  builder.buildCommand('../etc', ModuleType.JAVA)
        then:  thrown(IllegalArgumentException)
    }
}
```

### Why this works from Groovy

Kotlin synthesizes a Java getter for every `val` and `var` property. Groovy
exposes Java getters as properties, so `mgr.environment` from Groovy reads
through `getEnvironment()` on the Kotlin class. No `@JvmField` needed; in
fact, **avoid `@JvmField`** so you preserve the ability to refactor a `val`
into a typed accessor method later without breaking callers.

## Groovy / Kotlin Interop Annotations

| Annotation | Use it when | Avoid it when |
|------------|-------------|---------------|
| `@JvmStatic` | Companion-object methods, factory `of()`, anything called as `Foo.bar()` from Groovy. | Never harmful — apply liberally. |
| `@JvmOverloads` | Constructor / function has default parameters Groovy needs to call. | — |
| `@JvmField` | You truly want a Java public field (no getter generated). | **Almost always.** Letting Kotlin synthesize the getter preserves refactor-ability and keeps `transient` semantics straightforward. |
| `@field:Transient` | Closure-valued fields and any non-serializable JVM field on a `Serializable` class. Targets the JVM backing field, not the getter. | Don't combine with `@JvmField` — exposing a transient public field defeats the typed-accessor pattern. |

## Type Safety Patterns

### Typed enums with a `parse(raw)` companion

Stringly-typed config (`executionStrategy: 'parallel'`) spreads tolerant
parsing — and tolerant bugs — across every call site. Parse once at the
boundary into a Kotlin enum:

```kotlin
enum class ExecutionStrategy : Serializable {
    PARALLEL, AGGREGATED;

    fun keyword(): String = name.lowercase()

    companion object {
        @JvmStatic
        fun parse(raw: Any?): ExecutionStrategy {
            if (raw is ExecutionStrategy) return raw
            if (raw is String) {
                val key = raw.lowercase()
                for (s in values()) if (s.keyword() == key) return s
            }
            throw IllegalArgumentException(
                "executionStrategy must be one of " +
                    "${values().joinToString(", ") { it.keyword() }}, got '${raw ?: "null"}'"
            )
        }
    }
}
```

Apply this shape to every enum-shaped config value: post conditions, tool
names, phase identifiers, deploy targets. Downstream `when` on the enum is
**exhaustive** — the compiler enforces every value is handled, and the user
gets an actionable enumerated error message for typos.

### Typed value objects from user-supplied Maps

Groovy Jenkinsfile syntax forces users to write Maps. Parse them into typed
value objects **at construction** so downstream code never indexes `Map<*, *>`:

```kotlin
class ExtraStageDefinition(
    name: String?, agent: String?, steps: List<String>?, whenExpression: String?
) : Serializable {
    val name: String = Preconditions.requireNonEmpty(name, "extraStages[].name")
    val agent: String? = agent
    val steps: List<String> =
        steps?.let { Collections.unmodifiableList(ArrayList(it)) } ?: emptyList()
    val whenExpression: String? = whenExpression

    companion object {
        @JvmStatic
        fun fromMap(raw: Map<*, *>?, index: Int): ExtraStageDefinition { /* per-field type-check */ }
        private const val serialVersionUID: Long = 1L
    }
}
```

Tolerate unknown keys for forward compatibility; reject **wrong types on
known keys** with a clear error.

### Typed accessor methods over raw maps

Some maps must stay keyed by user-supplied strings (hook registries). Don't
make every caller build the key inline — wrap each map in a typed method on
the config class:

```kotlin
class PipelineConfig : Serializable {
    @field:Transient val phaseOverrides: Map<String, Closure<*>>?

    /** Module-specific key first, then global phase key. Null-safe. */
    fun phaseOverride(moduleName: String, phase: BuildPhase): Closure<*>? {
        val po = phaseOverrides ?: return null
        return po["$moduleName:${phase.hookKey()}"] ?: po[phase.hookKey()]
    }
}
```

`vars/` calls `cfg.phaseOverride(mod.name, phase)` — no string keys in
orchestration code, no null guards repeated at every call site.

### Strategy + Factory for extension points

When a `vars/` entry point dispatches on a user-supplied tool name (IaC tools,
aggregation tools, deploy targets), express the surface as a Kotlin interface
plus a Kotlin `object` factory. Adding a tool localizes to one enum value,
one factory branch, one impl class, and one Spock spec — `vars/` never changes:

```kotlin
interface IacTool : Serializable {
    fun name(): IacToolName
    fun planChangesCommand(): String
    fun provisionCommand(): String
}

object IacToolFactory : Serializable {
    @JvmStatic
    fun of(name: String?, config: Map<String, Any?>): IacTool =
        of(IacToolName.parse(name), config)

    @JvmStatic
    fun of(tool: IacToolName, config: Map<String, Any?>): IacTool = when (tool) {
        IacToolName.TERRAFORM -> TerraformTool(/* required(config, ...) */)
        IacToolName.PULUMI    -> PulumiTool(/* ... */)
        // exhaustive — compiler enforces every enum value is handled
    }
}
```

### Shared validation utility

Extract common validation (null checks, path sanitization) into a single
Kotlin object so every `src/` class shares one implementation:

```kotlin
object Preconditions {
    @JvmStatic
    fun requireNonEmpty(value: String?, name: String): String {
        if (value.isNullOrEmpty()) throw IllegalArgumentException("$name must not be null or empty")
        return value
    }

    /** Allowlist-based path sanitization — see Security Patterns. */
    @JvmStatic
    fun sanitizePath(path: String?): String {
        val p = requireNonEmpty(path, "path")
        if (p.contains("..")) throw IllegalArgumentException("Path must not contain '..'")
        if (!Regex("""^[a-zA-Z0-9][a-zA-Z0-9/_\-.+]*$""").matches(p) && p != ".")
            throw IllegalArgumentException("Path contains disallowed characters")
        return p
    }
}
```

## Architecture: vars/ vs src/

| | `vars/` (Groovy) | `src/` (Kotlin) |
|---|------------------|------------------|
| **Calls Jenkins DSL** | Yes (`sh`, `stash`, `withCredentials`) | **No** — returns data |
| **Contains logic** | No (no `switch`, no string building) | Yes (decisions, commands, contracts) |
| **CPS-transformed** | Yes | No |
| **Type checking** | None (Groovy `def`) | Full (Kotlin compiler) |
| **Testable** | JenkinsPipelineUnit (Layer 2) | Plain Spock (Layer 1) |
| **Implements Serializable** | N/A | Required (with `serialVersionUID`) |

## Build Contract Pattern

A build contract formally declares what a module build does (commands,
expected artifacts, test results, stash definitions) **before** execution,
and verification compares it against actual workspace contents afterward.
Modelled in Kotlin:

```kotlin
enum class BuildPhase : Serializable {
    COMPILE, TEST, SCA, PACKAGE;

    fun displayName(): String = when (this) {
        SCA -> "SCA Scan"
        else -> name.lowercase().replaceFirstChar { it.uppercase() }
    }
    fun hookKey(): String = name.lowercase()
}

class PhaseContract(
    val phase: BuildPhase,
    val command: String,
    val artifacts: List<ArtifactExpectation>,
    val testResults: TestResultExpectation?,
    val stashDefinition: StashDefinition?,
) : Serializable {
    fun hasWork(): Boolean = command.isNotBlank()
}

class BuildContract(
    val moduleName: String,
    val moduleType: ModuleType,
    val agentImage: String,
    val phases: List<PhaseContract>,
) : Serializable {
    private val phaseIndex = phases.associateBy { it.phase }
    fun findPhase(phase: BuildPhase): PhaseContract? = phaseIndex[phase]
}
```

Benefits:
- Repos can call `describeContract()` to inspect expectations without executing.
- Missing required artifacts mark the build `unstable` with a clear report.
- Phase overrides replace execution but verification still runs.
- Exhaustive `when` on `BuildPhase` makes a new phase a compile error
  everywhere it isn't handled.

## CPS Boundary & Transient Closure Fields

| File | CPS-transformed? | Implications |
|------|------------------|--------------|
| `vars/*.groovy` `call()` | Yes | Avoid `.each` / `.collect`; use `for` loops. Variables crossing step boundaries must be `Serializable`. |
| `src/**/*.kt` | **No** | Plain JVM bytecode. Use any Kotlin idiom. Still implement `Serializable` if instances cross step boundaries. |
| `src/**/*.groovy` (legacy) | No | Same serialization rules as Kotlin; you lose type safety. |

Groovy `Closure` is **not** Java-serializable. If a `Serializable` Kotlin
class stores a `Map<String, Closure<*>>` (hook registry, callback map),
Jenkins throws `NotSerializableException` at the next CPS checkpoint:

```kotlin
class PipelineConfig(overrides: Map<String, Any?>?) : Serializable {
    // @field:Transient targets the JVM field, not the getter.
    // No @JvmField — let the getter be synthesized so callers route through
    // the typed accessor below.
    @field:Transient val phaseOverrides: Map<String, Closure<*>>?

    fun phaseOverride(moduleName: String, phase: BuildPhase): Closure<*>? {
        val po = phaseOverrides ?: return null   // null after CPS deserialization
        return po["$moduleName:${phase.hookKey()}"] ?: po[phase.hookKey()]
    }

    companion object { private const val serialVersionUID: Long = 1L }
}
```

After CPS deserialization the field is `null` — the typed accessor handles it
once, so callers never see the transient mechanic.

## Loading Libraries

### Annotation (top of Jenkinsfile)
```groovy
@Library('my-lib@v1.2.3') _              // pinned version
@Library(['lib-a@v1.0', 'lib-b@v2.0']) _ // multiple libraries
```

### Declarative `libraries` block
```groovy
pipeline {
    libraries {
        lib('my-lib@v1.2.3')
    }
}
```

### Dynamic loading
```groovy
library identifier: 'my-lib@main',
        retriever: modernSCM([$class: 'GitSCMSource',
            remote: 'https://github.com/org/my-lib.git'])
```

## Build Setup (Gradle, Kotlin + Groovy)

```kotlin
// build.gradle.kts
plugins {
    groovy
    kotlin("jvm") version "1.9.22"
}

repositories { mavenCentral() }

dependencies {
    implementation(kotlin("stdlib"))
    implementation("org.codehaus.groovy:groovy-all:3.0.19")
    testImplementation("com.lesfurets:jenkins-pipeline-unit:1.9")
    testImplementation("org.spockframework:spock-core:2.3-groovy-3.0")
    testImplementation("junit:junit:4.13.2")
}

sourceSets {
    main {
        java.srcDirs("src")            // Kotlin compiler picks up .kt files here
        withConvention(GroovySourceSet::class) { groovy.srcDirs("vars") }
    }
    test {
        withConvention(GroovySourceSet::class) { groovy.srcDirs("test") }
    }
}
```

## Test Patterns

Generate two complementary sub-layers:

### Layer 1 — Pure Spock over Kotlin `src/`

No `BasePipelineTest`, no step mocking. Construct the Kotlin class directly
and assert on returned data:

```groovy
class ModuleBuilderTest extends Specification {
    def builder = new ModuleBuilder()

    def 'java build command uses gradlew'() {
        expect:
        builder.buildCommand('api', ModuleType.JAVA) == './gradlew :api:build -x test'
    }

    def 'rejects path traversal'() {
        when:  builder.buildCommand('../etc', ModuleType.JAVA)
        then:  thrown(IllegalArgumentException)
    }
}
```

Synthesized Kotlin getters mean Groovy reads `val` fields as properties; the
test reads like ordinary Groovy.

### Layer 2 — JenkinsPipelineUnit over `vars/`

A shared `PipelineSpec` base pre-registers no-op stubs for the common DSL
steps. Individual specs override only what they care about and assert on the
recorded call stack:

```groovy
class DeployToEnvTest extends PipelineSpec {
    def 'deploys with correct parameters'() {
        given:
        helper.registerAllowedMethod('withCredentials', [List, Closure], { l, body -> body() })

        when:
        pipelineTest.loadScript('vars/deployToEnv.groovy').call(env: 'staging', tag: 'v1.0')

        then:
        shCommands().any { it.contains('staging') }
    }
}
```

The split mirrors the architecture: Layer 1 proves logic is correct; Layer 2
proves `vars/` wires that logic to the right DSL steps.

## Appendix: Legacy Groovy `src/` Patterns

These patterns appear in older codebases. They work but should not be used for
new code — Kotlin is strictly better for the same job.

### Legacy: pure-Groovy `src/` class
```groovy
// src/com/myorg/ModuleBuilder.groovy — legacy; prefer Kotlin
class ModuleBuilder implements Serializable {
    private static final long serialVersionUID = 1L

    String buildCommand(String modulePath, String type) {
        Preconditions.sanitizePath(modulePath)
        switch (type) {                                  // no exhaustiveness check
            case 'java': return "./gradlew :${modulePath}:build -x test"
            case 'node': return "cd '${modulePath}' && npm ci && npm run build"
            default:     throw new IllegalArgumentException("Unknown type: ${type}")
        }
    }
}
```

### Legacy: `steps` context injection

This pattern passes the pipeline `steps` context to `src/` so the class can
call Jenkins steps directly. It works but requires mocking the Jenkins
runtime in tests (fragile, can mask real issues) and couples logic to the
runtime. **Don't introduce this in new code.**

```groovy
// src/org/myorg/Deploy.groovy — legacy
class Deploy implements Serializable {
    private static final long serialVersionUID = 1L
    def steps
    Deploy(steps) { this.steps = steps }

    def toEnvironment(String env, String tag) {
        steps.echo "Deploying ${tag} to ${env}"
        steps.sh "kubectl set image deployment/app app=${tag} -n ${env}"
    }
}
```

The Kotlin equivalent (no `steps`, returns commands) is shorter, type-safe,
and testable without any Jenkins runtime.
