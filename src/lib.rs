mod ast;
mod parser;
mod plugins;
mod validator;
mod tester;
mod utils;

use wasm_bindgen::prelude::*;
use serde_json::json;
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static THREAD_REGISTRY: RefCell<Option<Arc<plugins::PluginRegistry>>> = RefCell::new(None);
}

#[wasm_bindgen(start)]
pub fn main() {
    utils::set_panic_hook();
}

/// Parse a Jenkinsfile and return JSON: { success, error?, ast? }
#[wasm_bindgen]
pub fn parse_jenkinsfile(src: &str) -> String {
    match parser::parse(src) {
        Ok(ast) => {
            match serde_json::to_value(&ast) {
                Ok(ast_val) => json!({ "success": true, "ast": ast_val }).to_string(),
                Err(e) => json!({ "success": false, "error": e.to_string() }).to_string(),
            }
        }
        Err(e) => json!({
            "success": false,
            "error": {
                "message": e.message,
                "line": e.line,
                "col": e.col,
            }
        }).to_string(),
    }
}

/// Validate a Jenkinsfile and return JSON: { is_valid, errors[], warnings[], suggestions[] }
/// Uses the THREAD_REGISTRY if one has been set via `init_registry()`, otherwise uses the builtin.
#[wasm_bindgen]
pub fn validate(src: &str) -> String {
    let registry = THREAD_REGISTRY.with(|r| r.borrow().clone());
    match parser::parse(src) {
        Ok(ast) => {
            let result = if let Some(reg) = registry {
                let ctx = validator::context::ValidationContext::with_registry(&ast, src, reg);
                validator::validate_with_context(ctx)
            } else {
                validator::validate(&ast, src)
            };
            // Note: this fallback is unreachable in practice — ValidationResult always serialises successfully.
            serde_json::to_string(&result).unwrap_or_else(|e| {
                json!({ "is_valid": false, "errors": [e.to_string()], "warnings": [], "suggestions": [] }).to_string()
            })
        }
        Err(e) => {
            json!({
                "is_valid": false,
                "errors": [{ "code": "PARSE", "severity": "error", "message": e.message, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string()
        }
    }
}

/// Validate with an additional user-supplied plugin registry JSON merged into the builtin.
/// Returns the same JSON structure as `validate()`.
#[wasm_bindgen]
pub fn validate_with_registry(src: &str, registry_json: &str) -> String {
    use std::sync::Arc;
    let user_registry = match plugins::PluginRegistry::from_json(registry_json) {
        Ok(r) => r,
        Err(e) => {
            return json!({
                "is_valid": false,
                "errors": [{ "code": "REGISTRY", "severity": "error", "message": e, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string();
        }
    };
    let mut merged = (*plugins::PluginRegistry::builtin_arc()).clone();
    merged.merge(user_registry);
    match parser::parse(src) {
        Ok(ast) => {
            let ctx = validator::context::ValidationContext::with_registry(&ast, src, Arc::new(merged));
            let result = validator::validate_with_context(ctx);
            serde_json::to_string(&result).unwrap_or_else(|e| {
                json!({ "is_valid": false, "errors": [e.to_string()], "warnings": [], "suggestions": [] }).to_string()
            })
        }
        Err(e) => {
            json!({
                "is_valid": false,
                "errors": [{ "code": "PARSE", "severity": "error", "message": e.message, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string()
        }
    }
}

/// Set a persistent per-thread plugin registry that `validate()` will use.
/// Merges the user-supplied JSON into the builtin registry and stores the result.
/// Returns `{"ok": true}` on success or `{"ok": false, "error": "..."}` on failure.
#[wasm_bindgen]
pub fn init_registry(registry_json: String) -> String {
    let user_registry = match plugins::PluginRegistry::from_json(&registry_json) {
        Ok(r) => r,
        Err(e) => {
            return json!({ "ok": false, "error": e }).to_string();
        }
    };
    let mut merged = (*plugins::PluginRegistry::builtin_arc()).clone();
    merged.merge(user_registry);
    let arc = Arc::new(merged);
    THREAD_REGISTRY.with(|r| {
        *r.borrow_mut() = Some(arc);
    });
    json!({ "ok": true }).to_string()
}

/// Validate in strict mode: unknown steps become errors rather than warnings.
/// Returns the same JSON structure as `validate()`.
#[wasm_bindgen]
pub fn validate_strict(src: &str) -> String {
    let registry = THREAD_REGISTRY.with(|r| r.borrow().clone());
    match parser::parse(src) {
        Ok(ast) => {
            let result = if let Some(reg) = registry {
                let ctx = validator::context::ValidationContext::with_registry(&ast, src, reg)
                    .with_mode(validator::ValidationMode::Strict);
                validator::validate_with_context(ctx)
            } else {
                validator::validate_strict(&ast, src)
            };
            serde_json::to_string(&result).unwrap_or_else(|e| {
                json!({ "is_valid": false, "errors": [e.to_string()], "warnings": [], "suggestions": [] }).to_string()
            })
        }
        Err(e) => {
            json!({
                "is_valid": false,
                "errors": [{ "code": "PARSE", "severity": "error", "message": e.message, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string()
        }
    }
}

/// Return a sorted JSON array of step names found in the pipeline that are not
/// registered in the loaded plugin registry.  Returns `"[]"` on parse error.
#[wasm_bindgen]
pub fn get_unknown_keywords(src: &str) -> String {
    let ast = match parser::parse(src) {
        Ok(a) => a,
        Err(_) => return "[]".into(),
    };
    let registry = THREAD_REGISTRY.with(|r| r.borrow().clone())
        .unwrap_or_else(|| plugins::PluginRegistry::builtin_arc());
    let mut unknown: Vec<String> = ast::walk::walk_steps_with_stage(&ast.stages)
        .into_iter()
        .filter_map(|(_, step)| {
            if let ast::Step::Generic { name, .. } = step {
                if !registry.has_step(name) {
                    return Some(name.clone());
                }
            }
            None
        })
        .collect();
    unknown.sort();
    unknown.dedup();
    serde_json::to_string(&unknown).unwrap_or_else(|_| "[]".into())
}

/// Run structural tests and return JSON TestSuite
#[wasm_bindgen]
pub fn run_tests(src: &str) -> String {
    match parser::parse(src) {
        Ok(ast) => {
            let suite = tester::run_tests(&ast);
            serde_json::to_string(&suite).unwrap_or_else(|e| {
                json!({ "name": "error", "tests": [], "passed": 0, "failed": 0, "skipped": 0, "error": e.to_string() }).to_string()
            })
        }
        Err(e) => {
            json!({
                "name": "Jenkins Pipeline Structural Tests",
                "tests": [],
                "passed": 0,
                "failed": 1,
                "skipped": 0,
                "error": format!("Parse error: {}", e)
            }).to_string()
        }
    }
}

/// Get the raw Pipeline AST as JSON string, or null on error
#[wasm_bindgen]
pub fn get_ast_json(src: &str) -> String {
    match parser::parse(src) {
        Ok(ast) => serde_json::to_string(&ast).unwrap_or_else(|_| "null".into()),
        Err(_) => "null".into(),
    }
}

/// Get stage names as a JSON string[] array
#[wasm_bindgen]
pub fn get_stage_names(src: &str) -> String {
    match parser::parse(src) {
        Ok(ast) => {
            let names = collect_names(&ast.stages);
            serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())
        }
        Err(_) => "[]".into(),
    }
}

/// Returns a high-level summary of a pipeline as JSON.
/// On parse failure returns `{ "success": false }`.
#[wasm_bindgen]
pub fn get_pipeline_summary(src: &str) -> String {
    match parser::parse(src) {
        Err(_) => serde_json::to_string(&serde_json::json!({ "success": false }))
            .unwrap_or_else(|_| r#"{"success":false}"#.into()),
        Ok(pipeline) => {
            let all_stages = ast::walk::collect_all_stages(&pipeline.stages);
            let stage_count = all_stages.len();
            let agent_type = match &pipeline.agent {
                Some(ast::Agent::Any) => "any",
                Some(ast::Agent::None) => "none",
                Some(ast::Agent::Label(_)) => "label",
                Some(ast::Agent::Docker { .. }) => "docker",
                Some(ast::Agent::Dockerfile { .. }) => "dockerfile",
                Some(ast::Agent::Node { .. }) => "node",
                Some(ast::Agent::Kubernetes { .. }) => "kubernetes",
                Some(ast::Agent::Generic { .. }) => "generic",
                None => "none",
            };
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "stage_count": stage_count,
                "has_post": pipeline.post.is_some(),
                "agent_type": agent_type,
                "parameter_count": pipeline.parameters.len(),
                "has_triggers": !pipeline.triggers.is_empty(),
                "has_environment": !pipeline.environment.is_empty(),
            })).unwrap_or_else(|_| r#"{"success":false}"#.into())
        }
    }
}

/// Return the full builtin plugin registry as a JSON string `{ "plugins": [...] }`.
/// Useful for frontends that need to enumerate available plugins.
#[wasm_bindgen]
pub fn get_builtin_registry() -> String {
    let arc = plugins::PluginRegistry::builtin_arc();
    serde_json::to_string(&*arc).unwrap_or_else(|_| "null".into())
}

/// Validate a Jenkinsfile against *only* the user-supplied registry JSON.
/// Unlike `validate_with_registry`, this does NOT merge with the builtin registry —
/// the caller's JSON is used as-is.  Returns the same structure as `validate()`.
#[wasm_bindgen]
pub fn validate_with_exact_registry(src: &str, registry_json: &str) -> String {
    let registry = match plugins::PluginRegistry::from_json(registry_json) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            return json!({
                "is_valid": false,
                "errors": [{ "code": "REGISTRY", "severity": "error", "message": e, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string();
        }
    };
    match parser::parse(src) {
        Ok(ast) => {
            let ctx = validator::context::ValidationContext::with_registry(&ast, src, registry);
            let result = validator::validate_with_context(ctx);
            serde_json::to_string(&result).unwrap_or_else(|e| {
                json!({ "is_valid": false, "errors": [e.to_string()], "warnings": [], "suggestions": [] }).to_string()
            })
        }
        Err(e) => {
            json!({
                "is_valid": false,
                "errors": [{ "code": "PARSE", "severity": "error", "message": e.message, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string()
        }
    }
}

/// Like `validate_with_exact_registry` but runs in Strict mode (W007 unknown-step
/// diagnostics are promoted to errors, making `is_valid` false).
#[wasm_bindgen]
pub fn validate_strict_with_exact_registry(src: &str, registry_json: &str) -> String {
    let registry = match plugins::PluginRegistry::from_json(registry_json) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            return json!({
                "is_valid": false,
                "errors": [{ "code": "REGISTRY", "severity": "error", "message": e, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string();
        }
    };
    match parser::parse(src) {
        Ok(ast) => {
            let ctx = validator::context::ValidationContext::with_registry(&ast, src, registry)
                .with_mode(validator::ValidationMode::Strict);
            let result = validator::validate_with_context(ctx);
            serde_json::to_string(&result).unwrap_or_else(|e| {
                json!({ "is_valid": false, "errors": [e.to_string()], "warnings": [], "suggestions": [] }).to_string()
            })
        }
        Err(e) => {
            json!({
                "is_valid": false,
                "errors": [{ "code": "PARSE", "severity": "error", "message": e.message, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string()
        }
    }
}

/// Return all validator rule metadata as a JSON array (22 rules).
/// Each entry has `code`, `severity`, and `description` fields.
/// Useful for building a help panel or rule reference UI.
#[wasm_bindgen]
pub fn get_validation_rules() -> String {
    serde_json::json!([
        { "code": "E001", "severity": "error",      "description": "No agent defined at pipeline level" },
        { "code": "E002", "severity": "error",      "description": "Stage has no steps block" },
        { "code": "E003", "severity": "error",      "description": "Duplicate stage names at the same level" },
        { "code": "E004", "severity": "error",      "description": "Duplicate parameter names" },
        { "code": "E005", "severity": "error",      "description": "Duplicate section declarations (e.g. two agent blocks)" },
        { "code": "E006", "severity": "error",      "description": "Pipeline has no stages defined" },
        { "code": "W001", "severity": "warning",    "description": "Missing global timeout option" },
        { "code": "W002", "severity": "warning",    "description": "Plaintext credential name in environment variable" },
        { "code": "W003", "severity": "warning",    "description": "Multiline shell step missing set -e" },
        { "code": "W004", "severity": "warning",    "description": "Missing post section" },
        { "code": "W005", "severity": "warning",    "description": "Hardcoded secret-like value detected" },
        { "code": "W006", "severity": "warning",    "description": "Tool type not in plugin registry" },
        { "code": "W007", "severity": "warning",    "description": "Unknown step name not in plugin registry" },
        { "code": "W008", "severity": "warning",    "description": "Agent type not in plugin registry" },
        { "code": "W009", "severity": "warning",    "description": "Pipeline option not in plugin registry" },
        { "code": "W010", "severity": "warning",    "description": "Trigger not in plugin registry" },
        { "code": "S001", "severity": "suggestion", "description": "No parameters defined" },
        { "code": "S002", "severity": "suggestion", "description": "No timeout option defined" },
        { "code": "S003", "severity": "suggestion", "description": "Long pipeline with no stages" },
        { "code": "S004", "severity": "suggestion", "description": "Deep stage nesting detected" },
        { "code": "S005", "severity": "suggestion", "description": "allOf/anyOf with fewer than 2 conditions" },
        { "code": "W011", "severity": "warning",    "description": "Credential variable referenced in double-quoted sh string — Groovy interpolation exposes the secret value in the process argument list and bypasses credential masking" }
    ]).to_string()
}

fn collect_names(stages: &[ast::Stage]) -> Vec<String> {
    ast::walk::collect_all_stages(stages)
        .into_iter()
        .map(|s| s.name.clone())
        .collect()
}
