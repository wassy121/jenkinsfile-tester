pub mod walk;

use serde::{Deserialize, Serialize, Serializer, Deserializer};
use serde::de::{self, Visitor};

/// Source location (1-based line and column numbers) attached to key AST nodes.
///
/// Only present when the node was produced by the parser with span tracking enabled.
/// Absent when AST nodes are constructed directly (e.g. in unit tests).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub line: u32,
    pub col: u32,
}

// ── SharedLibrary ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedLibrary {
    pub name: String,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
}

// ── Binding enum for withCredentials ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Binding {
    UsernamePassword {
        credentials_id: String,
        username_variable: String,
        password_variable: String,
    },
    StringBinding {
        credentials_id: String,
        variable: String,
    },
    FileBinding {
        credentials_id: String,
        variable: String,
    },
    SshUserPrivateKey {
        credentials_id: String,
        key_file_variable: String,
        passphrase_variable: Option<String>,
    },
    Certificate {
        credentials_id: String,
        keystore_variable: String,
        password_variable: Option<String>,
    },
    Raw(String),
}

// ── Matrix types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixAxis {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixExclude {
    pub axes: Vec<MatrixAxis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matrix {
    pub axes: Vec<MatrixAxis>,
    pub excludes: Vec<MatrixExclude>,
    pub stages: Vec<Stage>,
}

// ── StageBody enum ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StageBody {
    Steps(Steps),
    Parallel { stages: Vec<Stage> },
    Sequential { stages: Vec<Stage> },
    Matrix(Matrix),
}

// ── PipelineOption ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineOption {
    Timeout { time: u64, unit: String },
    #[serde(rename = "buildDiscarder")]
    BuildDiscarder {
        num_to_keep: Option<String>,
        days_to_keep: Option<String>,
        artifact_num_to_keep: Option<String>,
        artifact_days_to_keep: Option<String>,
        raw: Option<String>,
    },
    Retry { count: u32 },
    #[serde(rename = "disableConcurrentBuilds")]
    DisableConcurrentBuilds { abort_previous: bool },
    #[serde(rename = "skipDefaultCheckout")]
    SkipDefaultCheckout,
    #[serde(rename = "skipStagesAfterUnstable")]
    SkipStagesAfterUnstable,
    #[serde(rename = "preserveStashes")]
    PreserveStashes { build_count: Option<u32> },
    Timestamps,
    #[serde(rename = "parallelsAlwaysFailFast")]
    ParallelsAlwaysFailFast,
    #[serde(rename = "newContainerPerStage")]
    NewContainerPerStage,
    #[serde(rename = "quietPeriod")]
    QuietPeriod { seconds: u32 },
    #[serde(rename = "checkoutToSubdirectory")]
    CheckoutToSubdirectory { path: String },
    #[serde(rename = "disableResume")]
    DisableResume,
    #[serde(rename = "ansiColor")]
    AnsiColor { colormap: String },
    Raw { name: String, text: String },
}

// ── Trigger enum ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Cron { spec: String },
    PollScm { spec: String },
    Upstream { projects: String, threshold: Option<String> },
    GithubPush,
    GitlabPush { trigger_on_push: Option<bool>, trigger_on_merge_request: Option<bool> },
    GenericTrigger { token: Option<String>, cause: Option<String> },
    Raw { name: String, text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub agent: Option<Agent>,
    pub environment: Vec<EnvVar>,
    pub options: Vec<PipelineOption>,
    pub parameters: Vec<Parameter>,
    pub triggers: Vec<Trigger>,
    pub tools: Vec<Tool>,
    pub stages: Vec<Stage>,
    pub post: Option<Post>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<SharedLibrary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Agent {
    Any,
    None,
    Label(String),
    Docker {
        image: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        custom_workspace: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reuse_node: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        registry_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        registry_credentials_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        always_pull: Option<bool>,
    },
    Dockerfile {
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        additional_build_args: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// node { label '…'; customWorkspace '…' } agent form.
    Node {
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        custom_workspace: Option<String>,
    },
    /// Typed kubernetes agent block with named sub-fields.
    #[serde(rename = "kubernetes")]
    Kubernetes {
        #[serde(skip_serializing_if = "Option::is_none")]
        yaml: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        yaml_file: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cloud: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default_container: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        inherit_from: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        retries: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// Catch-all for plugin-registered agent types (e.g. kubernetes).
    Generic { agent_type: String },
}

/// The value of an environment variable entry.
///
/// `Literal` serialises as a plain string (backwards-compatible).
/// `Credentials` serialises as `{ "type": "credentials", "id": "..." }`.
#[derive(Debug, Clone)]
pub enum EnvValue {
    Literal(String),
    Credentials { id: String },
}

impl Serialize for EnvValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            EnvValue::Literal(v) => s.serialize_str(v),
            EnvValue::Credentials { id } => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(Some(2))?;
                map.serialize_entry("type", "credentials")?;
                map.serialize_entry("id", id)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for EnvValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct EnvValueVisitor;

        impl<'de> Visitor<'de> for EnvValueVisitor {
            type Value = EnvValue;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a string or a credentials object")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<EnvValue, E> {
                Ok(EnvValue::Literal(v.to_string()))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<EnvValue, E> {
                Ok(EnvValue::Literal(v))
            }

            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<EnvValue, A::Error> {
                let mut type_val: Option<String> = None;
                let mut id_val: Option<String> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => { type_val = Some(map.next_value()?); }
                        "id" => { id_val = Some(map.next_value()?); }
                        _ => { let _: de::IgnoredAny = map.next_value()?; }
                    }
                }
                if type_val.as_deref() == Some("credentials") {
                    let id = id_val.ok_or_else(|| de::Error::missing_field("id"))?;
                    Ok(EnvValue::Credentials { id })
                } else {
                    Err(de::Error::custom("unknown EnvValue type"))
                }
            }
        }

        d.deserialize_any(EnvValueVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: EnvValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Parameter {
    String { name: String, default_value: Option<String>, description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
    Boolean { name: String, default_value: Option<bool>, description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
    Choice { name: String, choices: Vec<String>, description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
    Text { name: String, default_value: Option<String>, description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
    Password { name: String, default_value: Option<String>, description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
    File { name: String, description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
    Run { name: String, description: Option<String>, filter: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] location: Option<Location> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub tool_type: String,
    pub name: String,
}

// ── StageInput struct (stage-level input directive) ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageInput {
    pub message: String,
    pub ok: Option<String>,
    pub submitter: Option<String>,
    pub submitter_parameter: Option<String>,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    pub agent: Option<Agent>,
    pub environment: Vec<EnvVar>,
    pub when: Option<When>,
    #[serde(default)]
    pub options: Vec<PipelineOption>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<StageInput>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fail_fast: bool,
    pub body: StageBody,
    pub post: Option<Post>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Steps {
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Step {
    Sh {
        script: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_double_quoted: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<Location>,
    },
    Echo {
        message: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_double_quoted: bool,
    },
    Script { body: String },
    Checkout { scm: String },
    WithCredentials { bindings: Vec<Binding>, steps: Vec<Step> },
    Retry { count: u32, steps: Vec<Step> },
    Timeout { time: u32, unit: String, steps: Vec<Step> },
    Command { name: String, args: Vec<Arg> },
    Generic { name: String, args: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arg {
    pub key: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct When {
    pub conditions: Vec<WhenCondition>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub before_agent: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub before_input: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub before_options: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WhenCondition {
    Branch { pattern: String },
    Environment { name: String, value: String },
    Expression { expr: String },
    Not { condition: Box<WhenCondition> },
    AllOf { conditions: Vec<WhenCondition> },
    AnyOf { conditions: Vec<WhenCondition> },
    Tag { pattern: String },
    ChangeRequest,
    BuildingTag,
    Changelog { pattern: String },
    Changeset { pattern: String },
    TriggeredBy { cause: String },
    Equals { actual: String, expected: String },
    Generic { name: String, args: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub always: Option<Steps>,
    pub success: Option<Steps>,
    pub failure: Option<Steps>,
    pub unstable: Option<Steps>,
    pub aborted: Option<Steps>,
    pub changed: Option<Steps>,
    pub cleanup: Option<Steps>,
    pub regression: Option<Steps>,
    pub fixed: Option<Steps>,
    pub unsuccessful: Option<Steps>,
}

impl Post {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.always.is_none()
            && self.success.is_none()
            && self.failure.is_none()
            && self.unstable.is_none()
            && self.aborted.is_none()
            && self.changed.is_none()
            && self.cleanup.is_none()
            && self.regression.is_none()
            && self.fixed.is_none()
            && self.unsuccessful.is_none()
    }
}
