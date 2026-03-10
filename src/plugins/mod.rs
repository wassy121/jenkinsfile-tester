// Plugin registry module
//
// Registry JSON schema:
// {
//   "plugins": [
//     {
//       "plugin_id": "kubernetes",
//       "version": "1.29.0",
//       "contributes": {
//         "steps": [],
//         "agent_types": ["kubernetes"],
//         "options": [],
//         "triggers": [],
//         "tools": [],
//         "when_conditions": []
//       }
//     }
//   ]
// }

use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

/// Argument metadata for a plugin-contributed step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepArg {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub arg_type: Option<String>,
}

/// A plugin-contributed step with optional argument metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<StepArg>,
}

/// Serde helper for backwards-compatible step deserialization:
/// accepts both plain strings ("sh") and full objects ({ "name": "sh", ... })
#[derive(Deserialize)]
#[serde(untagged)]
enum StepEntryInput {
    Name(String),
    Full(StepEntry),
}

impl From<StepEntryInput> for StepEntry {
    fn from(input: StepEntryInput) -> Self {
        match input {
            StepEntryInput::Name(s) => StepEntry { name: s, args: vec![] },
            StepEntryInput::Full(e) => e,
        }
    }
}

fn deserialize_steps<'de, D>(deserializer: D) -> Result<Vec<StepEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let inputs: Vec<StepEntryInput> = Vec::deserialize(deserializer)?;
    Ok(inputs.into_iter().map(StepEntry::from).collect())
}

/// What a single plugin version contributes to the DSL
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginContributions {
    #[serde(deserialize_with = "deserialize_steps", default)]
    pub steps: Vec<StepEntry>,
    pub agent_types: Vec<String>,
    pub options: Vec<String>,
    pub triggers: Vec<String>,
    pub tools: Vec<String>,
    pub when_conditions: Vec<String>,
}

/// A single plugin entry in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub plugin_id: String,
    pub version: String,
    /// Minimum plugin version this entry applies to (inclusive). `None` means no lower bound.
    #[serde(default)]
    pub min_version: Option<String>,
    /// Maximum plugin version this entry applies to (inclusive). `None` means no upper bound.
    #[serde(default)]
    pub max_version: Option<String>,
    pub contributes: PluginContributions,
}

/// In-memory plugin registry — the union of all loaded plugins
#[derive(Debug, Clone, Default, Serialize)]
pub struct PluginRegistry {
    pub plugins: Vec<PluginEntry>,
}

// Serde helper struct for deserializing the registry JSON wrapper
#[derive(Deserialize)]
struct RegistryJson {
    plugins: Vec<PluginEntry>,
}

#[allow(dead_code)]
impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a JSON string into a PluginRegistry
    pub fn from_json(json: &str) -> Result<PluginRegistry, String> {
        let parsed: RegistryJson = serde_json::from_str(json)
            .map_err(|e| format!("Failed to parse plugin registry JSON: {}", e))?;
        Ok(PluginRegistry { plugins: parsed.plugins })
    }

    /// Append another registry's plugins into self
    pub fn merge(&mut self, other: PluginRegistry) {
        self.plugins.extend(other.plugins);
    }

    /// Returns true if any loaded plugin contributes the given tool type
    pub fn has_tool(&self, tool_type: &str) -> bool {
        let lower = tool_type.to_lowercase();
        self.plugins.iter().any(|p| {
            p.contributes.tools.iter().any(|t| t.to_lowercase() == lower)
        })
    }

    /// Returns true if any loaded plugin contributes the given agent type name
    pub fn has_agent_type(&self, agent_type: &str) -> bool {
        self.plugins.iter().any(|p| {
            p.contributes.agent_types.iter().any(|a| a == agent_type)
        })
    }

    /// Returns true if any loaded plugin contributes the given step name
    pub fn has_step(&self, step_name: &str) -> bool {
        self.plugins.iter().any(|p| {
            p.contributes.steps.iter().any(|s| s.name == step_name)
        })
    }

    /// Returns all contributed steps across all loaded plugins
    pub fn all_steps(&self) -> Vec<&StepEntry> {
        self.plugins.iter()
            .flat_map(|p| p.contributes.steps.iter())
            .collect()
    }

    /// Returns true if any loaded plugin contributes the given option name
    pub fn has_option(&self, option_name: &str) -> bool {
        self.plugins.iter().any(|p| {
            p.contributes.options.iter().any(|o| o == option_name)
        })
    }

    /// Returns true if any loaded plugin contributes the given trigger name
    pub fn has_trigger(&self, trigger_name: &str) -> bool {
        self.plugins.iter().any(|p| {
            p.contributes.triggers.iter().any(|t| t == trigger_name)
        })
    }

    /// Returns all contributed tool types across all loaded plugins
    pub fn all_tools(&self) -> Vec<&str> {
        self.plugins.iter()
            .flat_map(|p| p.contributes.tools.iter().map(|t| t.as_str()))
            .collect()
    }

    /// Returns a shared `Arc<PluginRegistry>` pre-loaded with the built-in plugin
    /// definitions. The JSON is parsed exactly once for the lifetime of the process.
    pub fn builtin_arc() -> Arc<PluginRegistry> {
        static BUILTIN: OnceLock<Arc<PluginRegistry>> = OnceLock::new();
        Arc::clone(BUILTIN.get_or_init(|| {
            Arc::new(
                PluginRegistry::from_json(BUILTIN_REGISTRY_JSON)
                    .expect("Built-in plugin registry JSON is invalid — this is a programmer error"),
            )
        }))
    }

    /// Returns a clone of the built-in plugin registry.
    /// Prefer `builtin_arc()` when sharing across contexts to avoid cloning.
    pub fn builtin() -> PluginRegistry {
        (*Self::builtin_arc()).clone()
    }
}

/// Bundled built-in registry covering common Jenkins plugins
pub static BUILTIN_REGISTRY_JSON: &str = r#"{
  "plugins": [
    {
      "plugin_id": "kubernetes",
      "version": "1.29.0",
      "contributes": {
        "steps": [],
        "agent_types": ["kubernetes"],
        "options": [],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "docker",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": ["docker", "dockerfile"],
        "options": [],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "nodejs",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["nodejs", "node"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "golang",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["go", "golang"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "gradle",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["gradle"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "maven",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["maven"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "ant",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["ant"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "git",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["git"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "jdk-tool",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": ["jdk", "java"],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "timestamper",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": ["timestamps"],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "ansicolor",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": ["ansiColor"],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "github",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": ["githubPush"],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "gitlab",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": ["gitlab"],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "generic-webhook-trigger",
      "version": "1.0.0",
      "contributes": {
        "steps": [],
        "agent_types": [],
        "options": [],
        "triggers": ["GenericTrigger"],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "slack",
      "version": "1.0.0",
      "contributes": {
        "steps": [{ "name": "slackSend" }],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "sonarqube",
      "version": "1.0.0",
      "contributes": {
        "steps": [{ "name": "withSonarQubeEnv" }, { "name": "waitForQualityGate" }],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "xunit",
      "version": "1.0.0",
      "contributes": {
        "steps": [{ "name": "xunit" }],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    },
    {
      "plugin_id": "workspace-cleanup",
      "version": "1.0.0",
      "contributes": {
        "steps": [{ "name": "cleanWs" }, { "name": "deleteDir" }],
        "agent_types": [],
        "options": [],
        "triggers": [],
        "tools": [],
        "when_conditions": []
      }
    }
  ]
}"#;

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_json_parses_valid_registry() {
        let json = r#"{
            "plugins": [
                {
                    "plugin_id": "kubernetes",
                    "version": "1.29.0",
                    "contributes": {
                        "steps": [],
                        "agent_types": ["kubernetes"],
                        "options": [],
                        "triggers": [],
                        "tools": [],
                        "when_conditions": []
                    }
                }
            ]
        }"#;
        let registry = PluginRegistry::from_json(json);
        assert!(registry.is_ok(), "expected Ok, got: {:?}", registry);
        let registry = registry.unwrap();
        assert_eq!(registry.plugins.len(), 1);
        assert_eq!(registry.plugins[0].plugin_id, "kubernetes");
    }

    #[test]
    fn from_json_returns_err_for_invalid_json() {
        let result = PluginRegistry::from_json("not valid json {{{");
        assert!(result.is_err(), "expected Err for invalid JSON");
    }

    #[test]
    fn has_tool_returns_true_for_contributed_tool() {
        let json = r#"{
            "plugins": [
                {
                    "plugin_id": "nodejs",
                    "version": "1.0.0",
                    "contributes": {
                        "steps": [],
                        "agent_types": [],
                        "options": [],
                        "triggers": [],
                        "tools": ["nodejs", "node"],
                        "when_conditions": []
                    }
                }
            ]
        }"#;
        let registry = PluginRegistry::from_json(json).unwrap();
        assert!(registry.has_tool("nodejs"));
        assert!(registry.has_tool("node"));
        assert!(!registry.has_tool("maven"));
    }

    #[test]
    fn builtin_has_nodejs_tool() {
        let registry = PluginRegistry::builtin();
        assert!(registry.has_tool("nodejs"), "builtin registry should have nodejs tool");
        assert!(!registry.has_tool("unknowntool"), "builtin registry should not have unknowntool");
    }

    #[test]
    fn merge_combines_plugins() {
        let json_a = r#"{"plugins": [{"plugin_id": "a", "version": "1.0.0", "contributes": {"steps": [], "agent_types": [], "options": [], "triggers": [], "tools": ["toolA"], "when_conditions": []}}]}"#;
        let json_b = r#"{"plugins": [{"plugin_id": "b", "version": "1.0.0", "contributes": {"steps": [], "agent_types": [], "options": [], "triggers": [], "tools": ["toolB"], "when_conditions": []}}]}"#;
        let mut reg_a = PluginRegistry::from_json(json_a).unwrap();
        let reg_b = PluginRegistry::from_json(json_b).unwrap();
        reg_a.merge(reg_b);
        assert!(reg_a.has_tool("toolA"));
        assert!(reg_a.has_tool("toolB"));
    }

    // ── TGAP-004: from_json error paths ──────────────────────────────────────

    /// Totally invalid JSON must fail.
    #[test]
    fn tgap004_from_json_returns_err_for_missing_required_fields() {
        // Missing `version` field — serde requires it (not Option<String>)
        let json = r#"{"plugins": [{"plugin_id": "bad", "contributes": {}}]}"#;
        let result = PluginRegistry::from_json(json);
        // `version` is a required String field, so deserialization must fail
        assert!(result.is_err(), "expected Err for missing version, got: {:?}", result);
    }

    /// Null entry in plugins array must fail.
    #[test]
    fn tgap004_from_json_returns_err_for_null_plugin_entry() {
        let json = r#"{"plugins": [null]}"#;
        let result = PluginRegistry::from_json(json);
        assert!(result.is_err(), "expected Err for null plugin entry");
    }

    /// Missing `contributes` field (required, no #[serde(default)]) must fail.
    #[test]
    fn tgap004_from_json_returns_err_for_missing_contributes() {
        let json = r#"{"plugins": [{"plugin_id": "bad", "version": "1.0.0"}]}"#;
        let result = PluginRegistry::from_json(json);
        assert!(result.is_err(), "expected Err for missing contributes field, got: {:?}", result);
    }

    // ── TGAP-006: has_step, has_option, has_trigger ───────────────────────────

    fn simple_registry(steps: &[&str], options: &[&str], triggers: &[&str]) -> PluginRegistry {
        let steps_json: Vec<String> = steps.iter().map(|s| format!(r#""{}""#, s)).collect();
        let options_json: Vec<String> = options.iter().map(|s| format!(r#""{}""#, s)).collect();
        let triggers_json: Vec<String> = triggers.iter().map(|s| format!(r#""{}""#, s)).collect();
        let json = format!(r#"{{
            "plugins": [{{
                "plugin_id": "test",
                "version": "1.0.0",
                "contributes": {{
                    "steps": [{}],
                    "agent_types": [],
                    "options": [{}],
                    "triggers": [{}],
                    "tools": [],
                    "when_conditions": []
                }}
            }}]
        }}"#,
            steps_json.join(","),
            options_json.join(","),
            triggers_json.join(","),
        );
        PluginRegistry::from_json(&json).unwrap()
    }

    #[test]
    fn tgap006_has_step_returns_true_for_contributed_step() {
        let reg = simple_registry(&["slackSend"], &[], &[]);
        assert!(reg.has_step("slackSend"));
        assert!(!reg.has_step("unknownStep"));
    }

    #[test]
    fn tgap006_has_option_returns_true_for_contributed_option() {
        let reg = simple_registry(&[], &["ansiColor"], &[]);
        assert!(reg.has_option("ansiColor"));
        assert!(!reg.has_option("unknownOption"));
    }

    #[test]
    fn tgap006_has_trigger_returns_true_for_contributed_trigger() {
        let reg = simple_registry(&[], &[], &["githubPush"]);
        assert!(reg.has_trigger("githubPush"));
        assert!(!reg.has_trigger("unknownTrigger"));
    }

    // ── TGAP-007: builtin registry coverage smoke test ────────────────────────

    /// Smoke test: all 18 expected plugin_ids are present in the builtin registry.
    #[test]
    fn tgap007_builtin_registry_covers_all_expected_contributions() {
        let registry = PluginRegistry::builtin();
        assert_eq!(registry.plugins.len(), 18, "expected 18 plugins, got: {}", registry.plugins.len());

        // Tools coverage
        for tool in &["nodejs", "node", "go", "golang", "gradle", "maven", "ant", "git", "jdk", "java"] {
            assert!(registry.has_tool(tool), "expected builtin to have tool '{}', but it doesn't", tool);
        }
        // Steps coverage
        for step in &["slackSend", "withSonarQubeEnv", "waitForQualityGate", "xunit", "cleanWs", "deleteDir"] {
            assert!(registry.has_step(step), "expected builtin to have step '{}', but it doesn't", step);
        }
        // Options coverage
        assert!(registry.has_option("timestamps"));
        assert!(registry.has_option("ansiColor"));
        // Triggers coverage
        assert!(registry.has_trigger("githubPush"));
        assert!(registry.has_trigger("gitlab"));
        assert!(registry.has_trigger("GenericTrigger"));
    }

    // ── TGAP-005: merge() duplicate plugin_id behavior ────────────────────────

    /// merge() does not deduplicate by plugin_id — it accumulates all entries.
    /// This test documents the known behavior: merging two registries with the
    /// same plugin_id results in duplicate entries in plugins Vec.
    #[test]
    fn merge_accumulates_duplicates_from_same_plugin_id() {
        let json_a = r#"{"plugins": [{"plugin_id": "myplugin", "version": "1.0.0", "contributes": {"steps": [], "agent_types": [], "options": [], "triggers": [], "tools": ["toolX"], "when_conditions": []}}]}"#;
        let json_b = r#"{"plugins": [{"plugin_id": "myplugin", "version": "1.0.0", "contributes": {"steps": [], "agent_types": [], "options": [], "triggers": [], "tools": ["toolX"], "when_conditions": []}}]}"#;
        let mut reg_a = PluginRegistry::from_json(json_a).unwrap();
        let reg_b = PluginRegistry::from_json(json_b).unwrap();
        reg_a.merge(reg_b);
        // Known behavior: merge() does not dedup — both entries remain
        assert_eq!(
            reg_a.plugins.len(), 2,
            "merge() accumulates duplicates: expected 2 plugin entries after merging same plugin_id twice"
        );
        // all_tools() therefore returns duplicates too
        let tools = reg_a.all_tools();
        assert_eq!(
            tools.len(), 2,
            "all_tools() returns duplicate entries when same tool contributed by two merged plugins"
        );
    }

    // ── TGAP-018: all_tools() duplicate behavior ──────────────────────────────

    /// all_tools() returns duplicate entries when the same tool name is contributed
    /// by two distinct plugins. This documents known behavior (no dedup).
    #[test]
    fn all_tools_returns_duplicate_entries_when_same_tool_in_two_plugins() {
        let json = r#"{
            "plugins": [
                {
                    "plugin_id": "plugin-alpha",
                    "version": "1.0.0",
                    "contributes": {
                        "steps": [], "agent_types": [], "options": [], "triggers": [],
                        "tools": ["sharedTool"],
                        "when_conditions": []
                    }
                },
                {
                    "plugin_id": "plugin-beta",
                    "version": "1.0.0",
                    "contributes": {
                        "steps": [], "agent_types": [], "options": [], "triggers": [],
                        "tools": ["sharedTool"],
                        "when_conditions": []
                    }
                }
            ]
        }"#;
        let registry = PluginRegistry::from_json(json).unwrap();
        let tools = registry.all_tools();
        // Known behavior: all_tools() returns one entry per plugin contribution,
        // so "sharedTool" appears twice when contributed by two separate plugins.
        assert_eq!(
            tools.len(), 2,
            "all_tools() should return duplicate entries for same tool in two plugins; got: {:?}", tools
        );
        assert!(tools.iter().all(|t| *t == "sharedTool"), "all entries should be 'sharedTool'");
        // has_tool() still works correctly despite duplicates
        assert!(registry.has_tool("sharedTool"));
    }

    // ── ARC-003: StepEntry tests ───────────────────────────────────────────────

    #[test]
    fn step_entry_has_step_by_name() {
        let registry = PluginRegistry {
            plugins: vec![PluginEntry {
                plugin_id: "myplugin".to_string(),
                version: "1.0.0".to_string(),
                min_version: None,
                max_version: None,
                contributes: PluginContributions {
                    steps: vec![StepEntry { name: "slackSend".to_string(), args: vec![] }],
                    agent_types: vec![],
                    options: vec![],
                    triggers: vec![],
                    tools: vec![],
                    when_conditions: vec![],
                },
            }],
        };
        assert!(registry.has_step("slackSend"));
        assert!(!registry.has_step("unknownStep"));
    }

    #[test]
    fn step_entry_args_accessible() {
        let registry = PluginRegistry {
            plugins: vec![PluginEntry {
                plugin_id: "myplugin".to_string(),
                version: "1.0.0".to_string(),
                min_version: None,
                max_version: None,
                contributes: PluginContributions {
                    steps: vec![StepEntry {
                        name: "sh".to_string(),
                        args: vec![StepArg {
                            name: "script".to_string(),
                            required: true,
                            arg_type: Some("string".to_string()),
                        }],
                    }],
                    agent_types: vec![],
                    options: vec![],
                    triggers: vec![],
                    tools: vec![],
                    when_conditions: vec![],
                },
            }],
        };
        let steps = registry.all_steps();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "sh");
        assert_eq!(steps[0].args.len(), 1);
        assert_eq!(steps[0].args[0].name, "script");
        assert!(steps[0].args[0].required);
    }

    #[test]
    fn from_json_accepts_string_steps_for_backwards_compat() {
        let json = r#"{
            "plugins": [{
                "plugin_id": "test",
                "version": "1.0.0",
                "contributes": {
                    "steps": ["sh", "echo"],
                    "agent_types": [],
                    "options": [],
                    "triggers": [],
                    "tools": [],
                    "when_conditions": []
                }
            }]
        }"#;
        let registry = PluginRegistry::from_json(json).unwrap();
        assert!(registry.has_step("sh"), "has_step('sh') should return true for old string format");
        assert!(registry.has_step("echo"), "has_step('echo') should return true for old string format");
        assert!(!registry.has_step("slackSend"));
    }
}
