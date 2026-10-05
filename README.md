# dolfin-query

Translate Dolfin query definitions to SPARQL 1.1 `SELECT`.

Queries are parsed by [`rowl`](https://crates.io/crates/rowl); this crate turns
the resulting `QueryDef` AST into a SPARQL string.

## Install

```toml
[dependencies]
dolfin-query = "0.1.10"
rowl = { version = "0.1.10", default-features = false }
```

## Usage

```rust
use rowl::parser::parse_ontology;

let src = "\
query popular_genres:
  ?movie a ex:Movie
    ex:genre ?genre
    ex:rating [?rating > 5]
  group by ?genre
    count ?movie as ?movieCount
    ?movieCount >= 10
  return
    ?genre
    ?movieCount
      order desc
    limit 5
";

let onto = parse_ontology(src).ontology.unwrap();
for (name, sparql) in dolfin_query::file_to_sparql(&onto) {
    println!("{name}:\n{}", sparql.unwrap());
}
```

Output:

```sparql
SELECT ?genre (COUNT(?movie) AS ?movieCount)
WHERE {
  ?movie a ex:Movie .
  ?movie ex:genre ?genre .
  ?movie ex:rating ?rating .
  FILTER (?rating > 5)
}
GROUP BY ?genre
HAVING ((?movieCount >= 10))
ORDER BY DESC(?movieCount)
LIMIT 5
```

## API

- `file_to_sparql(&OntologyFile)` — translate every query in a file.
- `to_sparql(&QueryDef, all_queries)` — translate one query; `all_queries`
  holds the named queries available for composition.
- `to_sparql_with(.., &dyn PropNamer)` — same, with a custom `PropNamer` to
  control how property and class names are rendered (e.g. resolve bare names
  to IRIs per subject type).

Errors (`QueryError`): `UnknownQuery`, `CircularDependency`, `Internal`.

## Supported constructs

- Typed subjects (`a ex:Movie`), property patterns, `is … of` inverse patterns
- Inline filters (`[?year > 2010]`, `[>= 2000]`)
- Datatype and union type checks (`[a string]`, `[a (ex:Person or string)]`)
- Unit-aware quantity comparisons (`[>= quantity(60 km/h)]`), compiled to a
  dimension guard plus SI-magnitude comparison
- Existence blocks: `some:` → `FILTER EXISTS`, `none:` → `FILTER NOT EXISTS`
- `group by` with aggregates (`count`, `average`, …) and `HAVING` conditions
- `return` blocks: projection, `distinct`, `order asc|desc`, `limit`
- Query composition (inlining other named queries), with cycle detection
- Silent variables: `?_name` is never projected

## License

MIT
