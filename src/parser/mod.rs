use pest::Parser;
use pest_derive::Parser;
use pest::iterators::Pair;

use crate::ast::*;

#[derive(Parser)]
#[grammar = "parser/jenkinsfile.pest"]
pub struct JenkinsfileParser;

/// A structured parse error containing message text and optional source location.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<String> for ParseError {
    fn from(s: String) -> Self {
        ParseError { message: s, line: None, col: None }
    }
}

impl From<&str> for ParseError {
    fn from(s: &str) -> Self {
        ParseError { message: s.to_string(), line: None, col: None }
    }
}

/// Extract a `Location` from the start of a pest `Pair`'s span.
///
/// Returns the 1-based line and column numbers of the first character of the matched token.
fn extract_location(pair: &Pair<Rule>) -> Option<crate::ast::Location> {
    let pos = pair.as_span().start_pos();
    let (line, col) = pos.line_col();
    Some(crate::ast::Location {
        line: line as u32,
        col: col as u32,
    })
}

pub fn parse(src: &str) -> Result<Pipeline, ParseError> {
    let pairs = JenkinsfileParser::parse(Rule::pipeline, src)
        .map_err(|e| {
            let (line, col) = match e.line_col {
                pest::error::LineColLocation::Pos((l, c)) => (Some(l as u32), Some(c as u32)),
                pest::error::LineColLocation::Span((l, c), _) => (Some(l as u32), Some(c as u32)),
            };
            ParseError { message: e.to_string(), line, col }
        })?;

    let pipeline_pair = pairs
        .into_iter()
        .next()
        .ok_or("No pipeline found")?;

    build_pipeline(pipeline_pair)
}

fn build_pipeline(pair: Pair<Rule>) -> Result<Pipeline, ParseError> {
    let mut agent = None;
    let mut environment = Vec::new();
    let mut options = Vec::new();
    let mut parameters = Vec::new();
    let mut triggers = Vec::new();
    let mut tools = Vec::new();
    let mut stages = Vec::new();
    let mut post = None;
    let mut libraries = Vec::new();
    let mut duplicate_sections: Vec<String> = Vec::new();
    let mut seen_sections = std::collections::HashSet::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::pipeline_body => {
                for body_item in inner.into_inner() {
                    match body_item.as_rule() {
                        Rule::pipeline_section => {
                            for section in body_item.into_inner() {
                                let section_name = match section.as_rule() {
                                    Rule::agent_decl => Some("agent"),
                                    Rule::environment_decl => Some("environment"),
                                    Rule::options_decl => Some("options"),
                                    Rule::parameters_decl => Some("parameters"),
                                    Rule::triggers_decl => Some("triggers"),
                                    Rule::tools_decl => Some("tools"),
                                    Rule::post_decl => Some("post"),
                                    Rule::libraries_decl => Some("libraries"),
                                    _ => None,
                                };
                                if let Some(name) = section_name {
                                    if !seen_sections.insert(name) {
                                        duplicate_sections.push(name.to_string());
                                    }
                                }
                                match section.as_rule() {
                                    Rule::agent_decl => agent = Some(build_agent(section)?),
                                    Rule::environment_decl => environment = build_environment(section),
                                    Rule::options_decl => options = build_options(section),
                                    Rule::parameters_decl => parameters = build_parameters(section),
                                    Rule::triggers_decl => triggers = build_triggers(section),
                                    Rule::tools_decl => tools = build_tools(section),
                                    Rule::post_decl => post = Some(build_post(section)),
                                    Rule::libraries_decl => libraries = build_libraries(section),
                                    _ => {}
                                }
                            }
                        }
                        Rule::stages_decl => stages = build_stages(body_item)?,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    if stages.is_empty() {
        return Err(ParseError {
            message: "pipeline body must contain a stages { } block".to_string(),
            line: None,
            col: None,
        });
    }

    Ok(Pipeline { agent, environment, options, parameters, triggers, tools, stages, post, duplicate_sections, libraries })
}

fn build_agent(pair: Pair<Rule>) -> Result<Agent, String> {
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::agent_spec => {
                for spec in inner.into_inner() {
                    return match spec.as_rule() {
                        Rule::agent_any => Ok(Agent::Any),
                        Rule::agent_none => Ok(Agent::None),
                        Rule::agent_label_shorthand => {
                            let s = unquote(spec.into_inner().next().map(|p| p.as_str()).unwrap_or(""));
                            Ok(Agent::Label(s))
                        }
                        Rule::agent_block => build_agent_block(spec),
                        _ => Err(format!("Unknown agent spec: {:?}", spec.as_rule())),
                    };
                }
            }
            _ => {}
        }
    }
    Err("No agent spec".into())
}

fn build_agent_block(pair: Pair<Rule>) -> Result<Agent, String> {
    for entry in pair.into_inner() {
        match entry.as_rule() {
            Rule::agent_block_entry => {
                for inner in entry.into_inner() {
                    match inner.as_rule() {
                        Rule::agent_label_entry => {
                            let label = inner.into_inner()
                                .next()
                                .map(|p| unquote(p.as_str()))
                                .unwrap_or_default();
                            return Ok(Agent::Label(label));
                        }
                        Rule::agent_docker_entry => {
                            let mut image = String::new();
                            let mut args = None;
                            let mut custom_workspace = None;
                            let mut reuse_node = None;
                            let mut registry_url = None;
                            let mut registry_credentials_id = None;
                            let mut always_pull = None;
                            for field in inner.into_inner() {
                                for f in field.into_inner() {
                                    match f.as_rule() {
                                        Rule::docker_image => {
                                            image = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()))
                                                .unwrap_or_default();
                                        }
                                        Rule::docker_args => {
                                            args = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::docker_customWorkspace => {
                                            custom_workspace = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::docker_reuseNode => {
                                            reuse_node = f.into_inner().next()
                                                .map(|p| p.as_str() == "true");
                                        }
                                        Rule::docker_registryUrl => {
                                            registry_url = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::docker_registryCredentialsId => {
                                            registry_credentials_id = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::docker_alwaysPull => {
                                            always_pull = f.into_inner().next()
                                                .map(|p| p.as_str() == "true");
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            return Ok(Agent::Docker { image, args, custom_workspace, reuse_node, registry_url, registry_credentials_id, always_pull });
                        }
                        Rule::agent_dockerfile_entry => {
                            let mut filename = None;
                            let mut dir = None;
                            let mut additional_build_args = None;
                            let mut label = None;
                            for field in inner.into_inner() {
                                for f in field.into_inner() {
                                    match f.as_rule() {
                                        Rule::dockerfile_filename => {
                                            filename = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::dockerfile_dir => {
                                            dir = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::dockerfile_additionalBuildArgs => {
                                            additional_build_args = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        Rule::dockerfile_label => {
                                            label = f.into_inner().next()
                                                .map(|p| unquote(p.as_str()));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            return Ok(Agent::Dockerfile { filename, dir, additional_build_args, label });
                        }
                        Rule::agent_node_entry => {
                            let mut label = String::new();
                            let mut custom_workspace = None;
                            for field in inner.into_inner() {
                                match field.as_rule() {
                                    Rule::node_field => {
                                        for f in field.into_inner() {
                                            match f.as_rule() {
                                                Rule::node_label => {
                                                    label = f.into_inner().next()
                                                        .map(|p| unquote(p.as_str()))
                                                        .unwrap_or_default();
                                                }
                                                Rule::node_customWorkspace => {
                                                    custom_workspace = f.into_inner().next()
                                                        .map(|p| unquote(p.as_str()));
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            return Ok(Agent::Node { label, custom_workspace });
                        }
                        Rule::agent_kubernetes_entry => {
                            let mut yaml = None;
                            let mut yaml_file = None;
                            let mut cloud = None;
                            let mut namespace = None;
                            let mut default_container = None;
                            let mut inherit_from = None;
                            let mut retries = None;
                            let mut label = None;
                            for field in inner.into_inner() {
                                // field is a kubernetes_field node; iterate its single child
                                if let Some(f) = field.into_inner().next() {
                                    match f.as_rule() {
                                        Rule::kubernetes_yaml => {
                                            yaml = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        Rule::kubernetes_yamlFile => {
                                            yaml_file = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        Rule::kubernetes_cloud => {
                                            cloud = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        Rule::kubernetes_namespace => {
                                            namespace = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        Rule::kubernetes_defaultContainer => {
                                            default_container = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        Rule::kubernetes_inheritFrom => {
                                            inherit_from = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        Rule::kubernetes_retries => {
                                            retries = f.into_inner().next()
                                                .and_then(|p| p.as_str().parse::<u32>().ok());
                                        }
                                        Rule::kubernetes_label => {
                                            label = f.into_inner().next().map(|p| unquote(p.as_str()));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            return Ok(Agent::Kubernetes { yaml, yaml_file, cloud, namespace, default_container, inherit_from, retries, label });
                        }
                        Rule::agent_generic_entry => {
                            let agent_type = inner.into_inner()
                                .next()
                                .map(|p| p.as_str().to_string())
                                .unwrap_or_default();
                            return Ok(Agent::Generic { agent_type });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Err("Empty agent block".into())
}

fn build_environment(pair: Pair<Rule>) -> Vec<EnvVar> {
    let mut vars = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::env_entry {
            let mut inner = entry.into_inner();
            let key = inner.next().map(|p| p.as_str().to_string()).unwrap_or_default();
            let value = inner.next().map(|env_val_pair| {
                // env_val_pair.as_rule() == Rule::env_value
                // env_value = { credentials_call | env_expression }
                // Descend into the single child to distinguish the alternative.
                if let Some(child) = env_val_pair.into_inner().next() {
                    match child.as_rule() {
                        Rule::credentials_call => {
                            // Extract the inner quoted_string ID from credentials('...')
                            let id = child.into_inner()
                                .next()
                                .map(|q| unquote(q.as_str()))
                                .unwrap_or_default();
                            EnvValue::Credentials { id }
                        }
                        // env_expression captures the raw text; unquote strips
                        // outer quotes if the value is a simple quoted string.
                        _ => EnvValue::Literal(unquote(child.as_str().trim())),
                    }
                } else {
                    EnvValue::Literal(String::new())
                }
            }).unwrap_or(EnvValue::Literal(String::new()));
            vars.push(EnvVar { key, value });
        }
    }
    vars
}

fn build_options(pair: Pair<Rule>) -> Vec<crate::ast::PipelineOption> {
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::option_entry)
        .map(|p| parse_option_entry(p))
        .collect()
}

/// Extract named args from a `call_expr` pair as a map of key -> value.
fn extract_call_args(pair: Pair<Rule>) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for child in pair.into_inner() {
        if child.as_rule() == Rule::call_arg_list {
            for arg in child.into_inner() {
                if arg.as_rule() == Rule::call_arg {
                    let mut ai = arg.into_inner();
                    let first = ai.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                    if let Some(second) = ai.next() {
                        map.insert(first, unquote(second.as_str()));
                    }
                }
            }
        }
    }
    map
}

fn parse_option_entry(pair: Pair<Rule>) -> crate::ast::PipelineOption {
    let raw_text = pair.as_str().trim().to_string();
    let mut inner = pair.into_inner();
    let name = inner.next().map(|p| p.as_str().to_string()).unwrap_or_default();

    // Special handling for buildDiscarder: look for a nested call_expr
    if name == "buildDiscarder" {
        // Find call_expr inside option_args > option_arg > call_expr
        let call = inner
            .filter(|p| p.as_rule() == Rule::option_args)
            .flat_map(|args| args.into_inner())
            .filter(|p| p.as_rule() == Rule::option_arg)
            .flat_map(|arg| arg.into_inner())
            .find(|p| p.as_rule() == Rule::call_expr);

        if let Some(call_expr) = call {
            let inner_name = call_expr.clone().into_inner().next()
                .map(|p| p.as_str().to_string())
                .unwrap_or_default();
            if inner_name == "logRotator" {
                let args = extract_call_args(call_expr);
                return PipelineOption::BuildDiscarder {
                    num_to_keep: args.get("numToKeepStr").cloned(),
                    days_to_keep: args.get("daysToKeepStr").cloned(),
                    artifact_num_to_keep: args.get("artifactNumToKeepStr").cloned(),
                    artifact_days_to_keep: args.get("artifactDaysToKeepStr").cloned(),
                    raw: None,
                };
            } else {
                return PipelineOption::BuildDiscarder {
                    num_to_keep: None, days_to_keep: None,
                    artifact_num_to_keep: None, artifact_days_to_keep: None,
                    raw: Some(raw_text),
                };
            }
        }
        return PipelineOption::BuildDiscarder {
            num_to_keep: None, days_to_keep: None,
            artifact_num_to_keep: None, artifact_days_to_keep: None,
            raw: Some(raw_text),
        };
    }

    // Collect args as (key, value) pairs — skip call_expr entries (not simple values)
    let args: Vec<(Option<String>, String)> = inner
        .filter(|p| p.as_rule() == Rule::option_args)
        .flat_map(|args_pair| args_pair.into_inner())
        .filter(|p| p.as_rule() == Rule::option_arg)
        .filter_map(|arg| {
            let mut ai = arg.into_inner();
            let first = ai.next()?;
            // If the first child is a call_expr, skip (handled elsewhere)
            if first.as_rule() == Rule::call_expr {
                return None;
            }
            let first_str = first.as_str().to_string();
            if let Some(second) = ai.next() {
                Some((Some(first_str), unquote(second.as_str())))
            } else {
                Some((None, unquote(&first_str)))
            }
        })
        .collect();

    let get_arg = |key: &str| -> Option<String> {
        args.iter().find(|(k, _)| k.as_deref() == Some(key)).map(|(_, v)| v.clone())
    };

    match name.as_str() {
        "timeout" => {
            let time: u64 = get_arg("time")
                .or_else(|| args.first().filter(|(k, _)| k.is_none()).map(|(_, v)| v.clone()))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let unit = get_arg("unit").unwrap_or_else(|| "MINUTES".to_string());
            PipelineOption::Timeout { time, unit }
        }
        "buildDiscarder" => {
            // Fallback (should not reach here after early return above)
            PipelineOption::BuildDiscarder {
                num_to_keep: None, days_to_keep: None,
                artifact_num_to_keep: None, artifact_days_to_keep: None,
                raw: Some(raw_text),
            }
        }
        "retry" => {
            let count: u32 = args.first().map(|(_, v)| v.parse().unwrap_or(0)).unwrap_or(0);
            PipelineOption::Retry { count }
        }
        "disableConcurrentBuilds" => {
            let abort_previous = get_arg("abortPrevious")
                .map(|v| v == "true")
                .unwrap_or(false);
            PipelineOption::DisableConcurrentBuilds { abort_previous }
        }
        "skipDefaultCheckout" => PipelineOption::SkipDefaultCheckout,
        "skipStagesAfterUnstable" => PipelineOption::SkipStagesAfterUnstable,
        "preserveStashes" => {
            let build_count = get_arg("buildCount").and_then(|v| v.parse().ok());
            PipelineOption::PreserveStashes { build_count }
        }
        "timestamps" => PipelineOption::Timestamps,
        "parallelsAlwaysFailFast" => PipelineOption::ParallelsAlwaysFailFast,
        "newContainerPerStage" => PipelineOption::NewContainerPerStage,
        "quietPeriod" => {
            let seconds: u32 = args.first().map(|(_, v)| v.parse().unwrap_or(0)).unwrap_or(0);
            PipelineOption::QuietPeriod { seconds }
        }
        "checkoutToSubdirectory" => {
            let path = args.first().map(|(_, v)| v.clone()).unwrap_or_default();
            PipelineOption::CheckoutToSubdirectory { path }
        }
        "disableResume" => PipelineOption::DisableResume,
        "ansiColor" => {
            let colormap = args.first().map(|(_, v)| v.clone()).unwrap_or_default();
            PipelineOption::AnsiColor { colormap }
        }
        _ => PipelineOption::Raw { name: name.clone(), text: raw_text },
    }
}

fn build_parameters(pair: Pair<Rule>) -> Vec<Parameter> {
    let mut params = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::param_entry {
            let param_location = extract_location(&entry);
            let text = entry.as_str();
            let mut inner = entry.into_inner();
            let type_name = inner.next().map(|p| p.as_str().to_string()).unwrap_or_default();
            let mut name = String::new();
            let mut default_value = None;
            let mut description = None;
            let mut choices = Vec::new();

            if let Some(args) = inner.next() {
                for arg in args.into_inner() {
                    if arg.as_rule() == Rule::param_arg {
                        let mut ai = arg.into_inner();
                        let key = ai.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                        let val_node = ai.next();

                        if val_node.as_ref().map(|p| p.as_rule()) == Some(Rule::list_literal) {
                            if let Some(list) = val_node {
                                for item in list.into_inner() {
                                    choices.push(unquote(item.as_str()));
                                }
                            }
                        } else {
                            let val = val_node.map(|p| unquote(p.as_str())).unwrap_or_default();
                            match key.as_str() {
                                "name"         => name = val,
                                "defaultValue" => default_value = Some(val),
                                "description"  => description = Some(val),
                                _              => choices.push(val),
                            }
                        }
                    }
                }
            }

            let mut filter = None;
            // For `run` param: extract filter from the args
            if type_name == "run" {
                // Re-scan args for `filter` key
                if let Some(args_pair_again) = {
                    // We need to re-iterate; use the text to do a simple extraction
                    // filter was pushed to `choices` if key was unknown, but we need the actual key
                    // Instead, re-extract from text
                    None::<()>
                } {
                    let _ = args_pair_again;
                }
                // `filter` key ends up in choices since it's not name/defaultValue/description
                // We stored unknown keys in `choices` as values; need to track key-value separately
                // The current loop above only saves values for unknown keys. We need filter key.
                // This needs a different approach - handled below via raw text parsing
                filter = extract_run_filter(text);
            }

            let param = match type_name.as_str() {
                "booleanParam" => Parameter::Boolean {
                    name,
                    default_value: default_value.as_deref().map(|v| v == "true"),
                    description,
                    location: param_location,
                },
                "choice" => Parameter::Choice { name, choices, description, location: param_location },
                "text" => Parameter::Text { name, default_value, description, location: param_location },
                "password" => Parameter::Password { name, default_value, description, location: param_location },
                "file" => Parameter::File { name, description, location: param_location },
                "run" => Parameter::Run { name, description, filter, location: param_location },
                _ => Parameter::String { name, default_value, description, location: param_location },
            };
            let _ = text;
            params.push(param);
        }
    }
    params
}

fn build_triggers(pair: Pair<Rule>) -> Vec<crate::ast::Trigger> {
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::trigger_entry)
        .map(|p| build_trigger(p))
        .collect()
}

fn build_trigger(pair: Pair<Rule>) -> crate::ast::Trigger {
    let raw_text = pair.as_str().trim().to_string();
    let mut inner = pair.into_inner();
    let name = inner.next().map(|p| p.as_str().to_string()).unwrap_or_default();

    // Collect positional and named args from trigger_args
    let mut positional: Vec<String> = Vec::new();
    let mut named: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    if let Some(args_pair) = inner.next() {
        // args_pair is trigger_args = { trigger_arg ~ ("," ~ trigger_arg)* }
        // trigger_arg = { (identifier ~ ":")? ~ value }
        for arg in args_pair.into_inner() {
            if arg.as_rule() == Rule::trigger_arg {
                let mut ai = arg.into_inner();
                let first = ai.next();
                if let Some(first_pair) = first {
                    if let Some(second_pair) = ai.next() {
                        // Named arg: first is identifier (key), second is value
                        let key = first_pair.as_str().to_string();
                        let val = unquote(second_pair.as_str());
                        named.insert(key, val.clone());
                        positional.push(val);
                    } else {
                        // Positional arg: first is the value itself
                        positional.push(unquote(first_pair.as_str()));
                    }
                }
            }
        }
    }

    match name.as_str() {
        "cron" => {
            let spec = positional.into_iter().next().unwrap_or_default();
            crate::ast::Trigger::Cron { spec }
        }
        "pollSCM" => {
            let spec = positional.into_iter().next().unwrap_or_default();
            crate::ast::Trigger::PollScm { spec }
        }
        "upstream" => {
            // upstream(projects: 'job', threshold: hudson.model.Result.SUCCESS)
            // Named args are now extracted by trigger_arg grammar rule.
            // Fall back to raw text extraction for bare_word threshold values.
            let projects = named.get("projects").cloned()
                .or_else(|| positional.first().cloned())
                .unwrap_or_default();
            let threshold = named.get("threshold").cloned()
                .or_else(|| positional.get(1).cloned());
            // Also try raw text extraction for threshold (may be bare word like hudson.model.Result.SUCCESS)
            let (projects, threshold) = parse_upstream_args(&raw_text, projects, threshold);
            crate::ast::Trigger::Upstream { projects, threshold }
        }
        "githubPush" => crate::ast::Trigger::GithubPush,
        "gitlabPush" => {
            crate::ast::Trigger::GitlabPush {
                trigger_on_push: None,
                trigger_on_merge_request: None,
            }
        }
        "GenericTrigger" | "genericTrigger" => {
            crate::ast::Trigger::GenericTrigger { token: None, cause: None }
        }
        _ => crate::ast::Trigger::Raw { name: name.clone(), text: raw_text },
    }
}

fn parse_upstream_args(raw: &str, default_projects: String, default_threshold: Option<String>) -> (String, Option<String>) {
    // Try to extract projects: 'value' and threshold: value from raw text
    let mut projects = default_projects;
    let mut threshold = default_threshold;

    // Simple regex-free extraction
    if let Some(p_start) = raw.find("projects:") {
        let rest = &raw[p_start + "projects:".len()..];
        let rest = rest.trim_start();
        // Find the quoted string
        if rest.starts_with('\'') {
            if let Some(end) = rest[1..].find('\'') {
                projects = rest[1..end+1].to_string();
            }
        } else if rest.starts_with('"') {
            if let Some(end) = rest[1..].find('"') {
                projects = rest[1..end+1].to_string();
            }
        }
    }

    if let Some(t_start) = raw.find("threshold:") {
        let rest = &raw[t_start + "threshold:".len()..];
        let rest = rest.trim_start();
        // Could be a bare word like hudson.model.Result.SUCCESS
        let end = rest.find(|c: char| c == ')' || c == ',').unwrap_or(rest.len());
        threshold = Some(rest[..end].trim().to_string());
    }

    (projects, threshold)
}

fn build_tools(pair: Pair<Rule>) -> Vec<Tool> {
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::tool_entry)
        .map(|p| {
            let mut inner = p.into_inner();
            let tool_type = inner.next().map(|x| x.as_str().to_string()).unwrap_or_default();
            let name = inner.next().map(|x| unquote(x.as_str())).unwrap_or_default();
            Tool { tool_type, name }
        })
        .collect()
}

fn build_stages(pair: Pair<Rule>) -> Result<Vec<Stage>, String> {
    let mut stages = Vec::new();
    for stage_pair in pair.into_inner() {
        if stage_pair.as_rule() == Rule::stage {
            stages.push(build_stage(stage_pair)?);
        }
    }
    Ok(stages)
}

fn build_stage(pair: Pair<Rule>) -> Result<Stage, String> {
    let location = extract_location(&pair);
    let mut inner = pair.into_inner();
    let name = inner.next()
        .map(|p| unquote(p.as_str()))
        .unwrap_or_default();

    let mut agent = None;
    let mut environment = Vec::new();
    let mut when = None;
    let mut options = Vec::new();
    let mut tools = Vec::new();
    let mut input = None;
    let mut fail_fast = false;
    let mut body_steps = None;
    let mut body_parallel = None;
    let mut body_sequential = None;
    let mut body_matrix = None;
    let mut post = None;
    let mut duplicate_sections: Vec<String> = Vec::new();
    let mut seen_stage_sections = std::collections::HashSet::new();

    if let Some(body) = inner.next() {
        for item in body.into_inner() {
            match item.as_rule() {
                Rule::stage_section => {
                    for section in item.into_inner() {
                        let section_name = match section.as_rule() {
                            Rule::agent_decl => Some("agent"),
                            Rule::environment_decl => Some("environment"),
                            Rule::when_decl => Some("when"),
                            Rule::options_decl => Some("options"),
                            Rule::tools_decl => Some("tools"),
                            Rule::post_decl => Some("post"),
                            Rule::input_decl => Some("input"),
                            Rule::fail_fast_decl => Some("failFast"),
                            _ => None,
                        };
                        if let Some(name) = section_name {
                            if !seen_stage_sections.insert(name) {
                                duplicate_sections.push(name.to_string());
                            }
                        }
                        match section.as_rule() {
                            Rule::agent_decl => agent = Some(build_agent(section)?),
                            Rule::environment_decl => environment = build_environment(section),
                            Rule::when_decl => when = Some(build_when(section)),
                            Rule::options_decl => options = build_options(section),
                            Rule::tools_decl => tools = build_tools(section),
                            Rule::post_decl => post = Some(build_post(section)),
                            Rule::input_decl => input = Some(build_stage_input(section)),
                            Rule::fail_fast_decl => {
                                let val = section.into_inner()
                                    .find(|p| p.as_rule() == Rule::bool_literal)
                                    .map(|p| p.as_str() == "true")
                                    .unwrap_or(false);
                                fail_fast = val;
                            }
                            _ => {}
                        }
                    }
                }
                Rule::steps_decl => body_steps = Some(build_steps(item)),
                Rule::parallel_decl => body_parallel = Some(build_parallel(item)?),
                Rule::stages_decl => body_sequential = Some(build_stages(item)?),
                Rule::matrix_decl => body_matrix = Some(build_matrix_body(item)?),
                _ => {}
            }
        }
    }

    let body = if let Some(steps) = body_steps {
        crate::ast::StageBody::Steps(steps)
    } else if let Some(branches) = body_parallel {
        crate::ast::StageBody::Parallel { stages: branches }
    } else if let Some(matrix) = body_matrix {
        crate::ast::StageBody::Matrix(matrix)
    } else if let Some(stages) = body_sequential {
        crate::ast::StageBody::Sequential { stages }
    } else {
        // Fallback: empty steps
        crate::ast::StageBody::Steps(crate::ast::Steps { steps: vec![] })
    };

    Ok(Stage { name, location, agent, environment, when, options, tools, input, fail_fast, body, post, duplicate_sections })
}

fn build_stage_input(pair: Pair<Rule>) -> crate::ast::StageInput {
    let mut message = String::new();
    let mut ok = None;
    let mut submitter = None;
    let mut submitter_parameter = None;
    let mut parameters = Vec::new();

    for field in pair.into_inner() {
        if field.as_rule() == Rule::input_field {
            for inner in field.into_inner() {
                match inner.as_rule() {
                    Rule::input_message => {
                        message = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::string_value)
                            .map(|p| unquote(p.as_str()))
                            .unwrap_or_default();
                    }
                    Rule::input_ok => {
                        ok = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::string_value)
                            .map(|p| unquote(p.as_str()));
                    }
                    Rule::input_submitter => {
                        submitter = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::string_value)
                            .map(|p| unquote(p.as_str()));
                    }
                    Rule::input_submitter_param => {
                        submitter_parameter = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::string_value)
                            .map(|p| unquote(p.as_str()));
                    }
                    Rule::parameters_decl => {
                        parameters = build_parameters(inner);
                    }
                    _ => {}
                }
            }
        }
    }

    crate::ast::StageInput { message, ok, submitter, submitter_parameter, parameters }
}

fn build_steps(pair: Pair<Rule>) -> Steps {
    let steps = pair.into_inner()
        .filter(|p| p.as_rule() == Rule::step)
        .filter_map(|p| build_step(p))
        .collect();
    Steps { steps }
}

fn build_step(pair: Pair<Rule>) -> Option<Step> {
    for inner in pair.into_inner() {
        return match inner.as_rule() {
            Rule::sh_step => {
                let step_location = extract_location(&inner);
                let str_pair = inner.into_inner()
                    .find(|p| matches!(p.as_rule(), Rule::triple_string | Rule::quoted_string));
                let is_double_quoted = str_pair.as_ref()
                    .map(|p| p.as_str().starts_with('"'))
                    .unwrap_or(false);
                let script = str_pair.map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(Step::Sh { script, is_double_quoted, location: step_location })
            }
            Rule::echo_step => {
                let str_pair = inner.into_inner().next();
                let is_double_quoted = str_pair.as_ref()
                    .map(|p| p.as_str().starts_with('"'))
                    .unwrap_or(false);
                let message = str_pair.map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(Step::Echo { message, is_double_quoted })
            }
            Rule::script_step => {
                let body = inner.into_inner()
                    .find(|p| p.as_rule() == Rule::script_body)
                    .map(|p| p.as_str().trim().to_string())
                    .unwrap_or_default();
                Some(Step::Script { body })
            }
            Rule::checkout_step => {
                let scm = inner.into_inner()
                    .next()
                    .map(|p| p.as_str().to_string())
                    .unwrap_or_else(|| "scm".to_string());
                Some(Step::Checkout { scm })
            }
            Rule::with_credentials_step => {
                let mut bindings = Vec::new();
                let mut nested_steps = Vec::new();
                for child in inner.into_inner() {
                    match child.as_rule() {
                        Rule::binding_list => {
                            for b in child.into_inner() {
                                if b.as_rule() == Rule::binding {
                                    bindings.push(build_binding(b));
                                }
                            }
                        }
                        Rule::step => {
                            if let Some(s) = build_step(child) {
                                nested_steps.push(s);
                            }
                        }
                        _ => {}
                    }
                }
                Some(Step::WithCredentials { bindings, steps: nested_steps })
            }
            Rule::retry_step => {
                let mut children = inner.into_inner();
                let count = children.next()
                    .and_then(|p| p.as_str().parse().ok())
                    .unwrap_or(1);
                let nested: Vec<Step> = children
                    .filter(|p| p.as_rule() == Rule::step)
                    .filter_map(|p| build_step(p))
                    .collect();
                Some(Step::Retry { count, steps: nested })
            }
            Rule::timeout_step => {
                let mut time = 0u32;
                let mut unit = "MINUTES".to_string();
                let mut nested_steps = Vec::new();
                for child in inner.into_inner() {
                    match child.as_rule() {
                        Rule::timeout_args => {
                            for arg in child.into_inner() {
                                if arg.as_rule() == Rule::timeout_arg {
                                    let mut ai = arg.into_inner();
                                    let first = ai.next().map(|p| p.as_str()).unwrap_or("");
                                    if let Some(second) = ai.next() {
                                        match first {
                                            "time" => time = second.as_str().parse().unwrap_or(0),
                                            "unit" => unit = unquote(second.as_str()),
                                            _ => {}
                                        }
                                    } else {
                                        time = first.parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                        Rule::step => {
                            if let Some(s) = build_step(child) {
                                nested_steps.push(s);
                            }
                        }
                        _ => {}
                    }
                }
                Some(Step::Timeout { time, unit, steps: nested_steps })
            }
            Rule::command_call => {
                let mut ci = inner.into_inner();
                let name = ci.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                let args: Vec<String> = ci.next()
                    .map(|named| {
                        named.into_inner()
                            .filter(|p| p.as_rule() == Rule::named_arg)
                            .map(|arg| {
                                let mut ai = arg.into_inner();
                                let first = ai.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                                if let Some(second) = ai.next() {
                                    unquote(second.as_str())
                                } else {
                                    unquote(&first)
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Step::Generic { name, args })
            }
            _ => None,
        };
    }
    None
}

fn build_binding(pair: Pair<Rule>) -> crate::ast::Binding {
    let raw = pair.as_str().trim().to_string();
    let mut inner = pair.into_inner();
    let name = inner.next().map(|p| p.as_str().to_string()).unwrap_or_default();
    // Collect args
    let mut args: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(args_pair) = inner.next() {
        for arg in args_pair.into_inner() {
            if arg.as_rule() == Rule::binding_arg {
                let mut ai = arg.into_inner();
                let key = ai.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                let val = ai.next().map(|p| unquote(p.as_str())).unwrap_or_default();
                args.insert(key, val);
            }
        }
    }
    match name.as_str() {
        "usernamePassword" => {
            let credentials_id = args.get("credentialsId").cloned().unwrap_or_default();
            let username_variable = args.get("usernameVariable").cloned().unwrap_or_default();
            let password_variable = args.get("passwordVariable").cloned().unwrap_or_default();
            Binding::UsernamePassword { credentials_id, username_variable, password_variable }
        }
        "string" => {
            let credentials_id = args.get("credentialsId").cloned().unwrap_or_default();
            let variable = args.get("variable").cloned().unwrap_or_default();
            Binding::StringBinding { credentials_id, variable }
        }
        "file" => {
            let credentials_id = args.get("credentialsId").cloned().unwrap_or_default();
            let variable = args.get("variable").cloned().unwrap_or_default();
            Binding::FileBinding { credentials_id, variable }
        }
        "sshUserPrivateKey" => {
            let credentials_id = args.get("credentialsId").cloned().unwrap_or_default();
            let key_file_variable = args.get("keyFileVariable").cloned().unwrap_or_default();
            let passphrase_variable = args.get("passphraseVariable").cloned();
            Binding::SshUserPrivateKey { credentials_id, key_file_variable, passphrase_variable }
        }
        "certificate" => {
            let credentials_id = args.get("credentialsId").cloned().unwrap_or_default();
            let keystore_variable = args.get("keystoreVariable").cloned().unwrap_or_default();
            let password_variable = args.get("passwordVariable").cloned();
            Binding::Certificate { credentials_id, keystore_variable, password_variable }
        }
        _ => Binding::Raw(raw),
    }
}

fn build_parallel(pair: Pair<Rule>) -> Result<Vec<Stage>, String> {
    let mut stages = Vec::new();
    for child in pair.into_inner() {
        if child.as_rule() == Rule::stage {
            stages.push(build_stage(child)?);
        }
    }
    Ok(stages)
}

fn build_matrix_axis(pair: Pair<Rule>) -> crate::ast::MatrixAxis {
    // pair is matrix_axis_entry = { "axis" ~ "{" ~ axis_field* ~ "}" }
    let mut name = String::new();
    let mut values = Vec::new();
    for field in pair.into_inner() {
        if field.as_rule() == Rule::axis_field {
            for inner in field.into_inner() {
                match inner.as_rule() {
                    Rule::axis_name => {
                        name = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::string_value)
                            .map(|p| unquote(p.as_str()))
                            .unwrap_or_default();
                    }
                    Rule::axis_values => {
                        for v in inner.into_inner() {
                            if v.as_rule() == Rule::string_value {
                                values.push(unquote(v.as_str()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    crate::ast::MatrixAxis { name, values }
}

fn build_matrix_exclude(pair: Pair<Rule>) -> crate::ast::MatrixExclude {
    // pair is matrix_exclude_entry = { "exclude" ~ "{" ~ matrix_axis_entry* ~ "}" }
    let axes = pair.into_inner()
        .filter(|p| p.as_rule() == Rule::matrix_axis_entry)
        .map(|p| build_matrix_axis(p))
        .collect();
    crate::ast::MatrixExclude { axes }
}

fn build_matrix_body(pair: Pair<Rule>) -> Result<crate::ast::Matrix, String> {
    // pair is matrix_decl = { "matrix" ~ "{" ~ matrix_section* ~ "}" }
    let mut axes = Vec::new();
    let mut excludes = Vec::new();
    let mut stages = Vec::new();

    for section in pair.into_inner() {
        if section.as_rule() == Rule::matrix_section {
            for inner in section.into_inner() {
                match inner.as_rule() {
                    Rule::matrix_axes => {
                        for entry in inner.into_inner() {
                            if entry.as_rule() == Rule::matrix_axis_entry {
                                axes.push(build_matrix_axis(entry));
                            }
                        }
                    }
                    Rule::matrix_excludes => {
                        for entry in inner.into_inner() {
                            if entry.as_rule() == Rule::matrix_exclude_entry {
                                excludes.push(build_matrix_exclude(entry));
                            }
                        }
                    }
                    Rule::stages_decl => {
                        stages = build_stages(inner)?;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(crate::ast::Matrix { axes, excludes, stages })
}

fn build_when(pair: Pair<Rule>) -> When {
    let mut conditions = Vec::new();
    let mut before_agent = false;
    let mut before_input = false;
    let mut before_options = false;

    for field in pair.into_inner() {
        if field.as_rule() == Rule::when_field {
            for inner in field.into_inner() {
                match inner.as_rule() {
                    Rule::when_before_agent => {
                        before_agent = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::bool_literal)
                            .map(|p| p.as_str() == "true")
                            .unwrap_or(false);
                    }
                    Rule::when_before_input => {
                        before_input = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::bool_literal)
                            .map(|p| p.as_str() == "true")
                            .unwrap_or(false);
                    }
                    Rule::when_before_options => {
                        before_options = inner.into_inner()
                            .find(|p| p.as_rule() == Rule::bool_literal)
                            .map(|p| p.as_str() == "true")
                            .unwrap_or(false);
                    }
                    Rule::when_condition => {
                        if let Some(cond) = build_when_condition(inner) {
                            conditions.push(cond);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    When { conditions, before_agent, before_input, before_options }
}

fn build_when_condition(pair: Pair<Rule>) -> Option<WhenCondition> {
    for inner in pair.into_inner() {
        return match inner.as_rule() {
            Rule::when_branch => {
                let pattern = inner.into_inner().next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::Branch { pattern })
            }
            Rule::when_environment => {
                let mut ci = inner.into_inner();
                let name = ci.next().map(|p| unquote(p.as_str())).unwrap_or_default();
                let value = ci.next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::Environment { name, value })
            }
            Rule::when_expression => {
                let expr = inner.into_inner()
                    .find(|p| p.as_rule() == Rule::expression_body)
                    .map(|p| p.as_str().trim().to_string())
                    .unwrap_or_default();
                Some(WhenCondition::Expression { expr })
            }
            Rule::when_not => {
                let cond = inner.into_inner()
                    .find(|p| p.as_rule() == Rule::when_condition)
                    .and_then(|p| build_when_condition(p))?;
                Some(WhenCondition::Not { condition: Box::new(cond) })
            }
            Rule::when_allOf => {
                let conditions = inner.into_inner()
                    .filter(|p| p.as_rule() == Rule::when_condition)
                    .filter_map(|p| build_when_condition(p))
                    .collect();
                Some(WhenCondition::AllOf { conditions })
            }
            Rule::when_anyOf => {
                let conditions = inner.into_inner()
                    .filter(|p| p.as_rule() == Rule::when_condition)
                    .filter_map(|p| build_when_condition(p))
                    .collect();
                Some(WhenCondition::AnyOf { conditions })
            }
            Rule::when_tag => {
                let pattern = inner.into_inner().next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::Tag { pattern })
            }
            Rule::when_changeRequest => Some(WhenCondition::ChangeRequest),
            Rule::when_buildingTag => Some(WhenCondition::BuildingTag),
            Rule::when_changelog => {
                let pattern = inner.into_inner().next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::Changelog { pattern })
            }
            Rule::when_changeset => {
                let pattern = inner.into_inner().next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::Changeset { pattern })
            }
            Rule::when_triggeredBy => {
                let cause = inner.into_inner().next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::TriggeredBy { cause })
            }
            Rule::when_equals => {
                let mut ci = inner.into_inner();
                let expected = ci.next().map(|p| unquote(p.as_str())).unwrap_or_default();
                let actual = ci.next().map(|p| unquote(p.as_str())).unwrap_or_default();
                Some(WhenCondition::Equals { actual, expected })
            }
            Rule::when_generic => {
                let mut ci = inner.into_inner();
                let name = ci.next().map(|p| p.as_str().to_string()).unwrap_or_default();
                let args: Vec<String> = ci
                    .filter(|p| p.as_rule() == Rule::quoted_string)
                    .map(|p| unquote(p.as_str()))
                    .collect();
                Some(WhenCondition::Generic { name, args })
            }
            _ => {
                // Catch-all: preserve unknown when conditions to prevent silent data loss
                let name = inner.as_str().trim().to_string();
                Some(WhenCondition::Generic { name, args: vec![] })
            }
        };
    }
    None
}

fn build_post(pair: Pair<Rule>) -> Post {
    let mut post = Post {
        always: None, success: None, failure: None, unstable: None,
        aborted: None, changed: None, cleanup: None, regression: None, fixed: None,
        unsuccessful: None,
    };
    for section in pair.into_inner() {
        if section.as_rule() == Rule::post_section {
            let mut si = section.into_inner();
            let condition = si.next().map(|p| p.as_str().to_string()).unwrap_or_default();
            let steps_list: Vec<Step> = si
                .filter(|p| p.as_rule() == Rule::step)
                .filter_map(|p| build_step(p))
                .collect();
            let steps = Steps { steps: steps_list };
            match condition.as_str() {
                "always" => post.always = Some(steps),
                "success" => post.success = Some(steps),
                "failure" => post.failure = Some(steps),
                "unstable" => post.unstable = Some(steps),
                "aborted" => post.aborted = Some(steps),
                "changed" => post.changed = Some(steps),
                "cleanup" => post.cleanup = Some(steps),
                "regression" => post.regression = Some(steps),
                "fixed" => post.fixed = Some(steps),
                "unsuccessful" => post.unsuccessful = Some(steps),
                _ => {}
            }
        }
    }
    post
}

fn extract_run_filter(text: &str) -> Option<String> {
    // Simple text extraction for filter: 'value' or filter: "value"
    if let Some(pos) = text.find("filter:") {
        let rest = text[pos + "filter:".len()..].trim_start();
        if rest.starts_with('\'') {
            if let Some(end) = rest[1..].find('\'') {
                return Some(rest[1..end+1].to_string());
            }
        } else if rest.starts_with('"') {
            if let Some(end) = rest[1..].find('"') {
                return Some(rest[1..end+1].to_string());
            }
        }
    }
    None
}

fn build_libraries(pair: Pair<Rule>) -> Vec<crate::ast::SharedLibrary> {
    let mut libs = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::lib_entry {
            if let Some(str_pair) = entry.into_inner().next() {
                let raw = unquote(str_pair.as_str());
                // Split on '@' to get name and optional ref
                if let Some(at_pos) = raw.find('@') {
                    let name = raw[..at_pos].to_string();
                    let ref_ = Some(raw[at_pos+1..].to_string());
                    libs.push(crate::ast::SharedLibrary { name, ref_ });
                } else {
                    libs.push(crate::ast::SharedLibrary { name: raw, ref_: None });
                }
            }
        }
    }
    libs
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 6 {
        if (s.starts_with("'''") && s.ends_with("'''")) ||
           (s.starts_with("\"\"\"") && s.ends_with("\"\"\"")) {
            return s[3..s.len()-3].to_string();
        }
    }
    if s.len() >= 2 {
        if (s.starts_with('\'') && s.ends_with('\'')) ||
           (s.starts_with('"') && s.ends_with('"')) {
            return s[1..s.len()-1].to_string();
        }
    }
    s.to_string()
}
