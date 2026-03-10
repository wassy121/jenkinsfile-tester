use crate::ast::{Agent, EnvValue, Location, Pipeline, PipelineOption, Stage, StageBody, Step};
use crate::ast::walk::{collect_all_stages, walk_steps_with_stage};
use crate::plugins::PluginRegistry;
use super::{Diagnostic, Severity, ValidationMode};

// E001: No agent defined at pipeline level
pub fn check_no_pipeline_agent(pipeline: &Pipeline) -> Vec<Diagnostic> {
    if pipeline.agent.is_none() {
        vec![Diagnostic {
            code: "E001".into(),
            severity: Severity::Error,
            message: "No agent defined at pipeline level".into(),
            location: Some(Location { line: 1, col: 1 }),
        }]
    } else {
        vec![]
    }
}

// E006: Pipeline has no stages defined
pub fn check_e006_empty_stages(pipeline: &Pipeline) -> Vec<Diagnostic> {
    if pipeline.stages.is_empty() {
        vec![Diagnostic {
            code: "E006".into(),
            severity: Severity::Error,
            message: "Pipeline has no stages defined".into(),
            location: Some(Location { line: 1, col: 1 }),
        }]
    } else {
        vec![]
    }
}

// E002: agent none with stages missing own agent
pub fn check_agent_none_coverage(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if matches!(pipeline.agent, Some(Agent::None)) {
        for stage in &pipeline.stages {
            check_stage_has_agent(stage, &mut diags);
        }
    }
    diags
}

fn check_stage_has_agent(stage: &Stage, diags: &mut Vec<Diagnostic>) {
    if stage.agent.is_none() {
        match &stage.body {
            StageBody::Steps(_) => {
                // Leaf stage (has steps) with no agent — flag it
                diags.push(Diagnostic {
                    code: "E002".into(),
                    severity: Severity::Error,
                    message: format!(
                        "Stage '{}' has no agent but pipeline agent is 'none'",
                        stage.name
                    ),
                    location: stage.location.clone(),
                });
            }
            StageBody::Parallel { stages: branches } => {
                // Recurse into parallel branches — each branch also needs its own agent
                for branch in branches {
                    check_stage_has_agent(branch, diags);
                }
            }
            StageBody::Sequential { stages } => {
                // Recurse into nested sequential stages
                for s in stages {
                    check_stage_has_agent(s, diags);
                }
            }
            StageBody::Matrix(matrix) => {
                for s in &matrix.stages {
                    check_stage_has_agent(s, diags);
                }
            }
        }
    }
}

// E003: Duplicate stage names at same level
pub fn check_duplicate_stage_names(pipeline: &Pipeline) -> Vec<Diagnostic> {
    check_names_at_level(&pipeline.stages)
}

fn check_names_at_level(stages: &[Stage]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for stage in stages {
        if !seen.insert(stage.name.clone()) {
            diags.push(Diagnostic {
                code: "E003".into(),
                severity: Severity::Error,
                message: format!("Duplicate stage name: '{}'", stage.name),
                location: stage.location.clone(),
            });
        }
        match &stage.body {
            StageBody::Sequential { stages: nested } => diags.extend(check_names_at_level(nested)),
            StageBody::Parallel { stages: branches } => diags.extend(check_names_at_level(branches)),
            StageBody::Matrix(matrix) => diags.extend(check_names_at_level(&matrix.stages)),
            StageBody::Steps(_) => {}
        }
    }
    diags
}

// W001: Empty steps blocks
pub fn check_empty_steps(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for stage in &pipeline.stages {
        check_stage_empty_steps(stage, &mut diags);
    }
    diags
}

fn check_stage_empty_steps(stage: &Stage, diags: &mut Vec<Diagnostic>) {
    match &stage.body {
        StageBody::Steps(steps) => {
            if steps.steps.is_empty() {
                diags.push(Diagnostic {
                    code: "W001".into(),
                    severity: Severity::Warning,
                    message: format!("Stage '{}' has an empty steps block", stage.name),
                    location: None,
                });
            }
        }
        StageBody::Parallel { stages: branches } => {
            for s in branches { check_stage_empty_steps(s, diags); }
        }
        StageBody::Sequential { stages: nested } => {
            for s in nested { check_stage_empty_steps(s, diags); }
        }
        StageBody::Matrix(matrix) => {
            for s in &matrix.stages { check_stage_empty_steps(s, diags); }
        }
    }
}

// W002: Credential env var name appears literally in sh script
pub fn check_credential_in_script(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let pipeline_cred_vars: Vec<String> = pipeline.environment.iter()
        .filter(|e| matches!(e.value, EnvValue::Credentials { .. }))
        .map(|e| e.key.clone())
        .collect();

    for stage in &pipeline.stages {
        collect_sh_scripts(stage, &pipeline_cred_vars, &mut diags);
    }
    diags
}

fn collect_sh_scripts(stage: &Stage, inherited_cred_vars: &[String], diags: &mut Vec<Diagnostic>) {
    // Merge pipeline-level cred vars with any declared at this stage level
    let mut all_cred_vars: Vec<String> = inherited_cred_vars.to_vec();
    for e in &stage.environment {
        if matches!(e.value, EnvValue::Credentials { .. }) {
            all_cred_vars.push(e.key.clone());
        }
    }

    match &stage.body {
        StageBody::Steps(steps) => {
            for step in &steps.steps {
                check_step_for_creds(step, &stage.name, &all_cred_vars, diags);
            }
        }
        StageBody::Parallel { stages: branches } => {
            for nested in branches {
                collect_sh_scripts(nested, &all_cred_vars, diags);
            }
        }
        StageBody::Sequential { stages: nested } => {
            for s in nested {
                collect_sh_scripts(s, &all_cred_vars, diags);
            }
        }
        StageBody::Matrix(matrix) => {
            for s in &matrix.stages {
                collect_sh_scripts(s, &all_cred_vars, diags);
            }
        }
    }
}

fn check_step_for_creds(step: &Step, stage_name: &str, cred_vars: &[String], diags: &mut Vec<Diagnostic>) {
    if let Step::Sh { script, .. } = step {
        for var in cred_vars {
            // Check if the literal variable name appears as a whole word (not as a
            // substring of a longer identifier like DEPLOY_KEY inside DEPLOY_KEY_USR),
            // and is NOT properly wrapped in ${VAR} interpolation syntax.
            if contains_whole_word(script, var) && !script.contains(&format!("${{{}}}", var)) {
                diags.push(Diagnostic {
                    code: "W002".into(),
                    severity: Severity::Warning,
                    message: format!(
                        "Credential variable '{}' referenced literally in sh script in stage '{}'",
                        var, stage_name
                    ),
                    location: None,
                });
            }
        }
    }
}

/// Returns true if `haystack` contains `word` as a whole identifier — i.e. not
/// preceded or followed by an alphanumeric or underscore character.
fn contains_whole_word(haystack: &str, word: &str) -> bool {
    let word_bytes = word.as_bytes();
    let hay_bytes = haystack.as_bytes();
    if word_bytes.len() > hay_bytes.len() {
        return false;
    }
    for start in 0..=(hay_bytes.len() - word_bytes.len()) {
        if &hay_bytes[start..start + word_bytes.len()] == word_bytes {
            let before_ok = start == 0 || !is_ident_char(hay_bytes[start - 1]);
            let after_ok = start + word_bytes.len() == hay_bytes.len()
                || !is_ident_char(hay_bytes[start + word_bytes.len()]);
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// W003: Env key not UPPER_SNAKE_CASE
pub fn check_env_naming(pipeline: &Pipeline) -> Vec<Diagnostic> {
    pipeline.environment.iter()
        .filter(|e| !is_upper_snake_case(&e.key))
        .map(|e| Diagnostic {
            code: "W003".into(),
            severity: Severity::Warning,
            message: format!("Environment variable '{}' should be UPPER_SNAKE_CASE", e.key),
            location: None,
        })
        .collect()
}

fn is_upper_snake_case(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
}

// W004: Parallel block where all branches share pipeline-level agent
pub fn check_parallel_shared_agent(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if pipeline.agent.is_some() {
        for stage in &pipeline.stages {
            if let StageBody::Parallel { stages: branches } = &stage.body {
                if branches.iter().all(|s| s.agent.is_none()) && branches.len() > 1 {
                    diags.push(Diagnostic {
                        code: "W004".into(),
                        severity: Severity::Warning,
                        message: format!(
                            "Stage '{}' uses parallel but all branches inherit pipeline agent — consider per-branch agents",
                            stage.name
                        ),
                        location: None,
                    });
                }
            }
        }
    }
    diags
}

// W005: Multi-line sh script lacks set -e (checks stages, nested stages, parallel branches,
// and post sections at both pipeline and stage level)
pub fn check_sh_set_e(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for stage in &pipeline.stages {
        collect_sh_set_e(stage, &mut diags);
    }
    // Pipeline-level post sections
    if let Some(post) = &pipeline.post {
        for steps in [&post.always, &post.success, &post.failure, &post.unstable,
                       &post.aborted, &post.changed, &post.cleanup, &post.regression,
                       &post.fixed, &post.unsuccessful] {
            if let Some(s) = steps {
                check_steps_for_set_e(s, "post", &mut diags);
            }
        }
    }
    diags
}

fn sh_lacks_set_e(script: &str) -> bool {
    script.contains('\n')
        && !script.contains("set -e")
        && !script.contains("set -ex")
        && !script.contains("set -eo")
        && !script.contains("set -o errexit")
}

fn check_steps_for_set_e(steps: &crate::ast::Steps, stage_name: &str, diags: &mut Vec<Diagnostic>) {
    for step in &steps.steps {
        if let Step::Sh { script, location, .. } = step {
            if sh_lacks_set_e(script) {
                diags.push(Diagnostic {
                    code: "W005".into(),
                    severity: Severity::Warning,
                    message: format!(
                        "Multi-line sh script in stage '{}' lacks 'set -e'",
                        stage_name
                    ),
                    location: location.clone(),
                });
            }
        }
    }
}

fn collect_sh_set_e(stage: &Stage, diags: &mut Vec<Diagnostic>) {
    match &stage.body {
        StageBody::Steps(steps) => {
            check_steps_for_set_e(steps, &stage.name, diags);
        }
        StageBody::Parallel { stages: branches } => {
            for s in branches { collect_sh_set_e(s, diags); }
        }
        StageBody::Sequential { stages: nested } => {
            for s in nested { collect_sh_set_e(s, diags); }
        }
        StageBody::Matrix(matrix) => {
            for s in &matrix.stages { collect_sh_set_e(s, diags); }
        }
    }
    // Also check post sections at stage level
    if let Some(post) = &stage.post {
        for steps in [&post.always, &post.success, &post.failure, &post.unstable,
                       &post.aborted, &post.changed, &post.cleanup, &post.regression,
                       &post.fixed, &post.unsuccessful] {
            if let Some(s) = steps {
                check_steps_for_set_e(s, &stage.name, diags);
            }
        }
    }
}

// S001: No post { always } block (only fires when post block exists but lacks always;
// S004 covers the case where there is no post block at all)
pub fn check_post_always(pipeline: &Pipeline) -> Vec<Diagnostic> {
    if pipeline.post.is_none() {
        return vec![];
    }
    let has_always = pipeline.post.as_ref().map_or(false, |p| p.always.is_some());
    if !has_always {
        vec![Diagnostic {
            code: "S001".into(),
            severity: Severity::Suggestion,
            message: "Consider adding a 'post { always { } }' block for cleanup/notifications".into(),
            location: None,
        }]
    } else {
        vec![]
    }
}

// S002: No global options { timeout(...) }
pub fn check_global_timeout(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let has_timeout = pipeline.options.iter().any(|o| matches!(o, PipelineOption::Timeout { .. }));
    if !has_timeout {
        vec![Diagnostic {
            code: "S002".into(),
            severity: Severity::Suggestion,
            message: "Consider adding a global 'options { timeout(...) }' to prevent runaway builds".into(),
            location: None,
        }]
    } else {
        vec![]
    }
}

// S003: Deploy-named stage has no when condition
pub fn check_deploy_has_when(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for stage in &pipeline.stages {
        collect_deploy_when(stage, &mut diags);
    }
    diags
}

fn collect_deploy_when(stage: &Stage, diags: &mut Vec<Diagnostic>) {
    let name_lower = stage.name.to_lowercase();
    if (name_lower.contains("deploy") || name_lower.contains("release") || name_lower.contains("publish"))
        && stage.when.is_none()
    {
        diags.push(Diagnostic {
            code: "S003".into(),
            severity: Severity::Suggestion,
            message: format!(
                "Stage '{}' looks like a deployment stage but has no 'when' condition",
                stage.name
            ),
            location: None,
        });
    }
    match &stage.body {
        StageBody::Sequential { stages: nested } => {
            for s in nested { collect_deploy_when(s, diags); }
        }
        StageBody::Parallel { stages: branches } => {
            for s in branches { collect_deploy_when(s, diags); }
        }
        StageBody::Matrix(matrix) => {
            for s in &matrix.stages { collect_deploy_when(s, diags); }
        }
        StageBody::Steps(_) => {}
    }
}

// S004: No post block at all
pub fn check_post_exists(pipeline: &Pipeline) -> Vec<Diagnostic> {
    if pipeline.post.is_none() {
        vec![Diagnostic {
            code: "S004".into(),
            severity: Severity::Suggestion,
            message: "Pipeline has no 'post' block — consider adding one for notifications and cleanup".into(),
            location: Some(Location { line: 1, col: 1 }),
        }]
    } else {
        vec![]
    }
}

// E004: Duplicate parameter names
pub fn check_duplicate_parameters(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut seen = std::collections::HashSet::new();
    let mut diags = Vec::new();
    for param in &pipeline.parameters {
        let (name, loc) = match param {
            crate::ast::Parameter::String { name, location, .. } => (name, location),
            crate::ast::Parameter::Boolean { name, location, .. } => (name, location),
            crate::ast::Parameter::Choice { name, location, .. } => (name, location),
            crate::ast::Parameter::Text { name, location, .. } => (name, location),
            crate::ast::Parameter::Password { name, location, .. } => (name, location),
            crate::ast::Parameter::File { name, location, .. } => (name, location),
            crate::ast::Parameter::Run { name, location, .. } => (name, location),
        };
        if !seen.insert(name.as_str()) {
            diags.push(Diagnostic {
                code: "E004".into(),
                severity: Severity::Error,
                message: format!("Duplicate parameter name '{}'", name),
                location: loc.clone(),
            });
        }
    }
    diags
}

// W006: Unknown or misspelled tool type
pub fn check_tool_types(pipeline: &Pipeline, registry: &PluginRegistry) -> Vec<Diagnostic> {
    let known: Vec<&str> = registry.all_tools();
    pipeline.tools.iter()
        .filter(|t| {
            let lower = t.tool_type.to_lowercase();
            !known.iter().any(|k| k.to_lowercase() == lower)
        })
        .map(|t| Diagnostic {
            code: "W006".into(),
            severity: Severity::Warning,
            message: format!(
                "Unknown tool type '{}' — not provided by any loaded plugin",
                t.tool_type
            ),
            location: None,
        })
        .collect()
}

// W007: Unknown step name — not in the plugin registry
pub fn check_w007_unknown_step(pipeline: &Pipeline, registry: &PluginRegistry, mode: ValidationMode) -> Vec<Diagnostic> {
    if mode == ValidationMode::Discovery {
        return vec![];
    }
    let severity = if mode == ValidationMode::Strict { Severity::Error } else { Severity::Warning };
    let mut diags = Vec::new();
    for (stage, step) in walk_steps_with_stage(&pipeline.stages) {
        if let Step::Generic { name, .. } = step {
            if !registry.has_step(name) {
                diags.push(Diagnostic {
                    code: "W007".into(),
                    severity: severity.clone(),
                    message: format!(
                        "W007: unknown step '{}' in stage '{}'",
                        name, stage.name
                    ),
                    location: None,
                });
            }
        }
    }
    diags
}

// W008: Plugin-registered agent type not in registry
pub fn check_w008_unknown_agent_type(pipeline: &Pipeline, registry: &PluginRegistry) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let check_agent = |agent: &crate::ast::Agent, diags: &mut Vec<Diagnostic>| {
        if let crate::ast::Agent::Generic { agent_type } = agent {
            if !registry.has_agent_type(agent_type) {
                diags.push(Diagnostic {
                    code: "W008".into(),
                    severity: Severity::Warning,
                    message: format!(
                        "W008: unknown agent type '{}' — not provided by any loaded plugin",
                        agent_type
                    ),
                    location: None,
                });
            }
        }
    };
    if let Some(agent) = &pipeline.agent {
        check_agent(agent, &mut diags);
    }
    for stage in collect_all_stages(&pipeline.stages) {
        if let Some(agent) = &stage.agent {
            check_agent(agent, &mut diags);
        }
    }
    diags
}

// W009: Option name not in plugin registry (only fires for PipelineOption::Raw)
pub fn check_w009_unknown_option(pipeline: &Pipeline, registry: &PluginRegistry) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let check_options = |options: &[crate::ast::PipelineOption], diags: &mut Vec<Diagnostic>| {
        for opt in options {
            if let crate::ast::PipelineOption::Raw { name, .. } = opt {
                if !registry.has_option(name) {
                    diags.push(Diagnostic {
                        code: "W009".into(),
                        severity: Severity::Warning,
                        message: format!(
                            "W009: unknown option '{}' — not provided by any loaded plugin",
                            name
                        ),
                        location: None,
                    });
                }
            }
        }
    };
    check_options(&pipeline.options, &mut diags);
    for stage in collect_all_stages(&pipeline.stages) {
        check_options(&stage.options, &mut diags);
    }
    diags
}

// W010: Trigger name not in plugin registry (only fires for Trigger::Raw)
pub fn check_w010_unknown_trigger(pipeline: &Pipeline, registry: &PluginRegistry) -> Vec<Diagnostic> {
    pipeline.triggers.iter()
        .filter_map(|t| {
            if let crate::ast::Trigger::Raw { name, .. } = t {
                if !registry.has_trigger(name) {
                    return Some(Diagnostic {
                        code: "W010".into(),
                        severity: Severity::Warning,
                        message: format!(
                            "W010: unknown trigger '{}' — not provided by any loaded plugin",
                            name
                        ),
                        location: None,
                    });
                }
            }
            None
        })
        .collect()
}

// E005: Duplicate section declarations (ARC-009)
pub fn check_e005_duplicate_sections(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for name in &pipeline.duplicate_sections {
        diags.push(Diagnostic {
            code: "E005".into(),
            severity: Severity::Error,
            message: format!("E005: section '{}' declared more than once in pipeline", name),
            location: None,
        });
    }
    for stage in collect_all_stages(&pipeline.stages) {
        for name in &stage.duplicate_sections {
            diags.push(Diagnostic {
                code: "E005".into(),
                severity: Severity::Error,
                message: format!("E005: section '{}' declared more than once in stage '{}'", name, stage.name),
                location: None,
            });
        }
    }
    diags
}

// S005: allOf/anyOf with fewer than 2 child conditions (VGAP-007)
pub fn check_s005_single_child_combinator(pipeline: &Pipeline) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for stage in collect_all_stages(&pipeline.stages) {
        if let Some(when) = &stage.when {
            check_when_s005(&when.conditions, &stage.name, &mut diags);
        }
    }
    diags
}

fn check_when_s005(conditions: &[crate::ast::WhenCondition], stage_name: &str, diags: &mut Vec<Diagnostic>) {
    use crate::ast::WhenCondition;
    for cond in conditions {
        match cond {
            WhenCondition::AllOf { conditions: children } | WhenCondition::AnyOf { conditions: children } => {
                if children.len() < 2 {
                    diags.push(Diagnostic {
                        code: "S005".into(),
                        severity: Severity::Suggestion,
                        message: format!(
                            "S005: single-child allOf/anyOf in stage '{}' — consider removing the wrapper",
                            stage_name
                        ),
                        location: None,
                    });
                }
                check_when_s005(children, stage_name, diags);
            }
            _ => {}
        }
    }
}

// W011: Credential variable referenced in double-quoted sh/echo string via Groovy string interpolation
pub fn check_w011_groovy_interpolated_credentials(pipeline: &Pipeline) -> Vec<Diagnostic> {
    use crate::ast::walk::{stage_steps, collect_all_steps_recursive};

    let mut diags = Vec::new();

    // Pipeline-level environment{} credential vars (KEY_USR / KEY_PSW)
    let pipeline_env_vars = collect_env_credential_vars(&pipeline.environment);

    for stage in collect_all_stages(&pipeline.stages) {
        let Some(steps_block) = stage_steps(stage) else { continue };

        let mut env_vars = pipeline_env_vars.clone();
        env_vars.extend(collect_env_credential_vars(&stage.environment));

        for step in &steps_block.steps {
            // Case 1: withCredentials — check inner steps against binding vars
            if let Step::WithCredentials { bindings, steps: inner_steps } = step {
                let cred_vars = collect_binding_vars(bindings);
                for inner_step in collect_all_steps_recursive(inner_steps) {
                    let (text, step_name, location): (&str, &str, Option<&crate::ast::Location>) =
                        match inner_step {
                            Step::Sh { script, is_double_quoted: true, location } =>
                                (script, "sh", location.as_ref()),
                            Step::Echo { message, is_double_quoted: true } =>
                                (message, "echo", None),
                            _ => continue,
                        };
                    for var in &cred_vars {
                        if text.contains(&format!("${{{}}}", var))
                            || text.contains(&format!("${}", var))
                        {
                            diags.push(Diagnostic {
                                code: "W011".into(),
                                severity: Severity::Warning,
                                message: format!(
                                    "Credential variable '{}' is passed to '{}' via Groovy \
                                     string interpolation — the secret value will be embedded \
                                     in the string and bypass credential masking. \
                                     Use single quotes instead.",
                                    var, step_name
                                ),
                                location: location.cloned(),
                            });
                        }
                    }
                }
            }
            // Case 2: any double-quoted sh/echo referencing env-block credential vars
            if !env_vars.is_empty() {
                let (text, step_name, location): (&str, &str, Option<&crate::ast::Location>) =
                    match step {
                        Step::Sh { script, is_double_quoted: true, location } =>
                            (script, "sh", location.as_ref()),
                        Step::Echo { message, is_double_quoted: true } =>
                            (message, "echo", None),
                        _ => continue,
                    };
                for var in &env_vars {
                    if text.contains(&format!("${{{}}}", var))
                        || text.contains(&format!("${}", var))
                    {
                        diags.push(Diagnostic {
                            code: "W011".into(),
                            severity: Severity::Warning,
                            message: format!(
                                "Credential variable '{}' is passed to '{}' via Groovy \
                                 string interpolation — the secret value will be embedded \
                                 in the string and bypass credential masking. \
                                 Use single quotes instead.",
                                var, step_name
                            ),
                            location: location.cloned(),
                        });
                    }
                }
            }
        }
    }

    diags
}

fn collect_env_credential_vars(env: &[crate::ast::EnvVar]) -> Vec<String> {
    let mut vars = Vec::new();
    for var in env {
        if matches!(var.value, crate::ast::EnvValue::Credentials { .. }) {
            vars.push(format!("{}_USR", var.key));
            vars.push(format!("{}_PSW", var.key));
        }
    }
    vars
}

fn collect_binding_vars(bindings: &[crate::ast::Binding]) -> Vec<String> {
    use crate::ast::Binding;
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

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    // ── AST builder helpers ───────────────────────────────────────────────────

    fn empty_pipeline() -> Pipeline {
        Pipeline {
            agent: None,
            environment: vec![],
            options: vec![],
            parameters: vec![],
            triggers: vec![],
            tools: vec![],
            stages: vec![],
            post: None,
            duplicate_sections: vec![],
            libraries: vec![],
        }
    }

    /// A stage with a single `echo` step and no sub-stages / parallel.
    fn leaf_stage(name: &str) -> Stage {
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

    fn sh_stage(name: &str, script: &str) -> Stage {
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
            body: StageBody::Steps(Steps { steps: vec![Step::Sh { script: script.to_string(), is_double_quoted: false, location: None }] }),
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

    fn post_always() -> Post {
        Post {
            always: Some(Steps { steps: vec![Step::Echo { message: "done".into(), is_double_quoted: false }] }),
            success: None, failure: None, unstable: None, aborted: None,
            changed: None, cleanup: None, regression: None, fixed: None,
            unsuccessful: None,
        }
    }

    fn when_branch(pattern: &str) -> When {
        When { conditions: vec![WhenCondition::Branch { pattern: pattern.to_string() }], before_agent: false, before_input: false, before_options: false }
    }

    fn has_code(diags: &[Diagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    // ── E001: check_no_pipeline_agent ────────────────────────────────────────

    /// A pipeline with no agent declaration must produce exactly one E001 error.
    #[test]
    fn e001_fires_when_no_agent() {
        let p = empty_pipeline();
        let diags = check_no_pipeline_agent(&p);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E001");
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].location.is_some(), "E001 must carry a sentinel location");
        assert_eq!(diags[0].location.as_ref().unwrap().line, 1);
    }

    /// A pipeline with agent any must produce no E001.
    #[test]
    fn e001_silent_when_agent_present() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        p.stages.push(leaf_stage("Build"));
        let diags = check_no_pipeline_agent(&p);
        assert!(diags.is_empty());
    }

    // ── E006: check_e006_empty_stages ────────────────────────────────────────

    /// An empty stages list must produce exactly one E006 error.
    #[test]
    fn e006_fires_when_no_stages() {
        let p = empty_pipeline();
        let diags = check_e006_empty_stages(&p);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E006");
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].location.is_some(), "E006 must carry a sentinel location");
    }

    /// A pipeline with at least one stage must produce no E006.
    #[test]
    fn e006_silent_when_stages_present() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Build"));
        let diags = check_e006_empty_stages(&p);
        assert!(diags.is_empty());
    }

    // ── E002: check_agent_none_coverage ──────────────────────────────────────

    /// `agent none` + leaf stage with no own agent → E002 for that stage.
    #[test]
    fn e002_fires_for_agentless_leaf_under_none_pipeline() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::None);
        p.stages.push(leaf_stage("Build")); // no agent override
        let diags = check_agent_none_coverage(&p);
        assert!(has_code(&diags, "E002"), "expected E002, got: {:?}", diags);
        assert!(diags[0].message.contains("Build"));
    }

    /// `agent none` + stage that declares its own agent → no E002.
    #[test]
    fn e002_silent_when_stage_declares_own_agent() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::None);
        let mut s = leaf_stage("Build");
        s.agent = Some(Agent::Label("linux".into()));
        p.stages.push(s);
        let diags = check_agent_none_coverage(&p);
        assert!(!has_code(&diags, "E002"));
    }

    /// `agent any` → rule not triggered regardless of stage agents.
    #[test]
    fn e002_not_triggered_when_pipeline_agent_is_any() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        p.stages.push(leaf_stage("Build"));
        let diags = check_agent_none_coverage(&p);
        assert!(diags.is_empty());
    }

    /// `agent none` + stage using parallel (no direct steps) → no E002.
    /// Parallel branches bear their own agents; the outer parallel stage itself
    /// does not need an agent declaration.
    #[test]
    fn e002_silent_for_parallel_container_stage() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::None);
        let mut branch = leaf_stage("Branch A");
        branch.agent = Some(Agent::Label("x".into()));
        let outer = Stage {
            name: "Test".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![branch] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        let diags = check_agent_none_coverage(&p);
        assert!(!has_code(&diags, "E002"));
    }

    // ── E003: check_duplicate_stage_names ────────────────────────────────────

    /// Two top-level stages with identical names → E003.
    #[test]
    fn e003_fires_for_duplicate_names_at_top_level() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Build"));
        p.stages.push(leaf_stage("Build")); // duplicate
        let diags = check_duplicate_stage_names(&p);
        assert!(has_code(&diags, "E003"));
        assert!(diags[0].message.contains("Build"));
    }

    /// Identical names at *different* nesting levels are fine — Jenkins allows this.
    #[test]
    fn e003_silent_for_same_name_at_different_levels() {
        let mut p = empty_pipeline();
        let inner = leaf_stage("Build"); // nested
        let outer = Stage {
            name: "CI".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Sequential { stages: vec![inner] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        p.stages.push(leaf_stage("Build")); // top-level — different scope
        let diags = check_duplicate_stage_names(&p);
        assert!(!has_code(&diags, "E003"));
    }

    /// Three stages where two share a name → exactly one E003.
    #[test]
    fn e003_exactly_one_diagnostic_per_duplicate() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("A"));
        p.stages.push(leaf_stage("B"));
        p.stages.push(leaf_stage("A")); // second occurrence of "A"
        let diags = check_duplicate_stage_names(&p);
        let e003: Vec<_> = diags.iter().filter(|d| d.code == "E003").collect();
        assert_eq!(e003.len(), 1);
    }

    // ── W001: check_empty_steps ───────────────────────────────────────────────

    /// A stage whose `steps {}` block is empty → W001.
    #[test]
    fn w001_fires_for_empty_steps_block() {
        let mut p = empty_pipeline();
        p.stages.push(empty_steps_stage("Lint"));
        let diags = check_empty_steps(&p);
        assert!(has_code(&diags, "W001"));
        assert!(diags[0].message.contains("Lint"));
    }

    /// A stage with at least one step → no W001.
    #[test]
    fn w001_silent_when_steps_present() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "make"));
        let diags = check_empty_steps(&p);
        assert!(!has_code(&diags, "W001"));
    }

    /// A stage with `parallel` but no `steps` block → no W001.
    /// Parallel stages don't have their own steps.
    #[test]
    fn w001_silent_for_stage_without_steps_block() {
        let mut p = empty_pipeline();
        let outer = Stage {
            name: "Test".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![leaf_stage("Branch A"), leaf_stage("Branch B")] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        let diags = check_empty_steps(&p);
        assert!(!has_code(&diags, "W001"));
    }

    /// W001 is also fired for empty steps inside a nested (parallel) stage.
    #[test]
    fn w001_fires_inside_nested_parallel_stage() {
        let mut p = empty_pipeline();
        let outer = Stage {
            name: "Test".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![empty_steps_stage("Unit"), leaf_stage("Integration")] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        let diags = check_empty_steps(&p);
        assert!(has_code(&diags, "W001"));
    }

    // ── W002: check_credential_in_script ─────────────────────────────────────

    /// An env var backed by `credentials()` whose name appears literally (not as
    /// `${VAR}`) in a sh script → W002.  This is insecure because it could log
    /// the credential ID or cause confusion about whether the secret is expanded.
    #[test]
    fn w002_fires_when_cred_var_used_literally_in_sh() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar {
            key: "MY_TOKEN".into(),
            value: EnvValue::Credentials { id: "my-cred-id".into() },
        });
        // Script references MY_TOKEN without shell interpolation
        p.stages.push(sh_stage("Deploy", "curl -H MY_TOKEN https://api.example.com"));
        let diags = check_credential_in_script(&p);
        assert!(has_code(&diags, "W002"), "expected W002, got: {:?}", diags);
    }

    /// When the credential var is properly expanded as `${MY_TOKEN}`, no W002.
    #[test]
    fn w002_silent_when_cred_var_used_as_interpolation() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar {
            key: "MY_TOKEN".into(),
            value: EnvValue::Credentials { id: "my-cred-id".into() },
        });
        p.stages.push(sh_stage("Deploy", "curl -H ${MY_TOKEN} https://api.example.com"));
        let diags = check_credential_in_script(&p);
        assert!(!has_code(&diags, "W002"));
    }

    /// No credential env vars at all → rule short-circuits, no W002.
    #[test]
    fn w002_silent_when_no_cred_env_vars() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar { key: "APP_NAME".into(), value: EnvValue::Literal("myapp".into()) });
        p.stages.push(sh_stage("Build", "echo APP_NAME"));
        let diags = check_credential_in_script(&p);
        assert!(!has_code(&diags, "W002"));
    }

    // ── W003: check_env_naming ────────────────────────────────────────────────

    /// camelCase env var name → W003.
    #[test]
    fn w003_fires_for_camel_case_env_key() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar { key: "appVersion".into(), value: EnvValue::Literal("1.0".into()) });
        let diags = check_env_naming(&p);
        assert!(has_code(&diags, "W003"));
        assert!(diags[0].message.contains("appVersion"));
    }

    /// UPPER_SNAKE_CASE env var → no W003.
    #[test]
    fn w003_silent_for_upper_snake_case() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar { key: "APP_VERSION".into(), value: EnvValue::Literal("1.0".into()) });
        let diags = check_env_naming(&p);
        assert!(!has_code(&diags, "W003"));
    }

    /// Names containing digits in UPPER_SNAKE_CASE are valid (e.g. `APP_V2`).
    #[test]
    fn w003_silent_for_upper_snake_with_digits() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar { key: "APP_V2".into(), value: EnvValue::Literal("2.0".into()) });
        let diags = check_env_naming(&p);
        assert!(!has_code(&diags, "W003"));
    }

    /// Mixed batch: one bad, one good → only one W003 for the bad one.
    #[test]
    fn w003_only_flags_violating_vars() {
        let mut p = empty_pipeline();
        p.environment.push(EnvVar { key: "goodVar".into(), value: EnvValue::Literal("x".into()) });
        p.environment.push(EnvVar { key: "GOOD_VAR".into(), value: EnvValue::Literal("y".into()) });
        let diags = check_env_naming(&p);
        let w003: Vec<_> = diags.iter().filter(|d| d.code == "W003").collect();
        assert_eq!(w003.len(), 1);
        assert!(w003[0].message.contains("goodVar"));
    }

    // ── W004: check_parallel_shared_agent ────────────────────────────────────

    /// Pipeline has an agent and all parallel branches inherit it → W004.
    #[test]
    fn w004_fires_when_all_parallel_branches_inherit_agent() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        let outer = Stage {
            name: "Test".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![
                leaf_stage("Unit"),        // agent: None — inherits
                leaf_stage("Integration"), // agent: None — inherits
            ] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        let diags = check_parallel_shared_agent(&p);
        assert!(has_code(&diags, "W004"), "expected W004, got: {:?}", diags);
    }

    /// At least one parallel branch has its own agent → no W004.
    #[test]
    fn w004_silent_when_at_least_one_branch_has_own_agent() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        let mut branch_b = leaf_stage("Integration");
        branch_b.agent = Some(Agent::Label("docker".into()));
        let outer = Stage {
            name: "Test".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![leaf_stage("Unit"), branch_b] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        let diags = check_parallel_shared_agent(&p);
        assert!(!has_code(&diags, "W004"));
    }

    /// No pipeline-level agent → W004 is not applicable, no warning.
    #[test]
    fn w004_silent_when_no_pipeline_agent() {
        let mut p = empty_pipeline(); // agent: None (not set)
        let outer = Stage {
            name: "Test".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![leaf_stage("A"), leaf_stage("B")] },
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(outer);
        let diags = check_parallel_shared_agent(&p);
        assert!(!has_code(&diags, "W004"));
    }

    // ── W005: check_sh_set_e ──────────────────────────────────────────────────

    /// A multi-line sh script without `set -e` → W005.
    #[test]
    fn w005_fires_for_multiline_sh_without_set_e() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "npm install\nnpm run build"));
        let diags = check_sh_set_e(&p);
        assert!(has_code(&diags, "W005"), "expected W005, got: {:?}", diags);
    }

    /// Single-line sh script → no W005 regardless of content.
    #[test]
    fn w005_silent_for_single_line_sh() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "make all"));
        let diags = check_sh_set_e(&p);
        assert!(!has_code(&diags, "W005"));
    }

    /// Multi-line sh with `set -e` at the top → no W005.
    #[test]
    fn w005_silent_for_multiline_sh_with_set_e() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "set -e\nnpm install\nnpm run build"));
        let diags = check_sh_set_e(&p);
        assert!(!has_code(&diags, "W005"));
    }

    /// Multi-line sh with `set -ex` (trace + exit-on-error) → no W005.
    #[test]
    fn w005_silent_for_multiline_sh_with_set_ex() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "set -ex\nnpm install\nnpm run build"));
        let diags = check_sh_set_e(&p);
        assert!(!has_code(&diags, "W005"));
    }

    /// Multi-line sh with `set -eo pipefail` → no W005 (REC-015).
    #[test]
    fn w005_silent_for_multiline_sh_with_set_eo_pipefail() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "set -eo pipefail\nnpm install\nnpm run build"));
        let diags = check_sh_set_e(&p);
        assert!(!has_code(&diags, "W005"));
    }

    /// Multi-line sh in a post { always } section fires W005 (REC-008).
    #[test]
    fn w005_fires_for_multiline_sh_in_post_always() {
        let mut p = empty_pipeline();
        p.post = Some(Post {
            always: Some(Steps { steps: vec![Step::Sh { script: "curl notify\ncurl cleanup".into(), is_double_quoted: false, location: None }] }),
            success: None, failure: None, unstable: None, aborted: None,
            changed: None, cleanup: None, regression: None, fixed: None,
            unsuccessful: None,
        });
        p.stages.push(leaf_stage("Build"));
        let diags = check_sh_set_e(&p);
        assert!(has_code(&diags, "W005"), "expected W005 in post always, got: {:?}", diags);
    }

    /// Multi-line sh in a pipeline-level post section with `set -e` → no W005.
    #[test]
    fn w005_silent_for_post_sh_with_set_e() {
        let mut p = empty_pipeline();
        p.post = Some(Post {
            always: Some(Steps { steps: vec![Step::Sh { script: "set -e\ncurl notify\ncurl cleanup".into(), is_double_quoted: false, location: None }] }),
            success: None, failure: None, unstable: None, aborted: None,
            changed: None, cleanup: None, regression: None, fixed: None,
            unsuccessful: None,
        });
        p.stages.push(leaf_stage("Build"));
        let diags = check_sh_set_e(&p);
        assert!(!has_code(&diags, "W005"));
    }

    // ── E004: check_duplicate_parameters ─────────────────────────────────────

    /// Two parameters with the same name → E004.
    #[test]
    fn e004_fires_for_duplicate_parameter_name() {
        let mut p = empty_pipeline();
        p.parameters.push(Parameter::String { name: "VERSION".into(), default_value: None, description: None, location: None });
        p.parameters.push(Parameter::String { name: "VERSION".into(), default_value: None, description: None, location: None });
        let diags = check_duplicate_parameters(&p);
        assert!(has_code(&diags, "E004"), "expected E004, got: {:?}", diags);
    }

    /// Parameters with distinct names → no E004.
    #[test]
    fn e004_silent_for_unique_parameter_names() {
        let mut p = empty_pipeline();
        p.parameters.push(Parameter::String { name: "VERSION".into(), default_value: None, description: None, location: None });
        p.parameters.push(Parameter::Boolean { name: "DEPLOY".into(), default_value: None, description: None, location: None });
        let diags = check_duplicate_parameters(&p);
        assert!(!has_code(&diags, "E004"));
    }

    // ── W006: check_tool_types ────────────────────────────────────────────────

    /// An unknown tool type → W006.
    #[test]
    fn w006_fires_for_unknown_tool_type() {
        let mut p = empty_pipeline();
        p.tools.push(Tool { tool_type: "maeven".into(), name: "M3".into() }); // typo
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_tool_types(&p, &registry);
        assert!(has_code(&diags, "W006"), "expected W006, got: {:?}", diags);
    }

    /// A known tool type → no W006.
    #[test]
    fn w006_silent_for_known_tool_types() {
        let mut p = empty_pipeline();
        p.tools.push(Tool { tool_type: "maven".into(), name: "M3".into() });
        p.tools.push(Tool { tool_type: "jdk".into(), name: "JDK17".into() });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_tool_types(&p, &registry);
        assert!(!has_code(&diags, "W006"));
    }

    // ── S001: check_post_always ───────────────────────────────────────────────

    /// No post block at all → S001 does NOT fire (S004 covers this case).
    #[test]
    fn s001_does_not_fire_when_no_post_block() {
        let p = empty_pipeline();
        let diags = check_post_always(&p);
        assert!(!has_code(&diags, "S001"), "S001 should not fire when post is absent; S004 covers that");
    }

    /// Post block exists but has no `always` section → S001 suggestion.
    #[test]
    fn s001_fires_when_post_has_no_always_section() {
        let mut p = empty_pipeline();
        p.post = Some(Post {
            always: None,
            success: Some(Steps { steps: vec![Step::Echo { message: "ok".into(), is_double_quoted: false }] }),
            failure: None, unstable: None, aborted: None,
            changed: None, cleanup: None, regression: None, fixed: None,
            unsuccessful: None,
        });
        let diags = check_post_always(&p);
        assert!(has_code(&diags, "S001"));
    }

    /// Post block with an `always` section → no S001.
    #[test]
    fn s001_silent_when_post_has_always() {
        let mut p = empty_pipeline();
        p.post = Some(post_always());
        let diags = check_post_always(&p);
        assert!(!has_code(&diags, "S001"));
    }

    // ── S002: check_global_timeout ────────────────────────────────────────────

    /// No options at all → S002.
    #[test]
    fn s002_fires_when_no_options_defined() {
        let p = empty_pipeline();
        let diags = check_global_timeout(&p);
        assert!(has_code(&diags, "S002"));
    }

    /// Options present but none are Timeout → S002.
    #[test]
    fn s002_fires_when_options_lack_timeout() {
        let mut p = empty_pipeline();
        p.options.push(PipelineOption::BuildDiscarder {
            num_to_keep: Some("5".into()),
            days_to_keep: None,
            artifact_num_to_keep: None,
            artifact_days_to_keep: None,
            raw: None,
        });
        let diags = check_global_timeout(&p);
        assert!(has_code(&diags, "S002"));
    }

    /// Options contains a Timeout entry → no S002.
    #[test]
    fn s002_silent_when_timeout_option_present() {
        let mut p = empty_pipeline();
        p.options.push(PipelineOption::Timeout { time: 30, unit: "MINUTES".into() });
        let diags = check_global_timeout(&p);
        assert!(!has_code(&diags, "S002"));
    }

    // ── S003: check_deploy_has_when ───────────────────────────────────────────

    /// A stage named "Deploy" with no `when` clause → S003.
    #[test]
    fn s003_fires_for_deploy_stage_without_when() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Deploy"));
        let diags = check_deploy_has_when(&p);
        assert!(has_code(&diags, "S003"), "expected S003, got: {:?}", diags);
    }

    /// A stage named "Deploy" that has a `when` clause → no S003.
    #[test]
    fn s003_silent_when_deploy_stage_has_when() {
        let mut p = empty_pipeline();
        let mut s = leaf_stage("Deploy");
        s.when = Some(when_branch("main"));
        p.stages.push(s);
        let diags = check_deploy_has_when(&p);
        assert!(!has_code(&diags, "S003"));
    }

    /// Case-insensitive: "DEPLOY TO PRODUCTION" → S003.
    #[test]
    fn s003_fires_case_insensitively() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("DEPLOY TO PRODUCTION"));
        let diags = check_deploy_has_when(&p);
        assert!(has_code(&diags, "S003"));
    }

    /// "Publish Artifacts" (contains "publish") → S003.
    #[test]
    fn s003_fires_for_publish_named_stage() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Publish Artifacts"));
        let diags = check_deploy_has_when(&p);
        assert!(has_code(&diags, "S003"));
    }

    /// "Release" in stage name → S003.
    #[test]
    fn s003_fires_for_release_named_stage() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Create Release"));
        let diags = check_deploy_has_when(&p);
        assert!(has_code(&diags, "S003"));
    }

    /// Unrelated stage name "Build" → no S003.
    #[test]
    fn s003_silent_for_non_deploy_stage() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Build"));
        let diags = check_deploy_has_when(&p);
        assert!(!has_code(&diags, "S003"));
    }

    // ── S004: check_post_exists ───────────────────────────────────────────────

    /// No post block → S004.
    #[test]
    fn s004_fires_when_post_missing() {
        let p = empty_pipeline();
        let diags = check_post_exists(&p);
        assert!(has_code(&diags, "S004"));
    }

    /// Post block present (even if empty of sections) → no S004.
    #[test]
    fn s004_silent_when_post_present() {
        let mut p = empty_pipeline();
        p.post = Some(post_always());
        let diags = check_post_exists(&p);
        assert!(!has_code(&diags, "S004"));
    }

    // ── is_upper_snake_case (private helper) ──────────────────────────────────

    #[test]
    fn upper_snake_case_valid_cases() {
        assert!(is_upper_snake_case("HELLO"));
        assert!(is_upper_snake_case("HELLO_WORLD"));
        assert!(is_upper_snake_case("APP_V2"));
        assert!(is_upper_snake_case("_"));
        assert!(is_upper_snake_case("A1_B2"));
    }

    #[test]
    fn upper_snake_case_invalid_cases() {
        assert!(!is_upper_snake_case("hello"));
        assert!(!is_upper_snake_case("helloWorld"));
        assert!(!is_upper_snake_case("Hello_World"));
        assert!(!is_upper_snake_case(""));
        assert!(!is_upper_snake_case("APP-NAME")); // hyphens not allowed
    }

    // ── W007: check_w007_unknown_step ────────────────────────────────────────

    fn generic_step_stage(stage_name: &str, step_name: &str) -> Stage {
        Stage {
            name: stage_name.to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps {
                steps: vec![Step::Generic { name: step_name.to_string(), args: vec![] }],
            }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    /// Step::Generic with unknown name → W007 (Permissive mode = Warning).
    #[test]
    fn w007_fires_for_unknown_generic_step() {
        let mut p = empty_pipeline();
        p.stages.push(generic_step_stage("Build", "unknownStep"));
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w007_unknown_step(&p, &registry, ValidationMode::Permissive);
        assert!(has_code(&diags, "W007"), "expected W007, got: {:?}", diags);
        assert_eq!(diags[0].severity, Severity::Warning, "Permissive mode should produce Warning");
        assert!(diags[0].message.contains("unknownStep"), "message should contain step name");
        assert!(diags[0].message.contains("Build"), "message should contain stage name");
    }

    /// Step::Generic with name in builtin registry → no W007.
    #[test]
    fn w007_silent_for_step_in_registry() {
        let mut p = empty_pipeline();
        p.stages.push(generic_step_stage("Notify", "slackSend"));
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w007_unknown_step(&p, &registry, ValidationMode::Permissive);
        assert!(!has_code(&diags, "W007"), "expected no W007 for step in registry, got: {:?}", diags);
    }

    /// Typed steps (Step::Sh) never trigger W007 — only Step::Generic does.
    #[test]
    fn w007_silent_for_typed_steps() {
        let mut p = empty_pipeline();
        p.stages.push(sh_stage("Build", "make all"));
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w007_unknown_step(&p, &registry, ValidationMode::Permissive);
        assert!(!has_code(&diags, "W007"), "expected no W007 for typed Step::Sh, got: {:?}", diags);
    }

    /// Strict mode: unknown Step::Generic → W007 with Error severity.
    #[test]
    fn w007_strict_mode_emits_error() {
        let mut p = empty_pipeline();
        p.stages.push(generic_step_stage("Build", "unknownStep"));
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w007_unknown_step(&p, &registry, ValidationMode::Strict);
        assert!(has_code(&diags, "W007"), "expected W007 in strict mode");
        assert_eq!(diags[0].severity, Severity::Error, "Strict mode should produce Error");
    }

    /// Discovery mode: unknown Step::Generic → silent (no diagnostics).
    #[test]
    fn w007_discovery_mode_is_silent() {
        let mut p = empty_pipeline();
        p.stages.push(generic_step_stage("Build", "unknownStep"));
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w007_unknown_step(&p, &registry, ValidationMode::Discovery);
        assert!(diags.is_empty(), "Discovery mode should emit no diagnostics, got: {:?}", diags);
    }

    // ── ARC-018: WhenCondition::Generic does not panic any rule ──────────────

    /// A pipeline with WhenCondition::Generic (catch-all for unknown conditions)
    /// must not panic or produce unexpected errors in any validator rule.
    #[test]
    fn arc018_when_condition_generic_does_not_panic_rules() {
        let mut p = empty_pipeline();
        let mut s = leaf_stage("Deploy");
        s.when = Some(When {
            conditions: vec![WhenCondition::Generic {
                name: "customCondition".into(),
                args: vec!["arg1".into()],
            }],
            before_agent: false, before_input: false, before_options: false,
        });
        p.stages.push(s);

        // Run all rules — none should panic
        let _ = check_no_pipeline_agent(&p);
        let _ = check_e006_empty_stages(&p);
        let _ = check_agent_none_coverage(&p);
        let _ = check_duplicate_stage_names(&p);
        let _ = check_empty_steps(&p);
        let _ = check_credential_in_script(&p);
        let _ = check_env_naming(&p);
        let _ = check_parallel_shared_agent(&p);
        let _ = check_sh_set_e(&p);
        let _ = check_post_always(&p);
        let _ = check_global_timeout(&p);
        let _ = check_deploy_has_when(&p);
        let _ = check_post_exists(&p);
        let _ = check_duplicate_parameters(&p);
        let registry = crate::plugins::PluginRegistry::builtin();
        let _ = check_tool_types(&p, &registry);
        let _ = check_e005_duplicate_sections(&p);
        let _ = check_s005_single_child_combinator(&p);
        let _ = check_w007_unknown_step(&p, &registry, ValidationMode::Permissive);
    }

    // ── E005: check_e005_duplicate_sections ──────────────────────────────────

    /// Pipeline with duplicate_sections populated → E005 error.
    #[test]
    fn e005_fires_for_duplicate_pipeline_section() {
        let mut p = empty_pipeline();
        p.duplicate_sections = vec!["agent".to_string()];
        p.stages.push(leaf_stage("Build"));
        let diags = check_e005_duplicate_sections(&p);
        assert!(has_code(&diags, "E005"), "expected E005, got: {:?}", diags);
        assert!(diags[0].message.contains("agent"));
    }

    /// Pipeline with no duplicates → no E005.
    #[test]
    fn e005_silent_when_no_duplicate_sections() {
        let mut p = empty_pipeline();
        p.stages.push(leaf_stage("Build"));
        let diags = check_e005_duplicate_sections(&p);
        assert!(!has_code(&diags, "E005"));
    }

    /// Stage with duplicate_sections populated → E005 error.
    #[test]
    fn e005_fires_for_duplicate_stage_section() {
        let mut p = empty_pipeline();
        let mut s = leaf_stage("Deploy");
        s.duplicate_sections = vec!["environment".to_string()];
        p.stages.push(s);
        let diags = check_e005_duplicate_sections(&p);
        assert!(has_code(&diags, "E005"), "expected E005, got: {:?}", diags);
        assert!(diags[0].message.contains("environment"));
        assert!(diags[0].message.contains("Deploy"));
    }

    // ── S005: check_s005_single_child_combinator ─────────────────────────────

    /// allOf with single child → S005.
    #[test]
    fn s005_fires_for_allof_with_single_child() {
        let mut p = empty_pipeline();
        let mut s = leaf_stage("Build");
        s.when = Some(When {
            conditions: vec![WhenCondition::AllOf {
                conditions: vec![WhenCondition::Branch { pattern: "main".into() }],
            }],
            before_agent: false, before_input: false, before_options: false,
        });
        p.stages.push(s);
        let diags = check_s005_single_child_combinator(&p);
        assert!(has_code(&diags, "S005"), "expected S005, got: {:?}", diags);
    }

    /// anyOf with two children → no S005.
    #[test]
    fn s005_silent_for_anyof_with_two_children() {
        let mut p = empty_pipeline();
        let mut s = leaf_stage("Build");
        s.when = Some(When {
            conditions: vec![WhenCondition::AnyOf {
                conditions: vec![
                    WhenCondition::Branch { pattern: "main".into() },
                    WhenCondition::Branch { pattern: "develop".into() },
                ],
            }],
            before_agent: false, before_input: false, before_options: false,
        });
        p.stages.push(s);
        let diags = check_s005_single_child_combinator(&p);
        assert!(!has_code(&diags, "S005"));
    }

    /// allOf with zero children → S005 (degenerate case).
    #[test]
    fn s005_fires_for_allof_with_zero_children() {
        let mut p = empty_pipeline();
        let mut s = leaf_stage("Build");
        s.when = Some(When {
            conditions: vec![WhenCondition::AllOf { conditions: vec![] }],
            before_agent: false, before_input: false, before_options: false,
        });
        p.stages.push(s);
        let diags = check_s005_single_child_combinator(&p);
        assert!(has_code(&diags, "S005"), "expected S005 for empty allOf");
    }

    // ── W008: check_w008_unknown_agent_type ──────────────────────────────────

    /// Agent::Generic with type not in registry → W008.
    #[test]
    fn w008_fires_for_unregistered_generic_agent_type() {
        let mut p = empty_pipeline();
        p.agent = Some(crate::ast::Agent::Generic { agent_type: "myCustomAgent".into() });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w008_unknown_agent_type(&p, &registry);
        assert!(has_code(&diags, "W008"), "expected W008, got: {:?}", diags);
        assert!(diags[0].message.contains("myCustomAgent"));
    }

    /// Agent::Generic with a type that IS in the registry → no W008.
    #[test]
    fn w008_silent_for_registered_agent_type() {
        let mut p = empty_pipeline();
        p.agent = Some(crate::ast::Agent::Generic { agent_type: "kubernetes".into() });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w008_unknown_agent_type(&p, &registry);
        assert!(!has_code(&diags, "W008"), "expected no W008 for kubernetes, got: {:?}", diags);
    }

    /// Non-generic agent types (Any, Docker, etc.) never fire W008.
    #[test]
    fn w008_silent_for_typed_agents() {
        let mut p = empty_pipeline();
        p.agent = Some(crate::ast::Agent::Any);
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w008_unknown_agent_type(&p, &registry);
        assert!(!has_code(&diags, "W008"), "expected no W008 for Agent::Any");
    }

    // ── W009: check_w009_unknown_option ──────────────────────────────────────

    /// PipelineOption::Raw with name not in registry → W009.
    #[test]
    fn w009_fires_for_unregistered_raw_option() {
        let mut p = empty_pipeline();
        p.options.push(crate::ast::PipelineOption::Raw {
            name: "myCustomOption".into(),
            text: "myCustomOption()".into(),
        });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w009_unknown_option(&p, &registry);
        assert!(has_code(&diags, "W009"), "expected W009, got: {:?}", diags);
        assert!(diags[0].message.contains("myCustomOption"));
    }

    /// PipelineOption::Raw with name in registry → no W009.
    #[test]
    fn w009_silent_for_registered_option() {
        let mut p = empty_pipeline();
        p.options.push(crate::ast::PipelineOption::Raw {
            name: "timestamps".into(),
            text: "timestamps()".into(),
        });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w009_unknown_option(&p, &registry);
        assert!(!has_code(&diags, "W009"), "expected no W009 for 'timestamps', got: {:?}", diags);
    }

    // ── W010: check_w010_unknown_trigger ─────────────────────────────────────

    /// Trigger::Raw with name not in registry → W010.
    #[test]
    fn w010_fires_for_unregistered_raw_trigger() {
        let mut p = empty_pipeline();
        p.triggers.push(crate::ast::Trigger::Raw {
            name: "myCustomTrigger".into(),
            text: "myCustomTrigger()".into(),
        });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w010_unknown_trigger(&p, &registry);
        assert!(has_code(&diags, "W010"), "expected W010, got: {:?}", diags);
        assert!(diags[0].message.contains("myCustomTrigger"));
    }

    /// Trigger::Raw with name in registry → no W010.
    #[test]
    fn w010_silent_for_registered_trigger() {
        let mut p = empty_pipeline();
        p.triggers.push(crate::ast::Trigger::Raw {
            name: "GenericTrigger".into(),
            text: "GenericTrigger(token: 'abc')".into(),
        });
        let registry = crate::plugins::PluginRegistry::builtin();
        let diags = check_w010_unknown_trigger(&p, &registry);
        assert!(!has_code(&diags, "W010"), "expected no W010 for 'GenericTrigger', got: {:?}", diags);
    }

    // ── LOC-001: W005 carries location from Step::Sh ─────────────────────────

    /// W005 diagnostic for sh-without-set-e must carry the location of the Step::Sh node.
    /// When the step has a location, the diagnostic should reflect it.
    #[test]
    fn w005_sh_without_set_e_carries_location_when_set() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        let stage = Stage {
            name: "Build".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps {
                steps: vec![Step::Sh {
                    script: "npm install\nnpm run build".into(),
                    is_double_quoted: false,
                    location: Some(Location { line: 5, col: 1 }),
                }],
            }),
            post: None,
            duplicate_sections: vec![],
        };
        p.stages.push(stage);
        let diags = check_sh_set_e(&p);
        assert!(has_code(&diags, "W005"), "expected W005, got: {:?}", diags);
        let loc = diags[0].location.as_ref().expect("W005 should carry location");
        assert_eq!(loc.line, 5, "W005 location should match the Step::Sh location");
    }

    /// W005 diagnostic for sh-without-set-e without Step::Sh location emits None location.
    #[test]
    fn w005_sh_without_location_emits_none() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        p.stages.push(Stage {
            name: "Build".into(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Steps(Steps {
                steps: vec![Step::Sh {
                    script: "npm install\nnpm run build".into(),
                    is_double_quoted: false,
                    location: None,
                }],
            }),
            post: None,
            duplicate_sections: vec![],
        });
        let diags = check_sh_set_e(&p);
        assert!(has_code(&diags, "W005"), "expected W005");
        // location is None when step has no location
        assert!(diags[0].location.is_none());
    }

    // ── LOC-002: E001 and S004 carry sentinel location ────────────────────────

    /// E001 (no agent) must carry sentinel location line=1, col=1.
    #[test]
    fn e001_no_agent_has_sentinel_location() {
        let p = empty_pipeline();
        let diags = check_no_pipeline_agent(&p);
        assert!(has_code(&diags, "E001"), "expected E001");
        let loc = diags[0].location.as_ref().expect("E001 must have location");
        assert_eq!(loc.line, 1);
        assert_eq!(loc.col, 1);
    }

    /// S004 (no post section) must carry sentinel location line=1, col=1.
    #[test]
    fn s004_no_post_has_sentinel_location() {
        let p = empty_pipeline();
        let diags = check_post_exists(&p);
        assert!(has_code(&diags, "S004"), "expected S004");
        let loc = diags[0].location.as_ref().expect("S004 must have location");
        assert_eq!(loc.line, 1);
        assert_eq!(loc.col, 1);
    }

    // ── W011: check_w011_groovy_interpolated_credentials ─────────────────────

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

    /// Double-quoted sh step referencing a bound credential variable triggers W011.
    #[test]
    fn w011_groovy_interpolated_credential_in_double_quoted_sh_triggers_warning() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        p.stages.push(with_credentials_stage(
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
        ));
        let diags = check_w011_groovy_interpolated_credentials(&p);
        assert!(has_code(&diags, "W011"), "expected W011, got: {:?}", diags);
        assert!(diags[0].message.contains("API_TOKEN"));
    }

    /// Single-quoted sh step with the same credential variable must not trigger W011.
    #[test]
    fn w011_single_quoted_sh_with_credential_is_clean() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        p.stages.push(with_credentials_stage(
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
        ));
        let diags = check_w011_groovy_interpolated_credentials(&p);
        assert!(!has_code(&diags, "W011"), "W011 must not fire for single-quoted sh");
    }

    /// Double-quoted sh step that does NOT reference any credential variable must not trigger W011.
    #[test]
    fn w011_double_quoted_sh_without_credential_var_is_clean() {
        let mut p = empty_pipeline();
        p.agent = Some(Agent::Any);
        p.stages.push(with_credentials_stage(
            "Deploy",
            vec![Binding::StringBinding {
                credentials_id: "my-token".into(),
                variable: "API_TOKEN".into(),
            }],
            vec![Step::Sh {
                script: "curl https://api.example.com".into(),
                is_double_quoted: true,
                location: None,
            }],
        ));
        let diags = check_w011_groovy_interpolated_credentials(&p);
        assert!(!has_code(&diags, "W011"), "W011 must not fire when credential var not in script");
    }
}
