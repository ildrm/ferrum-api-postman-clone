//! Deterministic scope precedence and local dynamic-variable interpolation.

use std::collections::HashMap;

use chrono::Utc;
use ferrum_domain::Variable;
use thiserror::Error;
use uuid::Uuid;

/// Variable scopes ordered from lowest to highest precedence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    /// Application global.
    Global,
    /// Workspace.
    Workspace,
    /// Collection.
    Collection,
    /// Selected environment.
    Environment,
    /// Local-only.
    Local,
    /// Request.
    Request,
    /// One execution.
    Runtime,
}

/// A scope and its variables.
#[derive(Clone, Debug)]
pub struct VariableScope<'a> {
    /// Scope category.
    pub scope: Scope,
    /// Variables in this scope.
    pub variables: &'a [Variable],
}

/// Interpolation output and unresolved diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Interpolation {
    /// Resolved text.
    pub value: String,
    /// Unique unresolved variable names in first-seen order.
    pub unresolved: Vec<String>,
}

/// Errors raised while obtaining a sensitive variable.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// Secret backend could not supply a value.
    #[error("could not resolve sensitive variable '{name}': {message}")]
    Secret {
        /// Variable name.
        name: String,
        /// Redacted backend error.
        message: String,
    },
}

/// Resolves a sensitive value without exposing secret-store details to the parser.
pub trait SensitiveValueProvider {
    /// Reads one sensitive variable value.
    fn get(&self, name: &str) -> Result<Option<String>, ResolveError>;
}

/// Scope-aware variable resolver.
pub struct VariableResolver<'a, P> {
    values: HashMap<&'a str, &'a Variable>,
    provider: &'a P,
}

impl<'a, P: SensitiveValueProvider> VariableResolver<'a, P> {
    /// Builds a resolver. Later scope entries override earlier entries.
    pub fn new(scopes: impl IntoIterator<Item = VariableScope<'a>>, provider: &'a P) -> Self {
        let mut values = HashMap::new();
        for scoped in scopes {
            for variable in scoped.variables.iter().filter(|item| item.enabled) {
                values.insert(variable.name.as_str(), variable);
            }
        }
        Self { values, provider }
    }

    /// Interpolates all `{{name}}` occurrences and preserves unresolved tokens.
    pub fn interpolate(&self, input: &str) -> Result<Interpolation, ResolveError> {
        let mut value = String::with_capacity(input.len());
        let mut unresolved = Vec::new();
        let mut cursor = 0;

        while let Some(relative_start) = input[cursor..].find("{{") {
            let start = cursor + relative_start;
            value.push_str(&input[cursor..start]);
            let token_start = start + 2;
            let Some(relative_end) = input[token_start..].find("}}") else {
                value.push_str(&input[start..]);
                cursor = input.len();
                break;
            };
            let end = token_start + relative_end;
            let name = input[token_start..end].trim();
            if let Some(resolved) = self.resolve_one(name)? {
                value.push_str(&resolved);
            } else {
                value.push_str(&input[start..end + 2]);
                if !unresolved.iter().any(|item| item == name) {
                    unresolved.push(name.to_owned());
                }
            }
            cursor = end + 2;
        }
        value.push_str(&input[cursor..]);
        Ok(Interpolation { value, unresolved })
    }

    fn resolve_one(&self, name: &str) -> Result<Option<String>, ResolveError> {
        if let Some(dynamic) = dynamic_value(name) {
            return Ok(Some(dynamic));
        }
        let Some(variable) = self.values.get(name) else {
            return Ok(None);
        };
        if variable.sensitive {
            self.provider.get(name)
        } else {
            Ok(Some(variable.current_value.clone()))
        }
    }
}

fn dynamic_value(name: &str) -> Option<String> {
    match name {
        "$uuid" => Some(Uuid::new_v4().to_string()),
        "$timestamp" => Some(Utc::now().timestamp().to_string()),
        "$randomInt" => {
            let bytes = *Uuid::new_v4().as_bytes();
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000;
            Some(value.to_string())
        }
        "$randomEmail" => Some(format!("user-{}@example.test", Uuid::new_v4().simple())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSecrets;

    impl SensitiveValueProvider for NoSecrets {
        fn get(&self, _name: &str) -> Result<Option<String>, ResolveError> {
            Ok(None)
        }
    }

    fn variable(name: &str, value: &str) -> Variable {
        Variable {
            name: name.into(),
            current_value: value.into(),
            initial_value: None,
            sensitive: false,
            enabled: true,
        }
    }

    #[test]
    fn later_scopes_override_earlier_scopes() {
        let globals = [variable("host", "global.test")];
        let environment = [variable("host", "environment.test")];
        let resolver = VariableResolver::new(
            [
                VariableScope {
                    scope: Scope::Global,
                    variables: &globals,
                },
                VariableScope {
                    scope: Scope::Environment,
                    variables: &environment,
                },
            ],
            &NoSecrets,
        );
        let output = resolver.interpolate("https://{{host}}/v1").unwrap();
        assert_eq!(output.value, "https://environment.test/v1");
        assert!(output.unresolved.is_empty());
    }

    #[test]
    fn preserves_and_reports_unresolved_variables() {
        let resolver = VariableResolver::new([], &NoSecrets);
        let output = resolver.interpolate("{{missing}}/{{missing}}").unwrap();
        assert_eq!(output.value, "{{missing}}/{{missing}}");
        assert_eq!(output.unresolved, ["missing"]);
    }

    #[test]
    fn produces_dynamic_values() {
        let resolver = VariableResolver::new([], &NoSecrets);
        let output = resolver.interpolate("{{$uuid}}/{{$randomInt}}").unwrap();
        assert!(!output.value.contains("{{"));
    }
}
