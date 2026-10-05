// SPARQL output tests for §13.1–§13.11.
// All tests are #[ignore] until T4-G wires up `to_sparql`.

use dolfin_query::to_sparql;
use rowl::parser::parse_ontology;

fn parse_single_query(src: &str) -> rowl::ast::QueryDef {
    let result = parse_ontology(src);
    assert!(result.is_ok(), "parse error: {:?}", result.errors());
    result.ontology.unwrap().queries().into_iter().next().unwrap().clone()
}

#[test]
fn test_sparql_13_1_simple_typed_subject() {
    let q = parse_single_query(concat!(
        "query movies_released_year:\n",
        "  a ex:Movie\n",
        "    ex:title ?title\n",
        "    ex:year ?year\n",
        "    ex:rating ?rating\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("SELECT"));
    assert!(sparql.contains("ex:Movie"));
    assert!(sparql.contains("?title"));
}

#[test]
fn test_sparql_13_2_inline_filters() {
    let q = parse_single_query(concat!(
        "query good_movies_after_2010:\n",
        "  a ex:Movie\n",
        "    ex:title ?title\n",
        "    ex:year [?year > 2010]\n",
        "    ex:rating [?rating > 7.5]\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("FILTER"));
    assert!(sparql.contains("> 2010") || sparql.contains(">2010"));
}

#[test]
fn test_sparql_13_3_return_order() {
    let q = parse_single_query(concat!(
        "query sorted_movies:\n",
        "  a ex:Movie\n",
        "    ex:title ?title\n",
        "    ex:rating ?rating\n",
        "  return\n",
        "    title ?title\n",
        "    rating ?rating\n",
        "      order desc\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("ORDER BY DESC"));
}

#[test]
fn test_sparql_13_4_limit() {
    let q = parse_single_query(concat!(
        "query top_10_movies:\n",
        "  a ex:Movie\n",
        "    ex:title ?title\n",
        "    ex:rating ?rating\n",
        "  return\n",
        "    ?title\n",
        "    ?rating\n",
        "      order desc\n",
        "    limit 10\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("LIMIT 10"));
}

#[test]
fn test_sparql_13_6_distinct() {
    let q = parse_single_query(concat!(
        "query distinct_genres:\n",
        "  a ex:Movie\n",
        "    ex:genre ?genre\n",
        "  return\n",
        "    ?genre\n",
        "      distinct\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("SELECT DISTINCT"));
}

#[test]
fn test_sparql_13_7_existence() {
    let q = parse_single_query(concat!(
        "query directors_not_horror:\n",
        "  ?director a ex:Director\n",
        "    ex:name ?name\n",
        "  some:\n",
        "    is ex:director of ?_movie\n",
        "  none:\n",
        "    ?director is ex:director of [ex:genre \"Horror\"]\n",
        "  return\n",
        "    ?director\n",
        "    ?name\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("FILTER EXISTS"));
    assert!(sparql.contains("FILTER NOT EXISTS"));
}

#[test]
fn test_sparql_13_8_group_by() {
    let q = parse_single_query(concat!(
        "query movies_per_genre_count:\n",
        "  ?movie a ex:Movie\n",
        "    ex:genre ?genre\n",
        "  group by ?genre\n",
        "    count ?movie as ?movieCount\n",
        "  return\n",
        "    ?genre\n",
        "    ?movieCount\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("GROUP BY"));
    assert!(sparql.starts_with("SELECT ?genre (COUNT(?movie) AS ?movieCount)\n"), "{sparql}");
}

#[test]
fn test_sparql_13_9_having() {
    let q = parse_single_query(concat!(
        "query popular_genres:\n",
        "  ?movie a ex:Movie\n",
        "    ex:genre ?genre\n",
        "    ex:rating ?rating\n",
        "  group by ?genre\n",
        "    count ?movie as ?movieCount\n",
        "    average ?rating as ?avgRating\n",
        "    ?movieCount >= 10\n",
        "    ?avgRating > 7.0\n",
        "  return\n",
        "    ?genre\n",
        "    ?movieCount\n",
        "    ?avgRating\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("HAVING"));
    assert!(sparql.starts_with("SELECT ?genre (COUNT(?movie) AS ?movieCount) (AVG(?rating) AS ?avgRating)\n"), "{sparql}");
}

#[test]
fn test_sparql_primitive_type_is_datatype_filter() {
    let q = parse_single_query(concat!(
        "query labels:\n",
        "  a ex:Movie\n",
        "    ex:label [ a string ]\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(sparql.contains("FILTER (datatype(?_v1) = xsd:string)"), "{sparql}");
}

#[test]
fn test_sparql_mixed_union_is_one_filter() {
    let q = parse_single_query(concat!(
        "query labels:\n",
        "  a ex:Movie\n",
        "    ex:label [ a (ex:Person or string) ]\n",
    ));
    let sparql = to_sparql(&q, &[&q]).unwrap();
    assert!(
        sparql.contains("FILTER (EXISTS { ?_v1 a ex:Person . } || datatype(?_v1) = xsd:string)"),
        "{sparql}"
    );
    assert!(!sparql.contains("UNION"), "{sparql}");
}
