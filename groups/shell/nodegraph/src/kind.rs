//! Kind descriptions, evaluation contracts, and owned registration.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::value::{Value, ValueType};

/// One named input or output, in evaluation order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortDesc {
    pub name: String,
    pub value_type: ValueType,
}

/// One named parameter, in evaluation order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParamDesc {
    pub name: String,
    pub value_type: ValueType,
}

/// Errors shared by registration, lookup, and kind evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeError {
    DuplicateType(String),
    UnknownType(String),
    Evaluation(String),
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateType(name) => write!(f, "duplicate node type: {name}"),
            Self::UnknownType(name) => write!(f, "unknown node type: {name}"),
            Self::Evaluation(message) => write!(f, "node evaluation failed: {message}"),
        }
    }
}

impl Error for NodeError {}

/// A kind's ordered interface and its evaluation operation.
///
/// Implementations validate parameter and input slices before evaluating them.
pub trait NodeKind {
    fn type_name(&self) -> &str;
    fn inputs(&self) -> &[PortDesc];
    fn outputs(&self) -> &[PortDesc];
    fn params(&self) -> &[ParamDesc];
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError>;
}

/// Owns kinds under exact, case-sensitive snapshots of their type names.
#[derive(Default)]
pub struct KindRegistry {
    kinds: BTreeMap<String, Box<dyn NodeKind>>,
}

impl KindRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a kind without replacing an existing registration.
    pub fn add(&mut self, kind: Box<dyn NodeKind>) -> Result<(), NodeError> {
        let name = kind.type_name().to_owned();
        match self.kinds.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(kind);
                Ok(())
            }
            Entry::Occupied(entry) => Err(NodeError::DuplicateType(entry.key().clone())),
        }
    }

    /// Returns the registered implementation or a named lookup error.
    pub fn lookup(&self, type_name: &str) -> Result<&dyn NodeKind, NodeError> {
        match self.kinds.get(type_name) {
            Some(kind) => Ok(kind.as_ref()),
            None => Err(NodeError::UnknownType(type_name.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
    use crate::value::{Value, ValueType};

    struct Echo {
        ports: Vec<PortDesc>,
    }

    impl Echo {
        fn new(marker: i64) -> Self {
            Self {
                ports: vec![PortDesc {
                    name: format!("value{marker}"),
                    value_type: ValueType::Text,
                }],
            }
        }
    }

    impl NodeKind for Echo {
        fn type_name(&self) -> &str {
            "test.echo"
        }

        fn inputs(&self) -> &[PortDesc] {
            &self.ports
        }

        fn outputs(&self) -> &[PortDesc] {
            &self.ports
        }

        fn params(&self) -> &[ParamDesc] {
            &[]
        }

        fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
            if !params.is_empty() {
                return Err(NodeError::Evaluation("expected no parameters".into()));
            }
            match inputs {
                [Value::Text(text)] => Ok(vec![Value::Text(text.clone())]),
                _ => Err(NodeError::Evaluation("expected one text input".into())),
            }
        }
    }

    #[test]
    fn registry_rejects_duplicates_and_preserves_original() {
        let mut registry = KindRegistry::new();
        assert_eq!(registry.add(Box::new(Echo::new(7))), Ok(()));
        assert_eq!(
            registry.add(Box::new(Echo::new(9))),
            Err(NodeError::DuplicateType("test.echo".into()))
        );
        let kind = registry.lookup("test.echo").expect("registered kind");
        assert_eq!(kind.inputs()[0].name, "value7");
        assert_eq!(kind.inputs()[0].value_type, ValueType::Text);
        assert_eq!(kind.inputs(), kind.outputs());
        assert!(kind.params().is_empty());
        assert_eq!(
            kind.eval(&[], &[Value::Text("payload".into())]),
            Ok(vec![Value::Text("payload".into())])
        );
        assert_eq!(
            kind.eval(&[], &[Value::Bool(true)]),
            Err(NodeError::Evaluation("expected one text input".into()))
        );
        assert_eq!(
            kind.eval(&[], &[]),
            Err(NodeError::Evaluation("expected one text input".into()))
        );
        assert_eq!(
            kind.eval(&[Value::Int(1)], &[]),
            Err(NodeError::Evaluation("expected no parameters".into()))
        );
    }

    #[test]
    fn registry_reports_unknown_names_without_normalizing() {
        let mut registry = KindRegistry::default();
        registry.add(Box::new(Echo::new(7))).expect("first add");
        for name in ["", "missing", "TEST.ECHO", "test.echo\r\n"] {
            match registry.lookup(name) {
                Err(error) => assert_eq!(error, NodeError::UnknownType(name.into())),
                Ok(_) => panic!("unexpected kind for {name:?}"),
            }
        }
        assert_eq!(
            NodeError::DuplicateType("test.echo".into()).to_string(),
            "duplicate node type: test.echo"
        );
        assert_eq!(
            NodeError::UnknownType("missing".into()).to_string(),
            "unknown node type: missing"
        );
        assert_eq!(
            NodeError::Evaluation("bad input".into()).to_string(),
            "node evaluation failed: bad input"
        );
    }
}
