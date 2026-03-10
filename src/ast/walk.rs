//! Shared AST traversal helpers used across validator, tester, and lib.

use super::{EnvValue, Stage, StageBody, Step, Steps};

/// Recursively collects all stages from a slice, including nested parallel
/// and sequential stages. The returned vec includes each stage at every level.
pub fn collect_all_stages(stages: &[Stage]) -> Vec<&Stage> {
    let mut out = Vec::new();
    for stage in stages {
        collect_stage_recursive(stage, &mut out);
    }
    out
}

fn collect_stage_recursive<'a>(stage: &'a Stage, out: &mut Vec<&'a Stage>) {
    out.push(stage);
    match &stage.body {
        StageBody::Steps(_) => {}
        StageBody::Parallel { stages: branches } | StageBody::Sequential { stages: branches } => {
            for s in branches {
                collect_stage_recursive(s, out);
            }
        }
        StageBody::Matrix(matrix) => {
            for s in &matrix.stages {
                collect_stage_recursive(s, out);
            }
        }
    }
}

/// Collects all steps from a stage's steps block (if present).
#[allow(dead_code)]
pub fn collect_all_steps(stage: &Stage) -> Vec<&Step> {
    match &stage.body {
        StageBody::Steps(s) => s.steps.iter().collect(),
        _ => Vec::new(),
    }
}

/// Helper: get steps from stage body if it's a Steps variant
pub fn stage_steps(stage: &Stage) -> Option<&Steps> {
    match &stage.body {
        StageBody::Steps(s) => Some(s),
        _ => None,
    }
}

/// Helper: get parallel branches from stage body if it's a Parallel variant
#[allow(dead_code)]
pub fn stage_parallel(stage: &Stage) -> Option<&Vec<Stage>> {
    match &stage.body {
        StageBody::Parallel { stages: branches } => Some(branches),
        _ => None,
    }
}

/// Helper: get sequential stages from stage body if it's a Sequential variant
#[allow(dead_code)]
pub fn stage_sequential(stage: &Stage) -> Option<&Vec<Stage>> {
    match &stage.body {
        StageBody::Sequential { stages } => Some(stages),
        _ => None,
    }
}

/// Recursively collect all steps from a slice of steps, descending into
/// WithCredentials, Retry, and Timeout body blocks.
pub fn collect_all_steps_recursive(steps: &[Step]) -> Vec<&Step> {
    let mut result = Vec::new();
    for step in steps {
        result.push(step);
        match step {
            Step::WithCredentials { steps: body, .. } => {
                result.extend(collect_all_steps_recursive(body));
            }
            Step::Retry { steps: body, .. } => {
                result.extend(collect_all_steps_recursive(body));
            }
            Step::Timeout { steps: body, .. } => {
                result.extend(collect_all_steps_recursive(body));
            }
            _ => {}
        }
    }
    result
}

/// Yields (stage, step) pairs for all steps in all stages recursively.
pub fn walk_steps_with_stage<'a>(stages: &'a [Stage]) -> Vec<(&'a Stage, &'a Step)> {
    let mut result = Vec::new();
    for stage in collect_all_stages(stages) {
        if let Some(steps_block) = stage_steps(stage) {
            for step in collect_all_steps_recursive(&steps_block.steps) {
                result.push((stage, step));
            }
        }
    }
    result
}

#[allow(dead_code)]
/// Returns the effective environment variables for a given stage,
/// combining pipeline-level env (outer) with stage-level env (inner wins on conflict).
/// Stage-level values override pipeline-level values for the same key.
pub fn collect_env_vars_for_stage<'a>(
    stage: &'a crate::ast::Stage,
    pipeline: &'a crate::ast::Pipeline,
) -> std::collections::HashMap<&'a str, &'a EnvValue> {
    let mut map = std::collections::HashMap::new();
    // Pipeline-level env first (lower priority)
    for env_var in &pipeline.environment {
        map.insert(env_var.key.as_str(), &env_var.value);
    }
    // Stage-level env overrides pipeline-level
    for env_var in &stage.environment {
        map.insert(env_var.key.as_str(), &env_var.value);
    }
    map
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Step, Steps, StageBody, Stage};

    fn make_stage(name: &str, steps: Vec<Step>) -> Stage {
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
            body: StageBody::Steps(Steps { steps }),
            post: None,
            duplicate_sections: vec![],
        }
    }

    // ── collect_all_stages ───────────────────────────────────────────────────

    /// TGAP-016: Empty slice returns empty vec (named alias for gap tracking).
    #[test]
    fn collect_all_stages_empty_slice() {
        let result = collect_all_stages(&[]);
        assert!(result.is_empty());
    }

    /// Empty slice returns empty vec.
    #[test]
    fn tgap015_collect_all_stages_empty_slice() {
        let stages: Vec<Stage> = vec![];
        assert!(collect_all_stages(&stages).is_empty());
    }

    /// Flat stages (no nesting) returns all stages.
    #[test]
    fn tgap015_collect_all_stages_flat() {
        let stages = vec![
            make_stage("A", vec![]),
            make_stage("B", vec![]),
        ];
        let result = collect_all_stages(&stages);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "A");
        assert_eq!(result[1].name, "B");
    }

    /// Parallel container stage: container + branches are all returned.
    #[test]
    fn tgap015_collect_all_stages_parallel_container_with_no_steps() {
        let branch_a = make_stage("Branch A", vec![]);
        let branch_b = make_stage("Branch B", vec![]);
        let container = Stage {
            name: "Test".to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![branch_a, branch_b] },
            post: None,
            duplicate_sections: vec![],
        };
        let stages = [container];
        let result = collect_all_stages(&stages);
        // Container + 2 branches = 3 stages
        assert_eq!(result.len(), 3);
        let names: Vec<&str> = result.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Test"));
        assert!(names.contains(&"Branch A"));
        assert!(names.contains(&"Branch B"));
    }

    /// Deep nesting: sequential inside parallel is fully traversed.
    #[test]
    fn tgap015_collect_all_stages_deep_nesting() {
        let leaf = make_stage("Leaf", vec![]);
        let sequential = Stage {
            name: "Sequential".to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Sequential { stages: vec![leaf] },
            post: None,
            duplicate_sections: vec![],
        };
        let parallel = Stage {
            name: "Parallel".to_string(),
            location: None,
            agent: None,
            environment: vec![],
            when: None,
            options: vec![],
            tools: vec![],
            input: None,
            fail_fast: false,
            body: StageBody::Parallel { stages: vec![sequential] },
            post: None,
            duplicate_sections: vec![],
        };
        let stages = [parallel];
        let result = collect_all_stages(&stages);
        assert_eq!(result.len(), 3, "expected Parallel + Sequential + Leaf");
    }

    // ── collect_all_steps_recursive ──────────────────────────────────────────

    /// Empty slice returns empty vec.
    #[test]
    fn tgap015_collect_all_steps_recursive_empty() {
        let steps: Vec<Step> = vec![];
        assert!(collect_all_steps_recursive(&steps).is_empty());
    }

    /// Flat steps: all steps returned without recursion.
    #[test]
    fn tgap015_collect_all_steps_recursive_flat() {
        let steps = vec![
            Step::Echo { message: "hi".into(), is_double_quoted: false },
            Step::Sh { script: "make".into(), is_double_quoted: false, location: None },
        ];
        let result = collect_all_steps_recursive(&steps);
        assert_eq!(result.len(), 2);
    }

    /// WithCredentials body is recursed into.
    #[test]
    fn tgap015_collect_all_steps_recursive_with_credentials() {
        let inner = Step::Sh { script: "curl".into(), is_double_quoted: false, location: None };
        let outer = Step::WithCredentials {
            bindings: vec![],
            steps: vec![inner],
        };
        let steps = [outer];
        let result = collect_all_steps_recursive(&steps);
        // outer WithCredentials + inner Sh = 2
        assert_eq!(result.len(), 2);
        assert!(matches!(result[1], Step::Sh { .. }));
    }

    // ── collect_env_vars_for_stage ───────────────────────────────────────────

    fn make_pipeline_with_env(env: Vec<(String, crate::ast::EnvValue)>) -> crate::ast::Pipeline {
        use crate::ast::{EnvVar, Pipeline};
        Pipeline {
            agent: None,
            environment: env.into_iter().map(|(k, v)| EnvVar { key: k, value: v }).collect(),
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

    /// Pipeline env `FOO=1` + stage env `BAR=2` → result has both.
    #[test]
    fn collect_env_vars_for_stage_merges_pipeline_and_stage_env() {
        use crate::ast::{EnvValue, EnvVar};
        let pipeline = make_pipeline_with_env(vec![
            ("FOO".into(), EnvValue::Literal("1".into())),
        ]);
        let mut stage = make_stage("Build", vec![]);
        stage.environment.push(EnvVar {
            key: "BAR".into(),
            value: EnvValue::Literal("2".into()),
        });
        let map = collect_env_vars_for_stage(&stage, &pipeline);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("FOO"));
        assert!(map.contains_key("BAR"));
    }

    /// Pipeline env `FOO=1` + stage env `FOO=2` → result has `FOO=2` (stage wins).
    #[test]
    fn collect_env_vars_for_stage_stage_overrides_pipeline() {
        use crate::ast::{EnvValue, EnvVar};
        let pipeline = make_pipeline_with_env(vec![
            ("FOO".into(), EnvValue::Literal("1".into())),
        ]);
        let mut stage = make_stage("Build", vec![]);
        stage.environment.push(EnvVar {
            key: "FOO".into(),
            value: EnvValue::Literal("2".into()),
        });
        let map = collect_env_vars_for_stage(&stage, &pipeline);
        assert_eq!(map.len(), 1);
        if let EnvValue::Literal(v) = map["FOO"] {
            assert_eq!(v, "2");
        } else {
            panic!("expected Literal");
        }
    }
}
