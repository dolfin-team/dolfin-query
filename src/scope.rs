use std::collections::{HashMap, HashSet};
use rowl::ast::{QualifiedName, TypeRef, Literal, Expr};

/// A query node, as [`PropNamer`] sees it: a variable, or an anonymous node
/// (`[ … ]` block, subject-less pattern) by the start offset of its source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeKey {
    Var(String),
    At(usize),
}

impl NodeKey {
    /// The key of the anonymous node introduced at `span`.
    pub fn at(span: Option<&rowl::error::Span>) -> Option<Self> {
        span.map(|s| NodeKey::At(s.start.offset))
    }
}

/// Renders a property name as a SPARQL term, knowing the node it is used on
/// (the triple's subject), so a bare name can be resolved per type.
pub trait PropNamer {
    fn property(&self, property: &QualifiedName, subject: Option<&NodeKey>) -> String;

    /// Renders a class or individual name as a SPARQL term.
    fn name(&self, name: &QualifiedName) -> String {
        qn_to_sparql(name)
    }
}

/// Writes names as written ([`qn_to_sparql`]).
pub struct AsWritten;

impl PropNamer for AsWritten {
    fn property(&self, property: &QualifiedName, _: Option<&NodeKey>) -> String {
        qn_to_sparql(property)
    }
}

/// Forwards to another namer without the subject: for a composed query,
/// whose variables are not the ones the namer knows.
pub struct WithoutSubject<'a>(pub &'a dyn PropNamer);

impl PropNamer for WithoutSubject<'_> {
    fn property(&self, property: &QualifiedName, _: Option<&NodeKey>) -> String {
        self.0.property(property, None)
    }

    fn name(&self, name: &QualifiedName) -> String {
        self.0.name(name)
    }
}

/// Translation context: tracks variables, generates fresh names, collects projections.
pub struct ScopeCtx<'a> {
    counter: u32,
    /// Variables declared (seen) in the current query scope
    pub seen_vars: HashSet<String>,
    /// Variables that should appear in the SELECT projection (non-silent)
    pub projected: Vec<String>,
    /// The first (or primary) subject variable in scope, used by existence blocks
    pub primary_subject: Option<String>,
    pub namer: &'a dyn PropNamer,
    /// Generated variable → the anonymous node it stands for.
    keys: HashMap<String, NodeKey>,
}

impl ScopeCtx<'static> {
    pub fn new() -> Self {
        Self::with_namer(&AsWritten)
    }
}

impl<'a> ScopeCtx<'a> {
    pub fn with_namer(namer: &'a dyn PropNamer) -> Self {
        Self {
            counter: 0,
            seen_vars: HashSet::new(),
            projected: Vec::new(),
            primary_subject: None,
            namer,
            keys: HashMap::new(),
        }
    }

    /// A fresh variable standing for the anonymous node `key`.
    pub fn gensym_for(&mut self, prefix: &str, key: Option<NodeKey>) -> String {
        let v = self.gensym(prefix);
        if let Some(k) = key {
            self.keys.insert(v.clone(), k);
        }
        v
    }

    /// `property` as a SPARQL term, used with the term `subject` as subject.
    pub fn prop(&self, property: &QualifiedName, subject: &str) -> String {
        let key = match self.keys.get(subject) {
            Some(k) => Some(k.clone()),
            None if subject.starts_with('?') => Some(NodeKey::Var(subject.to_owned())),
            None => None,
        };
        self.namer.property(property, key.as_ref())
    }

    /// Generate a fresh internal variable name like `?_s0`.
    pub fn gensym(&mut self, prefix: &str) -> String {
        let n = self.counter;
        self.counter += 1;
        format!("?_{}{}", prefix, n)
    }

    /// Record a variable as seen; returns whether it is silent (starts with `?_`).
    pub fn see_var(&mut self, var: &str) -> bool {
        self.seen_vars.insert(var.to_string());
        is_silent(var)
    }

    /// Add a variable to the projection list if not already present and not silent.
    pub fn project(&mut self, var: &str) {
        if !is_silent(var) && !self.projected.contains(&var.to_string()) {
            self.projected.push(var.to_string());
        }
    }
}

impl Default for ScopeCtx<'static> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn is_silent(var: &str) -> bool {
    var.starts_with("?_")
}

/// Format a QualifiedName for SPARQL output.
/// For prefixed names (is_prefixed=true, e.g. ex:Movie) → "ex:Movie"
/// For dot-separated names → use full() which joins with "."
pub fn qn_to_sparql(qn: &QualifiedName) -> String {
    if qn.is_prefixed && qn.parts.len() >= 2 {
        format!("{}:{}", qn.parts[0], qn.parts[1..].join("."))
    } else {
        qn.full()
    }
}

/// Format a TypeRef for SPARQL output. `Union` has no single-term SPARQL
/// object form — `?s a (A or B)` compiles to a `UNION` block of triples
/// instead, via [`type_assertion_lines`]; callers must branch on `Union`
/// before reaching here.
pub fn typeref_to_sparql(tr: &TypeRef, namer: &dyn PropNamer) -> String {
    match tr {
        TypeRef::Named { name, .. } => namer.name(name),
        TypeRef::Primitive { kind, .. } => kind.xsd().to_string(),
        TypeRef::Union { .. } => {
            unreachable!("Union type refs are expanded by type_assertion_lines, not formatted inline")
        }
    }
}

/// `<subject> a <type_ref> .`, expanded to a `{ ... } UNION { ... }` block
/// when `type_ref` is `(A or B)` — SPARQL has no inline union-class object.
/// A primitive is a literal's datatype, not an `rdf:type`, so it becomes a
/// `datatype()` FILTER. A union with a primitive member is one `||` FILTER
/// (`EXISTS { ... }` for its classes): a FILTER alone in a `UNION` branch
/// sees `subject` unbound and never matches.
pub fn type_assertion_lines(subject: &str, tr: &TypeRef, namer: &dyn PropNamer) -> String {
    match tr {
        TypeRef::Union { members, .. } if has_primitive(tr) => {
            format!("FILTER ({})", members.iter().map(|m| type_test(subject, m, namer)).collect::<Vec<_>>().join(" || "))
        }
        TypeRef::Union { members, .. } => members
            .iter()
            .map(|m| format!("{{ {} }}", type_assertion_lines(subject, m, namer)))
            .collect::<Vec<_>>()
            .join(" UNION "),
        TypeRef::Primitive { .. } => format!("FILTER ({})", type_test(subject, tr, namer)),
        _ => format!("{} a {} .", subject, typeref_to_sparql(tr, namer)),
    }
}

fn has_primitive(tr: &TypeRef) -> bool {
    match tr {
        TypeRef::Primitive { .. } => true,
        TypeRef::Union { members, .. } => members.iter().any(has_primitive),
        TypeRef::Named { .. } => false,
    }
}

/// `type_ref` as a FILTER expression on `subject`. `datatype()` of an IRI is
/// an error, which `||` absorbs when another member holds.
fn type_test(subject: &str, tr: &TypeRef, namer: &dyn PropNamer) -> String {
    match tr {
        TypeRef::Primitive { kind, .. } => format!("datatype({}) = {}", subject, kind.xsd()),
        TypeRef::Union { members, .. } => members.iter().map(|m| type_test(subject, m, namer)).collect::<Vec<_>>().join(" || "),
        TypeRef::Named { .. } => format!("EXISTS {{ {} a {} . }}", subject, typeref_to_sparql(tr, namer)),
    }
}

/// Format a Literal for SPARQL output.
pub fn literal_to_sparql(lit: &Literal) -> String {
    match lit {
        Literal::Int { value, .. } => format!("{}", value),
        Literal::Float { value, .. } => format!("{}", value),
        Literal::String { value, .. } => format!("\"{}\"", value),
        Literal::Boolean { value, .. } => if *value { "true".to_string() } else { "false".to_string() },
        Literal::Iri { value, .. } => format!("<{}>", value),
        Literal::Temporal { content, .. } => {
            // SPARQL query context has no access to the file's @locale/@timezone
            // here, so resolution is strict: numeric dates need an inline `as`
            // mask, times need an inline offset. Unresolvable temporals fall
            // back to a plain string; the dolfin-analysis pass reports the error.
            let ctx = dolfin_datetime::TemporalContext::strict();
            match lit.resolve_temporal(&ctx) {
                Some(Ok((value, xsd_type))) => format!("\"{}\"^^{}", value, xsd_type),
                _ => format!("\"{}\"", content.replace('"', "\\\"")),
            }
        }
        Literal::Quantity { content, .. } => {
            // INTERIM: SPARQL sees only the canonical SI-base magnitude as a
            // bare xsd:double (lossy — no unit). This keeps numeric FILTERs
            // working; display-unpacking of the original unit is a follow-up
            // once the quantity→RDF representation lands. See
            // dolfin-units-turtle-plan.md.
            match lit.resolve_quantity() {
                Some(Ok(q)) => format!("\"{}\"^^xsd:double", q.si_value()),
                _ => format!("\"{}\"", content.replace('"', "\\\"")),
            }
        }
    }
}

/// Format an Expr for SPARQL output (simple literals only for now).
pub fn expr_to_sparql(expr: &Expr) -> String {
    match expr {
        Expr::Literal { value, .. } => literal_to_sparql(value),
        Expr::Variable { name, .. } => name.clone(),
        Expr::BinaryOp { op, left, right, .. } => {
            let sym = match op {
                rowl::ast::BinaryOp::Add => "+",
                rowl::ast::BinaryOp::Sub => "-",
                rowl::ast::BinaryOp::Mul => "*",
                rowl::ast::BinaryOp::Div => "/",
            };
            format!("({} {} {})", expr_to_sparql(left), sym, expr_to_sparql(right))
        }
        Expr::UnaryOp { op: _, operand, .. } => {
            format!("(-{})", expr_to_sparql(operand))
        }
    }
}

/// Format a comparison operator for SPARQL.
pub fn op_to_sparql(op: &rowl::ast::ComparisonOp) -> &'static str {
    match op {
        rowl::ast::ComparisonOp::Equal => "=",
        rowl::ast::ComparisonOp::NotEqual => "!=",
        rowl::ast::ComparisonOp::LessThan => "<",
        rowl::ast::ComparisonOp::LessEqual => "<=",
        rowl::ast::ComparisonOp::GreaterThan => ">",
        rowl::ast::ComparisonOp::GreaterEqual => ">=",
    }
}

/// Format an aggregation function name for SPARQL.
pub fn agg_to_sparql(kind: &rowl::ast::AggKind) -> &'static str {
    match kind {
        rowl::ast::AggKind::Count => "COUNT",
        rowl::ast::AggKind::Average => "AVG",
        rowl::ast::AggKind::Sum => "SUM",
        rowl::ast::AggKind::Min => "MIN",
        rowl::ast::AggKind::Max => "MAX",
    }
}
