use std::sync::Arc;
use crate::ast::Pipeline;
use crate::plugins::PluginRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    /// Unknown steps emit a Warning (default behaviour).
    Permissive,
    /// Unknown steps emit an Error.
    Strict,
    /// Unknown steps are silent; used internally by `get_unknown_keywords`.
    Discovery,
}

impl Default for ValidationMode {
    fn default() -> Self {
        ValidationMode::Permissive
    }
}

pub struct ValidationContext<'a> {
    pub pipeline: &'a Pipeline,
    pub registry: Arc<PluginRegistry>,
    pub mode: ValidationMode,
}

impl<'a> ValidationContext<'a> {
    #[allow(dead_code)]
    pub fn new(pipeline: &'a Pipeline, _source: &'a str) -> Self {
        Self {
            pipeline,
            registry: PluginRegistry::builtin_arc(),
            mode: ValidationMode::Permissive,
        }
    }

    pub fn with_registry(pipeline: &'a Pipeline, _source: &'a str, registry: Arc<PluginRegistry>) -> Self {
        Self { pipeline, registry, mode: ValidationMode::Permissive }
    }

    pub fn with_mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }
}
