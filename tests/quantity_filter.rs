//! A query comparison against a `quantity(...)` literal is compiled to a
//! unit-aware SPARQL unfold (dimension guard + SI-magnitude comparison), not a
//! bare scalar FILTER.

use dolfin_query::to_sparql;
use rowl::parser::parse_ontology;

fn sparql_for(src: &str) -> String {
    let result = parse_ontology(src);
    let onto = result.ontology.expect("parse ok");
    let q = onto.queries().into_iter().next().expect("a query").clone();
    to_sparql(&q, &[]).expect("to_sparql")
}

#[test]
fn quantity_comparison_unfolds_to_unit_aware_filter() {
    let src = "\
query fast_cars:
  a ex:Vehicle
    ex:topSpeed [ >= quantity(60 km/h) ]
";
    let sparql = sparql_for(src);

    // Resolves the stored value's unit via its datatype + the dq: definition block.
    assert!(sparql.contains("BIND(datatype("), "no datatype BIND:\n{sparql}");
    assert!(
        sparql.contains("https://dolfin.dev/quantity#coefficient"),
        "no coefficient join:\n{sparql}"
    );
    assert!(
        sparql.contains("https://dolfin.dev/quantity#dimension"),
        "no dimension join:\n{sparql}"
    );
    // Dimension guard for a velocity.
    assert!(sparql.contains("\"L1.T-1\""), "no dimension guard:\n{sparql}");
    // Compares SI magnitudes: 60 km/h = 16.666… m/s.
    assert!(sparql.contains("16.6666"), "no SI target:\n{sparql}");
    assert!(sparql.contains("xsd:double(str("), "no lexical cast:\n{sparql}");
}

#[test]
fn plain_numeric_comparison_stays_a_scalar_filter() {
    // A non-quantity comparison must not trigger the unfold.
    let src = "\
query recent:
  a ex:Movie
    ex:year [ >= 2000 ]
";
    let sparql = sparql_for(src);
    assert!(sparql.contains("FILTER"), "expected a FILTER:\n{sparql}");
    assert!(
        !sparql.contains("quantity#coefficient"),
        "plain comparison must not unfold:\n{sparql}"
    );
}
