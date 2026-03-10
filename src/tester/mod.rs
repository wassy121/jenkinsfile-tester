use serde::{Deserialize, Serialize};
use crate::ast::{Agent, Binding, EnvValue, Parameter, Pipeline, PipelineOption, Step};

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestSuite {
    pub name: String,
    pub tests: Vec<TestResult>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

pub fn run_tests(pipeline: &Pipeline) -> TestSuite {
    let tests = vec![
        test_pipeline_has_stages(pipeline),
        test_all_stages_named(pipeline),
        test_agent_declared(pipeline),
        test_no_placeholder_stage_names(pipeline),
        test_post_block_exists(pipeline),
        test_has_build_stage(pipeline),
        test_has_test_stage(pipeline),
        test_no_plaintext_secrets(pipeline),
        test_parallel_has_multiple_branches(pipeline),
        test_no_empty_steps_blocks(pipeline),
        test_has_build_discarder(pipeline),
        test_no_agent_any(pipeline),
        test_docker_images_pinned(pipeline),
        test_input_stages_no_agent(pipeline),
        test_when_stages_use_before_agent(pipeline),
        test_no_secret_string_parameters(pipeline),
        test_libraries_pinned_to_version(pipeline),
        test_script_blocks_are_small(pipeline),
        test_checkout_not_duplicated(pipeline),
        test_deploy_stages_disable_concurrent(pipeline),
        test_post_failure_handler_exists(pipeline),
        test_no_groovy_interpolated_credentials(pipeline),
    ];

    let passed = tests.iter().filter(|t| t.passed).count();
    let failed = tests.iter().filter(|t| !t.passed).count();

    TestSuite {
        name: "Jenkins Pipeline Structural Tests".into(),
        passed,
        failed,
        skipped: 0,
        tests,
    }
}

fn test_pipeline_has_stages(p: &Pipeline) -> TestResult {
    let passed = !p.stages.is_empty();
    TestResult {
        name: "pipeline_has_stages".into(),
        passed,
        message: if passed {
            format!("Pipeline has {} stage(s)", p.stages.len())
        } else {
            "Pipeline has no stages".into()
        },
    }
}

fn test_all_stages_named(p: &Pipeline) -> TestResult {
    let unnamed: Vec<_> = collect_all_stages(p)
        .into_iter()
        .filter(|s| s.is_empty())
        .collect();
    let passed = unnamed.is_empty();
    TestResult {
        name: "all_stages_named".into(),
        passed,
        message: if passed {
            "All stages have non-empty names".into()
        } else {
            format!("{} stage(s) have empty names", unnamed.len())
        },
    }
}

fn test_agent_declared(p: &Pipeline) -> TestResult {
    let passed = p.agent.is_some();
    TestResult {
        name: "agent_declared".into(),
        passed,
        message: if passed {
            "Pipeline-level agent is declared".into()
        } else {
            "Pipeline has no top-level agent declaration".into()
        },
    }
}

fn test_no_placeholder_stage_names(p: &Pipeline) -> TestResult {
    let placeholders = ["TODO", "FIXME", "placeholder", "stage", "untitled", "new stage"];
    let bad: Vec<String> = collect_all_stages(p)
        .into_iter()
        .filter(|name| {
            let lower = name.to_lowercase();
            placeholders.iter().any(|ph| lower == ph.to_lowercase())
        })
        .collect();
    let passed = bad.is_empty();
    TestResult {
        name: "no_placeholder_stage_names".into(),
        passed,
        message: if passed {
            "No placeholder stage names found".into()
        } else {
            format!("Placeholder stage names found: {:?}", bad)
        },
    }
}

fn test_post_block_exists(p: &Pipeline) -> TestResult {
    let passed = p.post.is_some();
    TestResult {
        name: "post_block_exists".into(),
        passed,
        message: if passed {
            "Pipeline has a post block".into()
        } else {
            "Pipeline is missing a post block".into()
        },
    }
}

fn test_has_build_stage(p: &Pipeline) -> TestResult {
    let names = collect_all_stages(p);
    let passed = names.iter().any(|n| {
        let lower = n.to_lowercase();
        lower.contains("build") || lower.contains("compile")
    });
    TestResult {
        name: "has_build_stage".into(),
        passed,
        message: if passed {
            "Found a build/compile stage".into()
        } else {
            "No build or compile stage found".into()
        },
    }
}

fn test_has_test_stage(p: &Pipeline) -> TestResult {
    let names = collect_all_stages(p);
    let passed = names.iter().any(|n| {
        let lower = n.to_lowercase();
        lower.contains("test") || lower.contains("spec") || lower.contains("check")
    });
    TestResult {
        name: "has_test_stage".into(),
        passed,
        message: if passed {
            "Found a test/spec/check stage".into()
        } else {
            "No test, spec, or check stage found".into()
        },
    }
}

fn has_sensitive_word(var_name: &str) -> bool {
    let keywords = ["PASSWORD", "SECRET", "TOKEN", "API_KEY", "APIKEY", "PASSWD", "CREDENTIAL", "CRED", "KEY"];
    let parts: Vec<&str> = var_name.split('_').collect();
    parts.iter().any(|p| {
        let upper = p.to_uppercase();
        keywords.contains(&upper.as_str())
    })
}

fn looks_like_secret_value(value: &str) -> bool {
    // GitHub tokens
    if value.starts_with("ghp_") || value.starts_with("ghs_") || value.starts_with("gho_") || value.starts_with("github_pat_") {
        return true;
    }
    // AWS access key ID
    if value.starts_with("AKIA") {
        return true;
    }
    // JWT format: three base64url segments separated by dots, each segment at least 8 chars
    // (guards against semver strings like "1.0.0" which also have three dot-separated parts)
    let parts: Vec<&str> = value.splitn(4, '.').collect();
    if parts.len() == 3
        && parts.iter().all(|p| p.len() >= 8 && p.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '='))
    {
        return true;
    }
    // Long hex string (32+ chars of [0-9a-f])
    if value.len() >= 32 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    false
}

fn test_no_plaintext_secrets(p: &Pipeline) -> TestResult {
    let mut found = Vec::new();

    for var in &p.environment {
        // Skip vars backed by credentials() helper — proper credentials binding is safe
        if matches!(var.value, EnvValue::Credentials { .. }) {
            continue;
        }
        // Name-based check: split on _ and check word components
        if has_sensitive_word(&var.key) {
            found.push(format!("{} (name pattern)", var.key));
            continue;
        }
        // Value-based heuristic checks (only for Literal values)
        if let EnvValue::Literal(ref s) = var.value {
            if looks_like_secret_value(s) {
                found.push(format!("{} (secret-shaped value)", var.key));
            }
        }
    }

    let passed = found.is_empty();
    TestResult {
        name: "no_plaintext_secrets".into(),
        passed,
        message: if passed {
            "No plaintext secrets detected in environment".into()
        } else {
            format!("Possible plaintext secrets in environment: {:?}", found)
        },
    }
}

fn test_parallel_has_multiple_branches(p: &Pipeline) -> TestResult {
    use crate::ast::StageBody;

    let all_stages = crate::ast::walk::collect_all_stages(&p.stages);
    let parallel_stages: Vec<_> = all_stages.iter()
        .filter(|s| matches!(&s.body, StageBody::Parallel { .. }))
        .collect();

    if parallel_stages.is_empty() {
        return TestResult {
            name: "parallel_has_multiple_branches".into(),
            passed: true,
            message: "No parallel blocks found (skipped)".into(),
        };
    }

    let bad: Vec<_> = parallel_stages.iter()
        .filter(|s| match &s.body {
            StageBody::Parallel { stages: branches } => branches.len() < 2,
            _ => false,
        })
        .map(|s| s.name.as_str())
        .collect();

    let passed = bad.is_empty();
    TestResult {
        name: "parallel_has_multiple_branches".into(),
        passed,
        message: if passed {
            "All parallel blocks have 2+ branches".into()
        } else {
            format!("Parallel block(s) with fewer than 2 branches: {:?}", bad)
        },
    }
}

fn test_no_empty_steps_blocks(p: &Pipeline) -> TestResult {
    let mut empty = Vec::new();
    for stage in &p.stages {
        collect_empty_steps(stage, &mut empty);
    }
    let passed = empty.is_empty();
    TestResult {
        name: "no_empty_steps_blocks".into(),
        passed,
        message: if passed {
            "No empty steps blocks found".into()
        } else {
            format!("Stages with empty steps: {:?}", empty)
        },
    }
}

fn collect_empty_steps(stage: &crate::ast::Stage, out: &mut Vec<String>) {
    use crate::ast::StageBody;
    match &stage.body {
        StageBody::Steps(steps) => {
            if steps.steps.is_empty() {
                out.push(stage.name.clone());
            }
        }
        StageBody::Parallel { stages: branches } => {
            for s in branches { collect_empty_steps(s, out); }
        }
        StageBody::Sequential { stages: nested } => {
            for s in nested { collect_empty_steps(s, out); }
        }
        StageBody::Matrix(matrix) => {
            for s in &matrix.stages { collect_empty_steps(s, out); }
        }
    }
}

fn collect_all_stages(p: &Pipeline) -> Vec<String> {
    crate::ast::walk::collect_all_stages(&p.stages)
        .into_iter()
        .map(|s| s.name.clone())
        .collect()
}

// ── Helpers for new tests ─────────────────────────────────────────────────────

/// Returns true if the Docker image has no explicit tag, or uses a mutable tag.
fn is_unpinned_image(image: &str) -> bool {
    image.rsplit_once(':')
        .map(|(_, tag)| matches!(tag, "latest" | "stable" | ""))
        .unwrap_or(true)
}

/// Collects unpinned Docker image strings from an optional agent into `out`.
fn collect_unpinned_docker_images(agent: &Option<Agent>, out: &mut Vec<String>) {
    if let Some(Agent::Docker { image, .. }) = agent {
        if is_unpinned_image(image) {
            out.push(image.clone());
        }
    }
}

// ── New structural tests ──────────────────────────────────────────────────────

fn test_has_build_discarder(p: &Pipeline) -> TestResult {
    let passed = p.options.iter().any(|o| matches!(o, PipelineOption::BuildDiscarder { .. }));
    TestResult {
        name: "has_build_discarder".into(),
        passed,
        message: if passed {
            "buildDiscarder(logRotator(...)) is configured — builds will not accumulate indefinitely".into()
        } else {
            "No buildDiscarder configured — builds accumulate indefinitely on the controller, causing disk exhaustion. Add options { buildDiscarder(logRotator(numToKeepStr: '10')) }".into()
        },
    }
}

fn test_no_agent_any(p: &Pipeline) -> TestResult {
    let passed = !matches!(p.agent, Some(Agent::Any));
    TestResult {
        name: "no_agent_any".into(),
        passed,
        message: if passed {
            "Pipeline does not use 'agent any' — execution environment is explicitly specified".into()
        } else {
            "'agent any' allows Jenkins to assign the build to any executor including the controller — use a specific label, docker image, or kubernetes agent instead".into()
        },
    }
}

fn test_docker_images_pinned(p: &Pipeline) -> TestResult {
    let mut unpinned: Vec<String> = Vec::new();

    collect_unpinned_docker_images(&p.agent, &mut unpinned);
    for stage in crate::ast::walk::collect_all_stages(&p.stages) {
        collect_unpinned_docker_images(&stage.agent, &mut unpinned);
    }

    if unpinned.is_empty() {
        // Determine whether any Docker agents exist at all
        let has_docker = matches!(&p.agent, Some(Agent::Docker { .. }))
            || crate::ast::walk::collect_all_stages(&p.stages)
                .iter()
                .any(|s| matches!(&s.agent, Some(Agent::Docker { .. })));

        let message = if has_docker {
            "All Docker agent images are pinned to explicit version tags".into()
        } else {
            "No Docker agents declared (skipped)".into()
        };
        return TestResult { name: "docker_images_pinned".into(), passed: true, message };
    }

    TestResult {
        name: "docker_images_pinned".into(),
        passed: false,
        message: format!(
            "Docker images using mutable or absent tags — non-reproducible builds. Pin to a specific version or digest (e.g. 'node:20.11.0' or '@sha256:...'): {:?}",
            unpinned
        ),
    }
}

fn test_input_stages_no_agent(p: &Pipeline) -> TestResult {
    let all = crate::ast::walk::collect_all_stages(&p.stages);
    let input_stages: Vec<_> = all.iter().filter(|s| s.input.is_some()).collect();

    if input_stages.is_empty() {
        return TestResult {
            name: "input_stages_no_agent".into(),
            passed: true,
            message: "No stages with input directives (skipped)".into(),
        };
    }

    let bad: Vec<String> = input_stages
        .iter()
        .filter(|s| matches!(&s.agent, Some(a) if !matches!(a, Agent::None)))
        .map(|s| s.name.clone())
        .collect();

    let passed = bad.is_empty();
    TestResult {
        name: "input_stages_no_agent".into(),
        passed,
        message: if passed {
            "All input stages run without a dedicated agent — executors are not held during approval windows".into()
        } else {
            format!(
                "Stages with input directives that also allocate an agent — the executor and workspace will be held for the entire human approval duration: {:?}",
                bad
            )
        },
    }
}

fn test_when_stages_use_before_agent(p: &Pipeline) -> TestResult {
    let all = crate::ast::walk::collect_all_stages(&p.stages);

    let candidates: Vec<_> = all
        .iter()
        .filter(|s| {
            s.when.is_some()
                && matches!(&s.agent, Some(a) if !matches!(a, Agent::None))
        })
        .collect();

    if candidates.is_empty() {
        return TestResult {
            name: "when_stages_use_before_agent".into(),
            passed: true,
            message: "No stages combine a 'when' condition with a stage-level agent (skipped)".into(),
        };
    }

    let bad: Vec<String> = candidates
        .iter()
        .filter(|s| s.when.as_ref().map(|w| !w.before_agent).unwrap_or(false))
        .map(|s| s.name.clone())
        .collect();

    let passed = bad.is_empty();
    TestResult {
        name: "when_stages_use_before_agent".into(),
        passed,
        message: if passed {
            "All conditional stages with agents use 'beforeAgent true' — agents are not allocated for skipped stages".into()
        } else {
            format!(
                "Stages with 'when' + 'agent' that are missing 'beforeAgent true' — Jenkins allocates the agent before evaluating the condition, wasting executors on every skipped run: {:?}",
                bad
            )
        },
    }
}

fn test_no_secret_string_parameters(p: &Pipeline) -> TestResult {
    const KEYWORDS: &[&str] = &[
        "password", "secret", "token", "key", "credential", "cred", "auth", "passwd", "apikey",
    ];

    if p.parameters.is_empty() {
        return TestResult {
            name: "no_secret_string_parameters".into(),
            passed: true,
            message: "No parameters declared (skipped)".into(),
        };
    }

    let bad: Vec<String> = p.parameters
        .iter()
        .filter_map(|param| match param {
            Parameter::String { name, .. } | Parameter::Text { name, .. } => Some(name),
            _ => None,
        })
        .filter(|name| {
            name.to_lowercase()
                .split(|c: char| c == '_' || c == '-')
                .any(|part| KEYWORDS.contains(&part))
        })
        .cloned()
        .collect();

    let passed = bad.is_empty();
    TestResult {
        name: "no_secret_string_parameters".into(),
        passed,
        message: if passed {
            "No string/text parameters found with credential-like names".into()
        } else {
            format!(
                "String or text parameters with sensitive names expose values in the Jenkins UI and build history — use the 'password' parameter type instead: {:?}",
                bad
            )
        },
    }
}

fn test_libraries_pinned_to_version(p: &Pipeline) -> TestResult {
    const MUTABLE: &[&str] = &["master", "main", "develop", "HEAD", "trunk"];

    if p.libraries.is_empty() {
        return TestResult {
            name: "libraries_pinned_to_version".into(),
            passed: true,
            message: "No shared libraries declared (skipped)".into(),
        };
    }

    let bad: Vec<String> = p.libraries
        .iter()
        .filter_map(|lib| match &lib.ref_ {
            None => Some(format!("{} (no version)", lib.name)),
            Some(r) if MUTABLE.contains(&r.as_str()) => Some(format!("{}@{}", lib.name, r)),
            _ => None,
        })
        .collect();

    let passed = bad.is_empty();
    TestResult {
        name: "libraries_pinned_to_version".into(),
        passed,
        message: if passed {
            "All shared libraries are pinned to a specific version tag or commit SHA".into()
        } else {
            format!(
                "Shared libraries pinned to mutable branches — a library change can silently break this pipeline overnight. Pin to a tag or SHA: {:?}",
                bad
            )
        },
    }
}

fn test_script_blocks_are_small(p: &Pipeline) -> TestResult {
    const MAX_LINES: usize = 15;

    let all_stages = crate::ast::walk::collect_all_stages(&p.stages);
    let mut violations: Vec<String> = Vec::new();
    let mut found_any = false;

    for stage in &all_stages {
        if let Some(steps_block) = crate::ast::walk::stage_steps(stage) {
            for step in crate::ast::walk::collect_all_steps_recursive(&steps_block.steps) {
                if let Step::Script { body } = step {
                    found_any = true;
                    let count = body.lines().filter(|l| !l.trim().is_empty()).count();
                    if count > MAX_LINES {
                        violations.push(format!("stage '{}': {} lines", stage.name, count));
                    }
                }
            }
        }
    }

    if !found_any {
        return TestResult {
            name: "script_blocks_are_small".into(),
            passed: true,
            message: "No script blocks found (skipped)".into(),
        };
    }

    let passed = violations.is_empty();
    TestResult {
        name: "script_blocks_are_small".into(),
        passed,
        message: if passed {
            "All script blocks are within the 15-line limit".into()
        } else {
            format!(
                "Large script blocks execute on the Jenkins controller and can cause GC pressure and instability — move complex logic to shared library steps: {:?}",
                violations
            )
        },
    }
}

fn test_checkout_not_duplicated(p: &Pipeline) -> TestResult {
    let count = crate::ast::walk::collect_all_stages(&p.stages)
        .iter()
        .flat_map(|stage| {
            crate::ast::walk::stage_steps(stage)
                .map(|s| crate::ast::walk::collect_all_steps_recursive(&s.steps))
                .unwrap_or_default()
        })
        .filter(|step| matches!(step, Step::Checkout { .. }))
        .count();

    let (passed, message) = match count {
        0 => (true, "No explicit checkout steps found (using implicit checkout)".into()),
        1 => (true, "Source checkout happens exactly once — correct".into()),
        n => (
            false,
            format!(
                "Source checkout occurs {} times — re-checking out source wastes bandwidth and time. Use stash/unstash to share files between agents instead",
                n
            ),
        ),
    };

    TestResult { name: "checkout_not_duplicated".into(), passed, message }
}

fn test_deploy_stages_disable_concurrent(p: &Pipeline) -> TestResult {
    let deploy_keywords = ["deploy", "release", "publish", "promote"];

    let deploy_stages: Vec<String> = crate::ast::walk::collect_all_stages(&p.stages)
        .iter()
        .filter(|s| {
            let lower = s.name.to_lowercase();
            deploy_keywords.iter().any(|kw| lower.contains(kw))
        })
        .map(|s| s.name.clone())
        .collect();

    if deploy_stages.is_empty() {
        return TestResult {
            name: "deploy_stages_disable_concurrent".into(),
            passed: true,
            message: "No deployment stages found (skipped)".into(),
        };
    }

    let has_option = p.options.iter().any(|o| matches!(o, PipelineOption::DisableConcurrentBuilds { .. }));
    TestResult {
        name: "deploy_stages_disable_concurrent".into(),
        passed: has_option,
        message: if has_option {
            "disableConcurrentBuilds() is configured — concurrent deployments to the same environment are prevented".into()
        } else {
            format!(
                "Pipeline has deployment stages ({:?}) without 'disableConcurrentBuilds()' — concurrent runs can cause race conditions, double-deploys, and environment corruption",
                deploy_stages
            )
        },
    }
}

fn test_post_failure_handler_exists(p: &Pipeline) -> TestResult {
    let passed = p.post.as_ref().map(|post| {
        post.failure.is_some() || post.unsuccessful.is_some() || post.always.is_some()
    }).unwrap_or(false);

    TestResult {
        name: "post_failure_handler_exists".into(),
        passed,
        message: if passed {
            "Pipeline post block handles failure outcomes — notifications and cleanup will run on build failure".into()
        } else {
            "No post block handling failure outcomes — failed builds produce no notification or cleanup. Add 'post { failure { ... } }' or 'post { always { ... } }'".into()
        },
    }
}

/// Derive the variable names that Jenkins injects for `credentials()` bindings in
/// `environment {}` blocks.  For each `EnvValue::Credentials` entry with key `K`,
/// Jenkins creates `K_USR` and `K_PSW`.
fn collect_env_credential_var_names(env: &[crate::ast::EnvVar]) -> Vec<String> {
    let mut vars = Vec::new();
    for var in env {
        if matches!(var.value, crate::ast::EnvValue::Credentials { .. }) {
            vars.push(format!("{}_USR", var.key));
            vars.push(format!("{}_PSW", var.key));
        }
    }
    vars
}

fn collect_binding_var_names(bindings: &[Binding]) -> Vec<String> {
    let mut vars = Vec::new();
    for binding in bindings {
        match binding {
            Binding::UsernamePassword { username_variable, password_variable, .. } => {
                vars.push(username_variable.clone());
                vars.push(password_variable.clone());
            }
            Binding::StringBinding { variable, .. } | Binding::FileBinding { variable, .. } => {
                vars.push(variable.clone());
            }
            Binding::SshUserPrivateKey { key_file_variable, passphrase_variable, .. } => {
                vars.push(key_file_variable.clone());
                if let Some(pv) = passphrase_variable {
                    vars.push(pv.clone());
                }
            }
            Binding::Certificate { keystore_variable, password_variable, .. } => {
                vars.push(keystore_variable.clone());
                if let Some(pv) = password_variable {
                    vars.push(pv.clone());
                }
            }
            Binding::Raw(_) => {}
        }
    }
    vars
}

fn test_no_groovy_interpolated_credentials(p: &Pipeline) -> TestResult {
    use crate::ast::walk::{collect_all_stages, stage_steps, collect_all_steps_recursive};

    let mut violations: Vec<String> = Vec::new();

    // Credential variable names derived from environment{} blocks (pipeline-level + stage-level).
    // Jenkins injects KEY_USR and KEY_PSW for every `KEY = credentials(...)` binding.
    let pipeline_env_vars = collect_env_credential_var_names(&p.environment);

    for stage in collect_all_stages(&p.stages) {
        let Some(steps_block) = stage_steps(stage) else { continue };

        // Merge pipeline-level env vars with any stage-level env credential vars
        let mut env_vars = pipeline_env_vars.clone();
        env_vars.extend(collect_env_credential_var_names(&stage.environment));

        for step in &steps_block.steps {
            // Case 1: withCredentials block — check inner steps against binding vars
            if let Step::WithCredentials { bindings, steps: inner_steps } = step {
                let cred_vars = collect_binding_var_names(bindings);
                for inner_step in collect_all_steps_recursive(inner_steps) {
                    let (text, step_name) = match inner_step {
                        Step::Sh { script, is_double_quoted: true, .. } => (script.as_str(), "sh"),
                        Step::Echo { message, is_double_quoted: true } => (message.as_str(), "echo"),
                        _ => continue,
                    };
                    for var in &cred_vars {
                        if text.contains(&format!("${{{}}}", var))
                            || text.contains(&format!("${}", var))
                        {
                            violations.push(format!(
                                "variable '{}' in '{}' step in stage '{}'",
                                var, step_name, stage.name
                            ));
                        }
                    }
                }
            }
            // Case 2: any double-quoted sh/echo step referencing env-block credential vars
            if !env_vars.is_empty() {
                let (text, step_name) = match step {
                    Step::Sh { script, is_double_quoted: true, .. } => (script.as_str(), "sh"),
                    Step::Echo { message, is_double_quoted: true } => (message.as_str(), "echo"),
                    _ => continue,
                };
                for var in &env_vars {
                    if text.contains(&format!("${{{}}}", var))
                        || text.contains(&format!("${}", var))
                    {
                        violations.push(format!(
                            "variable '{}' in '{}' step in stage '{}'",
                            var, step_name, stage.name
                        ));
                    }
                }
            }
        }
    }

    TestResult {
        name: "no_groovy_interpolated_credentials".into(),
        passed: violations.is_empty(),
        message: if violations.is_empty() {
            "No credential variables found in double-quoted sh/echo strings (Groovy interpolation)".into()
        } else {
            format!(
                "Credential variables passed to 'sh' or 'echo' via Groovy string interpolation — \
                 secrets are exposed and bypass masking. \
                 Use single-quoted strings instead: {:?}",
                violations
            )
        },
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    // ── AST builder helpers ───────────────────────────────────────────────────

    fn make_pipeline(agent: Option<Agent>, stages: Vec<Stage>, post: Option<Post>) -> Pipeline {
        Pipeline {
            agent,
            environment: vec![],
            options: vec![],
            parameters: vec![],
            triggers: vec![],
            tools: vec![],
            stages,
            post,
            duplicate_sections: vec![],
            libraries: vec![],
        }
    }

    fn named_stage(name: &str) -> Stage {
        Stage {
            name: name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Echo { message: "ok".into(), is_double_quoted: false }] }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn empty_steps_stage(name: &str) -> Stage {
        Stage {
            name: name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![] }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn parallel_stage(name: &str, branches: Vec<Stage>) -> Stage {
        Stage {
            name: name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: branches },
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn post_always() -> Post {
        Post {
            always: Some(Steps { steps: vec![] }),
            success: None, failure: None, unstable: None, aborted: None,
            changed: None, cleanup: None, regression: None, fixed: None,
            unsuccessful: None,
        }
    }

    fn find_test<'a>(suite: &'a TestSuite, name: &str) -> &'a TestResult {
        suite.tests.iter().find(|t| t.name == name)
            .unwrap_or_else(|| panic!("test '{}' not found in suite", name))
    }

    // ── test_pipeline_has_stages ──────────────────────────────────────────────

    #[test]
    fn pipeline_has_stages_passes_when_stages_present() {
        let p = make_pipeline(Some(Agent::Any), vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        assert!(find_test(&suite, "pipeline_has_stages").passed);
    }

    #[test]
    fn pipeline_has_stages_fails_when_no_stages() {
        let p = make_pipeline(Some(Agent::Any), vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "pipeline_has_stages");
        assert!(!t.passed);
        assert!(t.message.contains("no stages"));
    }

    #[test]
    fn pipeline_has_stages_message_includes_count() {
        let p = make_pipeline(Some(Agent::Any), vec![named_stage("A"), named_stage("B")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "pipeline_has_stages");
        assert!(t.passed);
        assert!(t.message.contains('2'));
    }

    // ── test_all_stages_named ─────────────────────────────────────────────────

    #[test]
    fn all_stages_named_passes_for_non_empty_names() {
        let p = make_pipeline(None, vec![named_stage("Build"), named_stage("Test")], None);
        let suite = run_tests(&p);
        assert!(find_test(&suite, "all_stages_named").passed);
    }

    #[test]
    fn all_stages_named_fails_for_empty_name() {
        let p = make_pipeline(None, vec![named_stage("")], None);
        let suite = run_tests(&p);
        assert!(!find_test(&suite, "all_stages_named").passed);
    }

    // ── test_agent_declared ───────────────────────────────────────────────────

    #[test]
    fn agent_declared_passes_when_agent_any() {
        let p = make_pipeline(Some(Agent::Any), vec![], None);
        assert!(find_test(&run_tests(&p), "agent_declared").passed);
    }

    #[test]
    fn agent_declared_passes_when_agent_none() {
        // `agent none` is a valid explicit declaration (no execution host required)
        let p = make_pipeline(Some(Agent::None), vec![], None);
        assert!(find_test(&run_tests(&p), "agent_declared").passed);
    }

    #[test]
    fn agent_declared_passes_when_agent_label() {
        let p = make_pipeline(Some(Agent::Label("linux".into())), vec![], None);
        assert!(find_test(&run_tests(&p), "agent_declared").passed);
    }

    #[test]
    fn agent_declared_fails_when_no_agent() {
        let p = make_pipeline(None, vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "agent_declared");
        assert!(!t.passed);
        assert!(t.message.contains("no top-level agent"));
    }

    // ── test_no_placeholder_stage_names ──────────────────────────────────────

    #[test]
    fn no_placeholder_names_passes_for_real_names() {
        let p = make_pipeline(None, vec![named_stage("Build"), named_stage("Deploy")], None);
        assert!(find_test(&run_tests(&p), "no_placeholder_stage_names").passed);
    }

    #[test]
    fn no_placeholder_names_fails_for_todo() {
        let p = make_pipeline(None, vec![named_stage("TODO")], None);
        assert!(!find_test(&run_tests(&p), "no_placeholder_stage_names").passed);
    }

    #[test]
    fn no_placeholder_names_fails_for_fixme() {
        let p = make_pipeline(None, vec![named_stage("FIXME")], None);
        assert!(!find_test(&run_tests(&p), "no_placeholder_stage_names").passed);
    }

    #[test]
    fn no_placeholder_names_fails_for_bare_stage() {
        // A developer leaving a stage named literally "stage" is a placeholder.
        let p = make_pipeline(None, vec![named_stage("stage")], None);
        assert!(!find_test(&run_tests(&p), "no_placeholder_stage_names").passed);
    }

    #[test]
    fn no_placeholder_names_is_case_insensitive() {
        let p = make_pipeline(None, vec![named_stage("todo")], None);
        assert!(!find_test(&run_tests(&p), "no_placeholder_stage_names").passed);
    }

    /// "Stagecoach" contains "stage" as a substring but is not an exact match.
    #[test]
    fn no_placeholder_names_does_not_flag_substring_matches() {
        let p = make_pipeline(None, vec![named_stage("Stagecoach")], None);
        assert!(find_test(&run_tests(&p), "no_placeholder_stage_names").passed);
    }

    // ── test_post_block_exists ────────────────────────────────────────────────

    #[test]
    fn post_block_exists_passes_when_post_present() {
        let p = make_pipeline(None, vec![], Some(post_always()));
        assert!(find_test(&run_tests(&p), "post_block_exists").passed);
    }

    #[test]
    fn post_block_exists_fails_when_no_post() {
        let p = make_pipeline(None, vec![], None);
        assert!(!find_test(&run_tests(&p), "post_block_exists").passed);
    }

    // ── test_has_build_stage ──────────────────────────────────────────────────

    #[test]
    fn has_build_stage_passes_for_stage_named_build() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        assert!(find_test(&run_tests(&p), "has_build_stage").passed);
    }

    #[test]
    fn has_build_stage_passes_for_stage_named_compile() {
        let p = make_pipeline(None, vec![named_stage("Compile Sources")], None);
        assert!(find_test(&run_tests(&p), "has_build_stage").passed);
    }

    #[test]
    fn has_build_stage_is_case_insensitive() {
        let p = make_pipeline(None, vec![named_stage("BUILD DOCKER IMAGE")], None);
        assert!(find_test(&run_tests(&p), "has_build_stage").passed);
    }

    #[test]
    fn has_build_stage_fails_when_only_test_stage_present() {
        let p = make_pipeline(None, vec![named_stage("Test")], None);
        assert!(!find_test(&run_tests(&p), "has_build_stage").passed);
    }

    // ── test_has_test_stage ───────────────────────────────────────────────────

    #[test]
    fn has_test_stage_passes_for_stage_named_test() {
        let p = make_pipeline(None, vec![named_stage("Test")], None);
        assert!(find_test(&run_tests(&p), "has_test_stage").passed);
    }

    #[test]
    fn has_test_stage_passes_for_stage_named_spec() {
        let p = make_pipeline(None, vec![named_stage("Run Specs")], None);
        assert!(find_test(&run_tests(&p), "has_test_stage").passed);
    }

    #[test]
    fn has_test_stage_passes_for_stage_named_check() {
        let p = make_pipeline(None, vec![named_stage("Quality Check")], None);
        assert!(find_test(&run_tests(&p), "has_test_stage").passed);
    }

    #[test]
    fn has_test_stage_fails_when_no_test_like_stage() {
        let p = make_pipeline(None, vec![named_stage("Build"), named_stage("Deploy")], None);
        assert!(!find_test(&run_tests(&p), "has_test_stage").passed);
    }

    // ── test_no_plaintext_secrets ─────────────────────────────────────────────

    #[test]
    fn no_plaintext_secrets_passes_when_secret_uses_credentials_helper() {
        let mut p = make_pipeline(None, vec![], None);
        p.environment.push(EnvVar {
            key: "DB_PASSWORD".into(),
            value: EnvValue::Credentials { id: "db-secret".into() },
        });
        assert!(find_test(&run_tests(&p), "no_plaintext_secrets").passed);
    }

    #[test]
    fn no_plaintext_secrets_fails_when_password_is_literal() {
        let mut p = make_pipeline(None, vec![], None);
        p.environment.push(EnvVar {
            key: "DB_PASSWORD".into(),
            value: EnvValue::Literal("hunter2".into()), // plaintext!
        });
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_plaintext_secrets");
        assert!(!t.passed);
        assert!(t.message.contains("DB_PASSWORD"));
    }

    #[test]
    fn no_plaintext_secrets_fails_for_token_key() {
        let mut p = make_pipeline(None, vec![], None);
        p.environment.push(EnvVar {
            key: "API_TOKEN".into(),
            value: EnvValue::Literal("abc123".into()),
        });
        assert!(!find_test(&run_tests(&p), "no_plaintext_secrets").passed);
    }

    #[test]
    fn no_plaintext_secrets_passes_for_unrelated_env_var() {
        let mut p = make_pipeline(None, vec![], None);
        p.environment.push(EnvVar {
            key: "APP_NAME".into(),
            value: EnvValue::Literal("my-service".into()),
        });
        assert!(find_test(&run_tests(&p), "no_plaintext_secrets").passed);
    }

    // ── test_parallel_has_multiple_branches ───────────────────────────────────

    #[test]
    fn parallel_multiple_branches_passes_when_no_parallel() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "parallel_has_multiple_branches");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn parallel_multiple_branches_passes_for_two_branches() {
        let p = make_pipeline(
            None,
            vec![parallel_stage("Test", vec![named_stage("Unit"), named_stage("Integration")])],
            None,
        );
        assert!(find_test(&run_tests(&p), "parallel_has_multiple_branches").passed);
    }

    #[test]
    fn parallel_multiple_branches_fails_for_single_branch() {
        // A parallel block with only one branch provides no parallelism benefit.
        let p = make_pipeline(
            None,
            vec![parallel_stage("Test", vec![named_stage("Unit")])],
            None,
        );
        let suite = run_tests(&p);
        let t = find_test(&suite, "parallel_has_multiple_branches");
        assert!(!t.passed);
        assert!(t.message.contains("fewer than 2"));
    }

    // ── test_no_empty_steps_blocks ────────────────────────────────────────────

    #[test]
    fn no_empty_steps_passes_for_non_empty_steps() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        assert!(find_test(&run_tests(&p), "no_empty_steps_blocks").passed);
    }

    #[test]
    fn no_empty_steps_fails_when_stage_has_empty_steps() {
        let p = make_pipeline(None, vec![empty_steps_stage("Lint")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_empty_steps_blocks");
        assert!(!t.passed);
        assert!(t.message.contains("Lint"));
    }

    // ── helper: stage with agent ──────────────────────────────────────────────

    fn stage_with_agent(name: &str, agent: Agent) -> Stage {
        Stage {
            name: name.to_string(),
            location: None,
            agent: Some(agent),
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Echo { message: "ok".into(), is_double_quoted: false }] }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn stage_with_input(name: &str, agent: Option<Agent>) -> Stage {
        use crate::ast::StageInput;
        Stage {
            name: name.to_string(),
            location: None,
            agent,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: Some(StageInput {
                message: "Approve?".into(),
                ok: None,
                submitter: None,
                submitter_parameter: None,
                parameters: vec![],
            }),
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Echo { message: "ok".into(), is_double_quoted: false }] }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn stage_with_when(name: &str, agent: Option<Agent>, before_agent: bool) -> Stage {
        use crate::ast::{When, WhenCondition};
        Stage {
            name: name.to_string(),
            location: None,
            agent,
            environment: vec![],
            when: Some(When {
                conditions: vec![WhenCondition::Branch { pattern: "main".into() }],
                before_agent,
                before_input: false,
                before_options: false,
            }),
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Echo { message: "ok".into(), is_double_quoted: false }] }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn stage_with_script(name: &str, lines: usize) -> Stage {
        let body = (0..lines).map(|i| format!("echo {}", i)).collect::<Vec<_>>().join("\n");
        Stage {
            name: name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Script { body }] }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    fn post_failure() -> Post {
        Post {
            always: None,
            success: None,
            failure: Some(Steps { steps: vec![] }),
            unstable: None,
            aborted: None,
            changed: None,
            cleanup: None,
            regression: None,
            fixed: None,
            unsuccessful: None,
        }
    }

    // ── test_has_build_discarder ──────────────────────────────────────────────

    #[test]
    fn has_build_discarder_passes_when_configured() {
        let mut p = make_pipeline(None, vec![], None);
        p.options.push(PipelineOption::BuildDiscarder {
            num_to_keep: Some("10".into()),
            days_to_keep: None,
            artifact_num_to_keep: None,
            artifact_days_to_keep: None,
            raw: None,
        });
        assert!(find_test(&run_tests(&p), "has_build_discarder").passed);
    }

    #[test]
    fn has_build_discarder_fails_when_absent() {
        let p = make_pipeline(None, vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "has_build_discarder");
        assert!(!t.passed);
        assert!(t.message.contains("No buildDiscarder"));
    }

    // ── test_no_agent_any ─────────────────────────────────────────────────────

    #[test]
    fn no_agent_any_passes_for_label_agent() {
        let p = make_pipeline(Some(Agent::Label("linux".into())), vec![], None);
        assert!(find_test(&run_tests(&p), "no_agent_any").passed);
    }

    #[test]
    fn no_agent_any_fails_for_agent_any() {
        let p = make_pipeline(Some(Agent::Any), vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_agent_any");
        assert!(!t.passed);
        assert!(t.message.contains("agent any"));
    }

    // ── test_docker_images_pinned ─────────────────────────────────────────────

    #[test]
    fn docker_images_pinned_skipped_when_no_docker() {
        let p = make_pipeline(Some(Agent::Label("linux".into())), vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "docker_images_pinned");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn docker_images_pinned_passes_for_exact_tag() {
        let p = make_pipeline(
            Some(Agent::Docker { image: "node:20.11.0".into(), args: None, custom_workspace: None, reuse_node: None, registry_url: None, registry_credentials_id: None, always_pull: None }),
            vec![],
            None,
        );
        let suite = run_tests(&p);
        let t = find_test(&suite, "docker_images_pinned");
        assert!(t.passed);
        assert!(!t.message.contains("skipped"));
    }

    #[test]
    fn docker_images_pinned_fails_for_latest_tag() {
        let p = make_pipeline(
            Some(Agent::Docker { image: "node:latest".into(), args: None, custom_workspace: None, reuse_node: None, registry_url: None, registry_credentials_id: None, always_pull: None }),
            vec![],
            None,
        );
        let suite = run_tests(&p);
        let t = find_test(&suite, "docker_images_pinned");
        assert!(!t.passed);
        assert!(t.message.contains("node:latest"));
    }

    #[test]
    fn docker_images_pinned_fails_for_image_without_tag() {
        let p = make_pipeline(
            Some(Agent::Docker { image: "node".into(), args: None, custom_workspace: None, reuse_node: None, registry_url: None, registry_credentials_id: None, always_pull: None }),
            vec![],
            None,
        );
        assert!(!find_test(&run_tests(&p), "docker_images_pinned").passed);
    }

    // ── test_input_stages_no_agent ────────────────────────────────────────────

    #[test]
    fn input_stages_no_agent_skipped_when_no_input_stages() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "input_stages_no_agent");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn input_stages_no_agent_passes_when_input_stage_has_no_agent() {
        let p = make_pipeline(None, vec![stage_with_input("Approve", None)], None);
        assert!(find_test(&run_tests(&p), "input_stages_no_agent").passed);
    }

    #[test]
    fn input_stages_no_agent_fails_when_input_stage_has_agent() {
        let p = make_pipeline(
            None,
            vec![stage_with_input("Approve", Some(Agent::Label("linux".into())))],
            None,
        );
        let suite = run_tests(&p);
        let t = find_test(&suite, "input_stages_no_agent");
        assert!(!t.passed);
        assert!(t.message.contains("Approve"));
    }

    // ── test_when_stages_use_before_agent ─────────────────────────────────────

    #[test]
    fn when_before_agent_skipped_when_no_when_with_agent() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "when_stages_use_before_agent");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn when_before_agent_passes_when_before_agent_true() {
        let p = make_pipeline(
            None,
            vec![stage_with_when("Deploy", Some(Agent::Label("linux".into())), true)],
            None,
        );
        assert!(find_test(&run_tests(&p), "when_stages_use_before_agent").passed);
    }

    #[test]
    fn when_before_agent_fails_when_before_agent_false() {
        let p = make_pipeline(
            None,
            vec![stage_with_when("Deploy", Some(Agent::Label("linux".into())), false)],
            None,
        );
        let suite = run_tests(&p);
        let t = find_test(&suite, "when_stages_use_before_agent");
        assert!(!t.passed);
        assert!(t.message.contains("Deploy"));
    }

    // ── test_no_secret_string_parameters ─────────────────────────────────────

    #[test]
    fn no_secret_string_params_skipped_when_no_parameters() {
        let p = make_pipeline(None, vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_secret_string_parameters");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn no_secret_string_params_passes_for_safe_param_name() {
        let mut p = make_pipeline(None, vec![], None);
        p.parameters.push(Parameter::String {
            name: "BRANCH_NAME".into(),
            default_value: None,
            description: None,
            location: None,
        });
        assert!(find_test(&run_tests(&p), "no_secret_string_parameters").passed);
    }

    #[test]
    fn no_secret_string_params_fails_for_password_param() {
        let mut p = make_pipeline(None, vec![], None);
        p.parameters.push(Parameter::String {
            name: "DB_PASSWORD".into(),
            default_value: None,
            description: None,
            location: None,
        });
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_secret_string_parameters");
        assert!(!t.passed);
        assert!(t.message.contains("DB_PASSWORD"));
    }

    // ── test_libraries_pinned_to_version ──────────────────────────────────────

    #[test]
    fn libraries_pinned_skipped_when_no_libraries() {
        let p = make_pipeline(None, vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "libraries_pinned_to_version");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn libraries_pinned_passes_for_sha_ref() {
        use crate::ast::SharedLibrary;
        let mut p = make_pipeline(None, vec![], None);
        p.libraries.push(SharedLibrary { name: "my-lib".into(), ref_: Some("abc123def".into()) });
        assert!(find_test(&run_tests(&p), "libraries_pinned_to_version").passed);
    }

    #[test]
    fn libraries_pinned_fails_for_main_branch() {
        use crate::ast::SharedLibrary;
        let mut p = make_pipeline(None, vec![], None);
        p.libraries.push(SharedLibrary { name: "my-lib".into(), ref_: Some("main".into()) });
        let suite = run_tests(&p);
        let t = find_test(&suite, "libraries_pinned_to_version");
        assert!(!t.passed);
        assert!(t.message.contains("my-lib@main"));
    }

    #[test]
    fn libraries_pinned_fails_for_no_ref() {
        use crate::ast::SharedLibrary;
        let mut p = make_pipeline(None, vec![], None);
        p.libraries.push(SharedLibrary { name: "my-lib".into(), ref_: None });
        let suite = run_tests(&p);
        let t = find_test(&suite, "libraries_pinned_to_version");
        assert!(!t.passed);
        assert!(t.message.contains("no version"));
    }

    // ── test_script_blocks_are_small ─────────────────────────────────────────

    #[test]
    fn script_blocks_small_skipped_when_no_scripts() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "script_blocks_are_small");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn script_blocks_small_passes_for_small_script() {
        let p = make_pipeline(None, vec![stage_with_script("Build", 5)], None);
        assert!(find_test(&run_tests(&p), "script_blocks_are_small").passed);
    }

    #[test]
    fn script_blocks_small_fails_for_large_script() {
        let p = make_pipeline(None, vec![stage_with_script("Build", 20)], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "script_blocks_are_small");
        assert!(!t.passed);
        assert!(t.message.contains("Build"));
    }

    // ── test_checkout_not_duplicated ──────────────────────────────────────────

    #[test]
    fn checkout_not_duplicated_passes_when_no_checkouts() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "checkout_not_duplicated");
        assert!(t.passed);
        assert!(t.message.contains("implicit checkout"));
    }

    #[test]
    fn checkout_not_duplicated_passes_for_single_checkout() {
        let stage = Stage {
            name: "Checkout".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Checkout { scm: "scm".into() }] }),
            post: None,
            duplicate_sections: vec![],
        };
        let p = make_pipeline(None, vec![stage], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "checkout_not_duplicated");
        assert!(t.passed);
        assert!(t.message.contains("exactly once"));
    }

    #[test]
    fn checkout_not_duplicated_fails_for_two_checkouts() {
        let make_checkout_stage = |name: &str| Stage {
            name: name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps { steps: vec![Step::Checkout { scm: "scm".into() }] }),
            post: None,
            duplicate_sections: vec![],
        };
        let p = make_pipeline(None, vec![make_checkout_stage("A"), make_checkout_stage("B")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "checkout_not_duplicated");
        assert!(!t.passed);
        assert!(t.message.contains("2 times"));
    }

    // ── test_deploy_stages_disable_concurrent ─────────────────────────────────

    #[test]
    fn deploy_concurrent_skipped_when_no_deploy_stages() {
        let p = make_pipeline(None, vec![named_stage("Build"), named_stage("Test")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "deploy_stages_disable_concurrent");
        assert!(t.passed);
        assert!(t.message.contains("skipped"));
    }

    #[test]
    fn deploy_concurrent_passes_when_option_set() {
        let mut p = make_pipeline(None, vec![named_stage("Deploy to Prod")], None);
        p.options.push(PipelineOption::DisableConcurrentBuilds { abort_previous: false });
        assert!(find_test(&run_tests(&p), "deploy_stages_disable_concurrent").passed);
    }

    #[test]
    fn deploy_concurrent_fails_when_missing_option() {
        let p = make_pipeline(None, vec![named_stage("Deploy to Prod")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "deploy_stages_disable_concurrent");
        assert!(!t.passed);
        assert!(t.message.contains("Deploy to Prod"));
    }

    // ── test_post_failure_handler_exists ──────────────────────────────────────

    #[test]
    fn post_failure_handler_passes_with_failure_block() {
        let p = make_pipeline(None, vec![], Some(post_failure()));
        assert!(find_test(&run_tests(&p), "post_failure_handler_exists").passed);
    }

    #[test]
    fn post_failure_handler_passes_with_always_block() {
        let p = make_pipeline(None, vec![], Some(post_always()));
        assert!(find_test(&run_tests(&p), "post_failure_handler_exists").passed);
    }

    #[test]
    fn post_failure_handler_fails_when_no_post() {
        let p = make_pipeline(None, vec![], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "post_failure_handler_exists");
        assert!(!t.passed);
        assert!(t.message.contains("No post block"));
    }

    #[test]
    fn post_failure_handler_fails_when_post_has_only_success() {
        let post = Post {
            always: None,
            success: Some(Steps { steps: vec![] }),
            failure: None,
            unstable: None,
            aborted: None,
            changed: None,
            cleanup: None,
            regression: None,
            fixed: None,
            unsuccessful: None,
        };
        let p = make_pipeline(None, vec![], Some(post));
        let suite = run_tests(&p);
        let t = find_test(&suite, "post_failure_handler_exists");
        assert!(!t.passed);
    }

    // ── is_unpinned_image helper ──────────────────────────────────────────────

    #[test]
    fn is_unpinned_image_no_tag_returns_true() {
        assert!(super::is_unpinned_image("node"));
    }

    #[test]
    fn is_unpinned_image_latest_returns_true() {
        assert!(super::is_unpinned_image("node:latest"));
    }

    #[test]
    fn is_unpinned_image_stable_returns_true() {
        assert!(super::is_unpinned_image("node:stable"));
    }

    #[test]
    fn is_unpinned_image_specific_version_returns_false() {
        assert!(!super::is_unpinned_image("node:20.11.0"));
    }

    // ── run_tests() suite metadata ────────────────────────────────────────────

    #[test]
    fn run_tests_always_returns_exactly_22_tests() {
        let p = make_pipeline(None, vec![], None);
        let suite = run_tests(&p);
        assert_eq!(suite.tests.len(), 22);
    }

    #[test]
    fn run_tests_passed_plus_failed_equals_total() {
        let p = make_pipeline(Some(Agent::Any), vec![named_stage("Build"), named_stage("Test")], Some(post_always()));
        let suite = run_tests(&p);
        assert_eq!(suite.passed + suite.failed + suite.skipped, suite.tests.len());
    }

    #[test]
    fn run_tests_skipped_is_always_zero() {
        // The tester does not currently produce skipped results.
        let p = make_pipeline(None, vec![], None);
        assert_eq!(run_tests(&p).skipped, 0);
    }

    // ── test_no_groovy_interpolated_credentials ───────────────────────────────

    fn with_credentials_stage(name: &str, bindings: Vec<Binding>, steps: Vec<Step>) -> Stage {
        Stage {
            name: name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps {
                steps: vec![Step::WithCredentials { bindings, steps }],
            }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    /// Vacuous case: no withCredentials present → test passes.
    #[test]
    fn no_groovy_interpolated_creds_passes_when_no_with_credentials() {
        let p = make_pipeline(None, vec![named_stage("Build")], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_groovy_interpolated_credentials");
        assert!(t.passed);
        assert!(t.message.contains("No credential"));
    }

    /// withCredentials present but sh is single-quoted → test passes.
    #[test]
    fn no_groovy_interpolated_creds_passes_for_single_quoted_sh() {
        let stage = with_credentials_stage(
            "Deploy",
            vec![Binding::StringBinding {
                credentials_id: "my-token".into(),
                variable: "API_TOKEN".into(),
            }],
            vec![Step::Sh {
                script: "curl -H 'Authorization: Bearer $API_TOKEN' https://api.example.com".into(),
                is_double_quoted: false,
                location: None,
            }],
        );
        let p = make_pipeline(None, vec![stage], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_groovy_interpolated_credentials");
        assert!(t.passed, "single-quoted sh should pass: {}", t.message);
    }

    /// withCredentials present and double-quoted sh references credential variable → test fails.
    #[test]
    fn no_groovy_interpolated_creds_fails_for_double_quoted_sh_with_credential() {
        let stage = with_credentials_stage(
            "Deploy",
            vec![Binding::StringBinding {
                credentials_id: "my-token".into(),
                variable: "API_TOKEN".into(),
            }],
            vec![Step::Sh {
                script: "curl -H 'Authorization: Bearer ${API_TOKEN}' https://api.example.com".into(),
                is_double_quoted: true,
                location: None,
            }],
        );
        let p = make_pipeline(None, vec![stage], None);
        let suite = run_tests(&p);
        let t = find_test(&suite, "no_groovy_interpolated_credentials");
        assert!(!t.passed, "double-quoted sh with credential var should fail");
        assert!(t.message.contains("API_TOKEN"), "message should name the variable");
    }
}
