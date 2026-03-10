// CLI binary for jenkinsfile-tester — compiles to wasm32-wasi for use with wasmtime.
// Declares its own module tree so this binary is independent of lib.rs and wasm-bindgen.

mod ast;
mod parser;
mod plugins;
mod validator;
mod tester;

use std::io::{self, Read};
use std::process;
use std::sync::Arc;
use serde_json::json;

const USAGE: &str = "\
Usage: jenkinsfile-tester [--registry FILE] <command> [file]

Commands:
  validate        Validate pipeline — errors, warnings, suggestions (JSON)
  validate-strict Validate in strict mode — unknown steps are errors (JSON)
  run-tests       Run 22 structural assertions (JSON)
  parse           Parse and return the full AST (JSON)
  summary         High-level pipeline summary (JSON)
  stage-names     List all stage names (JSON array)
  unknown-steps   List step names not found in the registry (JSON array)
  dump-registry   Print the builtin plugin registry as JSON

Arguments:
  --registry FILE  Use a custom plugin registry JSON file instead of builtins
  [file]           Path to a Jenkinsfile.  Omit (or use -) to read from stdin.

Exit codes:
  0  Valid / all tests pass
  1  Invalid / test failures / parse error
  2  Usage error";

fn main() {
    // Strip any bare "--" separators — wasmtime passes them through to the WASM module unchanged.
    let args: Vec<String> = std::env::args().filter(|a| a != "--").collect();

    let (cmd, src, registry_path) = match parse_args(&args) {
        Ok(x) => x,
        Err(msg) => {
            eprintln!("Error: {}\n\n{}", msg, USAGE);
            process::exit(2);
        }
    };

    let registry: Arc<plugins::PluginRegistry> = match registry_path {
        Some(path) => {
            match std::fs::read_to_string(&path) {
                Ok(json) => match plugins::PluginRegistry::from_json(&json) {
                    Ok(r) => Arc::new(r),
                    Err(e) => {
                        eprintln!("Error: invalid registry '{}': {}", path, e);
                        process::exit(2);
                    }
                },
                Err(e) => {
                    eprintln!("Error: cannot read registry '{}': {}", path, e);
                    process::exit(2);
                }
            }
        }
        None => plugins::PluginRegistry::builtin_arc(),
    };

    process::exit(run_command(&cmd, &src, registry));
}

fn parse_args(args: &[String]) -> Result<(String, String, Option<String>), String> {
    if args.len() < 2 {
        return Err("No command specified.".into());
    }

    let mut registry_path: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--registry" {
            i += 1;
            if i >= args.len() {
                return Err("--registry requires a file path argument.".into());
            }
            registry_path = Some(args[i].clone());
        } else {
            positional.push(args[i].clone());
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("No command specified.".into());
    }

    let cmd = positional[0].clone();

    // dump-registry needs no source input
    if cmd == "dump-registry" {
        return Ok((cmd, String::new(), registry_path));
    }

    let src = if positional.len() >= 2 && positional[1] != "-" {
        std::fs::read_to_string(&positional[1])
            .map_err(|e| format!("Cannot read '{}': {}", positional[1], e))?
    } else {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)
            .map_err(|e| format!("Cannot read stdin: {}", e))?;
        buf
    };
    Ok((cmd, src, registry_path))
}

fn run_command(cmd: &str, src: &str, registry: Arc<plugins::PluginRegistry>) -> i32 {
    match cmd {
        "validate" => {
            let (json_out, valid) = do_validate(src, false, registry);
            println!("{}", json_out);
            if valid { 0 } else { 1 }
        }
        "validate-strict" => {
            let (json_out, valid) = do_validate(src, true, registry);
            println!("{}", json_out);
            if valid { 0 } else { 1 }
        }
        "run-tests" => {
            match parser::parse(src) {
                Ok(ast) => {
                    let suite = tester::run_tests(&ast);
                    println!("{}", serde_json::to_string_pretty(&suite).unwrap_or_else(|_| "null".into()));
                    if suite.failed == 0 { 0 } else { 1 }
                }
                Err(e) => {
                    println!("{}", json!({
                        "name": "Jenkins Pipeline Structural Tests",
                        "tests": [],
                        "passed": 0,
                        "failed": 1,
                        "skipped": 0,
                        "error": format!("Parse error: {}", e.message)
                    }));
                    1
                }
            }
        }
        "parse" => {
            match parser::parse(src) {
                Ok(ast) => {
                    println!("{}", serde_json::to_string_pretty(&ast).unwrap_or_else(|_| "null".into()));
                    0
                }
                Err(e) => {
                    println!("{}", json!({
                        "success": false,
                        "error": { "message": e.message, "line": e.line, "col": e.col }
                    }));
                    1
                }
            }
        }
        "summary" => {
            match parser::parse(src) {
                Ok(pipeline) => {
                    let all_stages = ast::walk::collect_all_stages(&pipeline.stages);
                    let agent_type = match &pipeline.agent {
                        Some(ast::Agent::Any)          => "any",
                        Some(ast::Agent::None)         => "none",
                        Some(ast::Agent::Label(_))     => "label",
                        Some(ast::Agent::Docker { .. }) => "docker",
                        Some(ast::Agent::Dockerfile { .. }) => "dockerfile",
                        Some(ast::Agent::Node { .. })  => "node",
                        Some(ast::Agent::Kubernetes { .. }) => "kubernetes",
                        Some(ast::Agent::Generic { .. }) => "generic",
                        std::option::Option::None      => "none",
                    };
                    println!("{}", json!({
                        "success": true,
                        "stage_count": all_stages.len(),
                        "has_post": pipeline.post.is_some(),
                        "agent_type": agent_type,
                        "parameter_count": pipeline.parameters.len(),
                        "has_triggers": !pipeline.triggers.is_empty(),
                        "has_environment": !pipeline.environment.is_empty(),
                    }));
                    0
                }
                Err(e) => {
                    println!("{}", json!({ "success": false, "error": e.message }));
                    1
                }
            }
        }
        "stage-names" => {
            match parser::parse(src) {
                Ok(ast) => {
                    let names: Vec<String> = ast::walk::collect_all_stages(&ast.stages)
                        .into_iter()
                        .map(|s| s.name.clone())
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&names).unwrap_or_else(|_| "[]".into()));
                    0
                }
                Err(e) => {
                    println!("{}", json!({ "error": e.message }));
                    1
                }
            }
        }
        "unknown-steps" => {
            match parser::parse(src) {
                Ok(ast) => {
                    let mut unknown: Vec<String> = ast::walk::walk_steps_with_stage(&ast.stages)
                        .into_iter()
                        .filter_map(|(_, step)| {
                            if let ast::Step::Generic { name, .. } = step {
                                if !registry.has_step(name) {
                                    return Some(name.clone());
                                }
                            }
                            std::option::Option::None
                        })
                        .collect();
                    unknown.sort();
                    unknown.dedup();
                    println!("{}", serde_json::to_string_pretty(&unknown).unwrap_or_else(|_| "[]".into()));
                    0
                }
                Err(e) => {
                    println!("{}", json!({ "error": e.message }));
                    1
                }
            }
        }
        "dump-registry" => {
            let builtin = plugins::PluginRegistry::builtin_arc();
            println!("{}", serde_json::to_string_pretty(&*builtin).unwrap());
            0
        }
        _ => {
            eprintln!("Unknown command: '{}'\n\n{}", cmd, USAGE);
            2
        }
    }
}

fn do_validate(src: &str, strict: bool, registry: Arc<plugins::PluginRegistry>) -> (String, bool) {
    match parser::parse(src) {
        Ok(ast) => {
            let ctx = if strict {
                validator::context::ValidationContext::with_registry(&ast, src, registry)
                    .with_mode(validator::ValidationMode::Strict)
            } else {
                validator::context::ValidationContext::with_registry(&ast, src, registry)
            };
            let result = validator::validate_with_context(ctx);
            let valid = result.is_valid;
            (serde_json::to_string_pretty(&result).unwrap_or_else(|_| "null".into()), valid)
        }
        Err(e) => {
            let out = json!({
                "is_valid": false,
                "errors": [{ "code": "PARSE", "severity": "error", "message": e.message, "location": null }],
                "warnings": [],
                "suggestions": []
            }).to_string();
            (out, false)
        }
    }
}
