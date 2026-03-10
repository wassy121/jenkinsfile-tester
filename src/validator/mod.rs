pub mod context;
pub mod rules;

use serde::{Deserialize, Serialize};
use crate::ast::Pipeline;
pub use context::ValidationContext;
pub use context::ValidationMode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Suggestion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<crate::ast::Location>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub suggestions: Vec<Diagnostic>,
}

#[allow(dead_code)]
pub fn validate(pipeline: &Pipeline, source: &str) -> ValidationResult {
    let ctx = ValidationContext::new(pipeline, source);
    validate_with_context(ctx)
}

#[allow(dead_code)]
pub fn validate_strict(pipeline: &Pipeline, source: &str) -> ValidationResult {
    let ctx = ValidationContext::new(pipeline, source).with_mode(ValidationMode::Strict);
    validate_with_context(ctx)
}

pub fn validate_with_context(ctx: ValidationContext) -> ValidationResult {
    let mut all: Vec<Diagnostic> = Vec::new();

    all.extend(rules::check_no_pipeline_agent(ctx.pipeline));
    all.extend(rules::check_e006_empty_stages(ctx.pipeline));
    all.extend(rules::check_agent_none_coverage(ctx.pipeline));
    all.extend(rules::check_duplicate_stage_names(ctx.pipeline));
    all.extend(rules::check_empty_steps(ctx.pipeline));
    all.extend(rules::check_credential_in_script(ctx.pipeline));
    all.extend(rules::check_env_naming(ctx.pipeline));
    all.extend(rules::check_parallel_shared_agent(ctx.pipeline));
    all.extend(rules::check_sh_set_e(ctx.pipeline));
    all.extend(rules::check_post_always(ctx.pipeline));
    all.extend(rules::check_global_timeout(ctx.pipeline));
    all.extend(rules::check_deploy_has_when(ctx.pipeline));
    all.extend(rules::check_post_exists(ctx.pipeline));
    all.extend(rules::check_duplicate_parameters(ctx.pipeline));
    all.extend(rules::check_tool_types(ctx.pipeline, &*ctx.registry));
    all.extend(rules::check_e005_duplicate_sections(ctx.pipeline));
    all.extend(rules::check_s005_single_child_combinator(ctx.pipeline));
    all.extend(rules::check_w007_unknown_step(ctx.pipeline, &*ctx.registry, ctx.mode));
    all.extend(rules::check_w008_unknown_agent_type(ctx.pipeline, &*ctx.registry));
    all.extend(rules::check_w009_unknown_option(ctx.pipeline, &*ctx.registry));
    all.extend(rules::check_w010_unknown_trigger(ctx.pipeline, &*ctx.registry));
    all.extend(rules::check_w011_groovy_interpolated_credentials(ctx.pipeline));

    let errors: Vec<_> = all.iter().filter(|d| d.severity == Severity::Error).cloned().collect();
    let warnings: Vec<_> = all.iter().filter(|d| d.severity == Severity::Warning).cloned().collect();
    let suggestions: Vec<_> = all.iter().filter(|d| d.severity == Severity::Suggestion).cloned().collect();

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        warnings,
        suggestions,
    }
}
