use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum QueryError {
    /// Referenced query name is not defined
    UnknownQuery(String),
    /// Queries form a cycle
    CircularDependency(Vec<String>),
    /// Internal error (should not occur in well-typed AST)
    Internal(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::UnknownQuery(name) => write!(f, "unknown query '{}'", name),
            QueryError::CircularDependency(cycle) => {
                write!(f, "circular query dependency: {}", cycle.join(" → "))
            }
            QueryError::Internal(msg) => write!(f, "internal error: {}", msg),
        }
    }
}

impl std::error::Error for QueryError {}
