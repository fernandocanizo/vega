//! Turns a query pasted from outside into a design: the operation, the selected fields
//! and the variable values, keeping only what the schema knows. The query itself is
//! regenerated from that selection, so it reads like one built by hand.

use std::collections::HashMap;

use graphql_parser::query as gq;
use serde_json::{Map, Number, Value};

use crate::querygen::is_composite;
use crate::schema::{Field, FullType, Root, Schema};
use crate::selection::{OperationRef, Path, Seg, Selection};
use crate::variables::{self, Values};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub operation: OperationRef,
    pub selection: Selection,
    /// What was typed for each argument, keyed like the values of the Variables pane.
    pub values: Values,
    /// Fields, arguments, branches and fragments the schema does not know.
    pub discarded: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImportError {
    #[error("not a valid GraphQL query: {0}")]
    Parse(String),
    #[error("the pasted text has no operation")]
    NoOperation,
    #[error("no operation named `{0}` in the pasted query")]
    UnknownOperation(String),
    #[error("none of the pasted root fields exist in the schema")]
    NoRootField,
    #[error("none of the pasted fields exist in the schema")]
    EmptySelection,
}

/// A request body as copied from a client: `{"query": ..., "variables": {...}}`.
struct Body {
    query: String,
    variables: Map<String, Value>,
    operation_name: Option<String>,
}

fn json_body(text: &str) -> Option<Body> {
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    let object = value.as_object()?;
    Some(Body {
        query: object.get("query")?.as_str()?.to_string(),
        variables: object
            .get("variables")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        operation_name: object
            .get("operationName")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// True when pasted text is meant as a query rather than as search text: a JSON request
/// body, or text that starts like an operation and has a selection set.
pub fn looks_like_query(text: &str) -> bool {
    if json_body(text).is_some() {
        return true;
    }
    let text = text.trim_start();
    let starts_like_one = text.starts_with('{')
        || ["query", "mutation", "subscription"].iter().any(|keyword| {
            text.strip_prefix(keyword).is_some_and(|rest| {
                rest.starts_with(|c: char| c.is_whitespace() || "{($@".contains(c))
            })
        });
    starts_like_one && text.contains('{')
}

/// The operation, selection and values of pasted `text`: a JSON request body or a bare
/// GraphQL document. Variables are matched to arguments by where they are used.
pub fn import(schema: &Schema, text: &str) -> Result<Imported, ImportError> {
    let body = json_body(text);
    let (document_text, provided, wanted) = match &body {
        Some(body) => (
            body.query.as_str(),
            body.variables.clone(),
            body.operation_name.as_deref(),
        ),
        None => (text, Map::new(), None),
    };
    let document = gq::parse_query::<String>(document_text)
        .map_err(|e| ImportError::Parse(e.to_string()))?
        .into_static();

    let mut fragments = HashMap::new();
    let mut operations = Vec::new();
    for definition in &document.definitions {
        match definition {
            gq::Definition::Fragment(fragment) => {
                fragments.insert(fragment.name.clone(), fragment);
            }
            gq::Definition::Operation(operation) => operations.push(Parts::of(operation)),
        }
    }
    let parts = match wanted {
        Some(name) => operations
            .iter()
            .find(|parts| parts.name == Some(name))
            .ok_or_else(|| ImportError::UnknownOperation(name.to_string()))?,
        None => operations.first().ok_or(ImportError::NoOperation)?,
    };

    // Defaults first, so a variable that was passed wins over its default.
    let mut variables = Map::new();
    for definition in parts.variables {
        if let Some(default) = &definition.default_value {
            variables.insert(definition.name.clone(), literal(default, &Map::new()));
        }
    }
    variables.extend(provided);
    let ctx = Ctx {
        schema,
        fragments,
        variables,
    };

    let mut roots = Vec::new();
    ctx.root_fields(&parts.set.items, &mut roots, &mut Vec::new());
    let (operation, ast) = roots
        .iter()
        .find_map(|ast| {
            let operation = OperationRef {
                root: parts.root,
                field: ast.name.clone(),
            };
            operation.field(schema).map(|_| (operation, *ast))
        })
        .ok_or(ImportError::NoRootField)?;
    let Some(field) = operation.field(schema) else {
        return Err(ImportError::NoRootField);
    };

    let mut out = Out {
        discarded: roots.len() - 1,
        ..Out::default()
    };
    ctx.field(&mut out, field, &mut Path::new(), ast);

    let composite = field
        .ty
        .named()
        .and_then(|name| schema.type_by_name(name))
        .is_some_and(|ty| is_composite(ty.kind));
    if composite && out.selection.leaves().next().is_none() {
        return Err(ImportError::EmptySelection);
    }
    let mut selection = out.selection;
    let mut values = Values::new();
    for pending in out.pending {
        // Arguments of a field that ended up with nothing selected are dropped with it.
        let kept = selection.has_under(&pending.field) || (pending.field.is_empty() && !composite);
        if !kept {
            continue;
        }
        if pending.optional {
            selection.insert_arg(pending.arg.clone());
        }
        if let Some(raw) = pending.raw {
            values.insert(pending.arg, raw);
        }
    }
    Ok(Imported {
        operation,
        selection,
        values,
        discarded: out.discarded,
    })
}

/// The parts of an operation definition that import cares about.
struct Parts<'d> {
    name: Option<&'d str>,
    root: Root,
    set: &'d gq::SelectionSet<'static, String>,
    variables: &'d [gq::VariableDefinition<'static, String>],
}

impl<'d> Parts<'d> {
    fn of(operation: &'d gq::OperationDefinition<'static, String>) -> Self {
        let (name, root, set, variables) = match operation {
            gq::OperationDefinition::SelectionSet(set) => (None, Root::Query, set, &[][..]),
            gq::OperationDefinition::Query(q) => (
                q.name.as_deref(),
                Root::Query,
                &q.selection_set,
                &q.variable_definitions[..],
            ),
            gq::OperationDefinition::Mutation(m) => (
                m.name.as_deref(),
                Root::Mutation,
                &m.selection_set,
                &m.variable_definitions[..],
            ),
            gq::OperationDefinition::Subscription(s) => (
                s.name.as_deref(),
                Root::Subscription,
                &s.selection_set,
                &s.variable_definitions[..],
            ),
        };
        Self {
            name,
            root,
            set,
            variables,
        }
    }
}

/// An argument seen in the paste; whether it is kept is decided once the selection is known.
struct Pending {
    /// The field the argument belongs to.
    field: Path,
    arg: Path,
    optional: bool,
    raw: Option<String>,
}

#[derive(Default)]
struct Out {
    selection: Selection,
    pending: Vec<Pending>,
    discarded: usize,
    /// Fragments being expanded, so a fragment that spreads itself ends.
    expanding: Vec<String>,
}

struct Ctx<'d> {
    schema: &'d Schema,
    fragments: HashMap<String, &'d gq::FragmentDefinition<'static, String>>,
    variables: Map<String, Value>,
}

type Items = [gq::Selection<'static, String>];

impl<'d> Ctx<'d> {
    /// Every field of the root selection set, looking through fragments.
    fn root_fields<'a>(
        &self,
        items: &'a Items,
        found: &mut Vec<&'a gq::Field<'static, String>>,
        expanding: &mut Vec<String>,
    ) where
        'd: 'a,
    {
        for item in items {
            match item {
                gq::Selection::Field(field) => found.push(field),
                gq::Selection::InlineFragment(inline) => {
                    self.root_fields(&inline.selection_set.items, found, expanding)
                }
                gq::Selection::FragmentSpread(spread) => {
                    let name = &spread.fragment_name;
                    if let Some(fragment) = self.fragments.get(name) {
                        if !expanding.contains(name) {
                            expanding.push(name.clone());
                            self.root_fields(&fragment.selection_set.items, found, expanding);
                            expanding.pop();
                        }
                    }
                }
            }
        }
    }

    fn walk_set(&self, out: &mut Out, ty: &FullType, path: &mut Path, items: &Items) {
        for item in items {
            match item {
                gq::Selection::Field(ast) => {
                    if ast.name == "__typename" {
                        continue;
                    }
                    let found = ty.fields.iter().flatten().find(|f| f.name == ast.name);
                    let Some(field) = found else {
                        out.discarded += 1;
                        continue;
                    };
                    path.push(Seg::Field(field.name.clone()));
                    self.field(out, field, path, ast);
                    path.pop();
                }
                gq::Selection::FragmentSpread(spread) => {
                    let name = &spread.fragment_name;
                    match self.fragments.get(name) {
                        Some(fragment) if !out.expanding.contains(name) => {
                            out.expanding.push(name.clone());
                            let gq::TypeCondition::On(on) = &fragment.type_condition;
                            self.typed(out, ty, path, on, &fragment.selection_set.items);
                            out.expanding.pop();
                        }
                        _ => out.discarded += 1,
                    }
                }
                gq::Selection::InlineFragment(inline) => match &inline.type_condition {
                    None => self.walk_set(out, ty, path, &inline.selection_set.items),
                    Some(gq::TypeCondition::On(on)) => {
                        self.typed(out, ty, path, on, &inline.selection_set.items)
                    }
                },
            }
        }
    }

    /// A selection under `... on <on>` inside a value of type `ty`.
    fn typed(&self, out: &mut Out, ty: &FullType, path: &mut Path, on: &str, items: &Items) {
        if ty.name.as_deref() == Some(on) {
            return self.walk_set(out, ty, path, items);
        }
        let Some(target) = self.schema.type_by_name(on) else {
            out.discarded += 1;
            return;
        };
        let names = |ty: &FullType, wanted: Option<&str>| {
            ty.possible_types
                .iter()
                .flatten()
                .any(|possible| possible.named() == wanted)
        };
        if names(ty, Some(on)) {
            path.push(Seg::Branch(on.to_string()));
            self.walk_set(out, target, path, items);
            path.pop();
        } else if names(target, ty.name.as_deref()) {
            // An interface this type implements: its fields are the type's own.
            self.walk_set(out, ty, path, items);
        } else {
            out.discarded += 1;
        }
    }

    /// The field at `path`: its arguments, then its subfields or itself as a leaf.
    fn field(
        &self,
        out: &mut Out,
        field: &Field,
        path: &mut Path,
        ast: &gq::Field<'static, String>,
    ) {
        for (name, value) in &ast.arguments {
            let Some(arg) = field.args.iter().find(|arg| &arg.name == name) else {
                out.discarded += 1;
                continue;
            };
            let mut arg_path = path.clone();
            arg_path.push(Seg::Args);
            arg_path.push(Seg::Arg(name.clone()));
            let raw = match literal(value, &self.variables) {
                Value::Null => None,
                value => Some(variables::display(&value)),
            };
            out.pending.push(Pending {
                field: path.clone(),
                arg: arg_path,
                optional: !arg.is_required(),
                raw,
            });
        }
        let target = field
            .ty
            .named()
            .and_then(|name| self.schema.type_by_name(name));
        match target {
            Some(inner) if is_composite(inner.kind) => {
                self.walk_set(out, inner, path, &ast.selection_set.items)
            }
            // A scalar root operation is the root itself: nothing to select.
            _ if !path.is_empty() => out.selection.insert(path.clone()),
            _ => {}
        }
    }
}

/// A GraphQL value as JSON, with variables replaced by what is known for them.
fn literal(value: &gq::Value<'static, String>, variables: &Map<String, Value>) -> Value {
    match value {
        gq::Value::Variable(name) => variables.get(name).cloned().unwrap_or(Value::Null),
        gq::Value::Int(number) => number.as_i64().map_or(Value::Null, Value::from),
        gq::Value::Float(number) => Number::from_f64(*number).map_or(Value::Null, Value::Number),
        gq::Value::String(text) => Value::String(text.clone()),
        gq::Value::Boolean(flag) => Value::Bool(*flag),
        gq::Value::Null => Value::Null,
        gq::Value::Enum(name) => Value::String(name.clone()),
        gq::Value::List(items) => {
            Value::Array(items.iter().map(|i| literal(i, variables)).collect())
        }
        gq::Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), literal(value, variables)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::querygen::generate;
    use crate::selection::parse_select;
    use crate::testutil::design_schema;

    fn imported(text: &str) -> Imported {
        import(&design_schema(), text).unwrap()
    }

    fn rebuilt(text: &str) -> String {
        let schema = design_schema();
        let found = import(&schema, text).unwrap();
        generate(&schema, &found.operation, &found.selection)
            .unwrap()
            .text
    }

    fn selected(operation: &str, select: &str) -> Selection {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, operation).unwrap();
        parse_select(&schema, &op, select).unwrap()
    }

    /// The path of an argument: the fields leading to it, then its name.
    fn arg(parts: &[&str]) -> Path {
        let (name, fields) = parts.split_last().unwrap();
        let mut path: Path = fields.iter().map(|f| Seg::Field(f.to_string())).collect();
        path.push(Seg::Args);
        path.push(Seg::Arg(name.to_string()));
        path
    }

    #[test]
    fn a_bare_document_becomes_the_selection_in_schema_order() {
        let found = imported("{ me { name id } }");
        assert_eq!(found.operation.label(), "Query.me");
        assert_eq!(found.selection, selected("Query.me", "id,name"));
        assert_eq!(found.discarded, 0);
    }

    #[test]
    fn the_query_is_recomposed_as_if_built_by_hand() {
        assert_eq!(
            rebuilt("query   Whatever { me { friends { name } id } }"),
            "query Me {\n  me {\n    id\n    friends {\n      name\n    }\n  }\n}"
        );
    }

    #[test]
    fn aliases_directives_and_typename_are_dropped_without_counting() {
        let found = imported("{ me { who: name @include(if: true) __typename id } }");
        assert_eq!(found.selection, selected("Query.me", "id,name"));
        assert_eq!(found.discarded, 0);
    }

    #[test]
    fn a_json_body_fills_variables_by_where_they_are_used() {
        let found = imported(
            r#"{"query": "query Q($n: Int!) { me { posts(first: $n) { title } } }",
                "variables": {"n": 3}}"#,
        );
        assert_eq!(found.selection, selected("Query.me", "posts.title"));
        assert_eq!(found.values.get(&arg(&["posts", "first"])).unwrap(), "3");
        assert!(!found.selection.arg_included(&arg(&["posts", "first"])));
    }

    #[test]
    fn a_default_fills_a_variable_that_was_not_passed_and_a_passed_value_wins() {
        let query = "query Q($n: Int = 7) { me { posts(first: $n) { title } } }";
        let found = imported(query);
        assert_eq!(found.values.get(&arg(&["posts", "first"])).unwrap(), "7");

        let body = serde_json::json!({"query": query, "variables": {"n": 2}}).to_string();
        let found = imported(&body);
        assert_eq!(found.values.get(&arg(&["posts", "first"])).unwrap(), "2");
    }

    #[test]
    fn inline_literals_fill_arguments_and_optional_ones_are_opted_in() {
        let found = imported(r#"{ users(limit: 2, active: true, role: ADMIN) { id } }"#);
        assert_eq!(found.values.get(&arg(&["limit"])).unwrap(), "2");
        assert_eq!(found.values.get(&arg(&["active"])).unwrap(), "true");
        assert_eq!(found.values.get(&arg(&["role"])).unwrap(), "ADMIN");
        assert!(found.selection.arg_included(&arg(&["active"])));
        assert!(found.selection.arg_included(&arg(&["role"])));
        // `limit` has a default, so it is optional; required arguments are never "opted in".
        assert!(found.selection.arg_included(&arg(&["limit"])));
    }

    #[test]
    fn an_optional_argument_without_a_value_is_still_opted_in() {
        let found = imported("query Q($a: Boolean) { users(limit: 1, active: $a) { id } }");
        assert!(found.selection.arg_included(&arg(&["active"])));
        assert!(!found.values.contains_key(&arg(&["active"])));
    }

    #[test]
    fn lists_and_objects_are_kept_as_json_text() {
        let found = imported(r#"{ users(limit: 1, ids: [1, "b"], filter: {title: "t"}) { id } }"#);
        assert_eq!(found.values.get(&arg(&["ids"])).unwrap(), r#"[1,"b"]"#);
        assert_eq!(
            found.values.get(&arg(&["filter"])).unwrap(),
            r#"{"title":"t"}"#
        );
    }

    #[test]
    fn what_the_schema_does_not_know_is_discarded_and_counted() {
        let found = imported("{ me { id nope posts(first: 1, bogus: 2) { title } } }");
        assert_eq!(found.selection, selected("Query.me", "id,posts.title"));
        assert_eq!(found.discarded, 2);
    }

    #[test]
    fn the_first_root_field_that_exists_is_kept() {
        let found = imported("{ nope me { id } version }");
        assert_eq!(found.operation.label(), "Query.me");
        assert_eq!(found.discarded, 2);
    }

    #[test]
    fn no_known_root_field_is_an_error() {
        let schema = design_schema();
        assert_eq!(
            import(&schema, "{ nope { id } }"),
            Err(ImportError::NoRootField)
        );
        assert_eq!(
            import(&schema, "mutation { me { id } }"),
            Err(ImportError::NoRootField)
        );
    }

    #[test]
    fn nothing_known_selected_below_a_known_root_is_an_error() {
        assert_eq!(
            import(&design_schema(), "{ me { nope } }"),
            Err(ImportError::EmptySelection)
        );
    }

    #[test]
    fn a_scalar_root_needs_no_selection() {
        let found = imported("{ postCount(since: \"2020\") }");
        assert!(found.selection.leaves().next().is_none());
        assert!(found.selection.arg_included(&arg(&["since"])));
        assert_eq!(found.values.get(&arg(&["since"])).unwrap(), "2020");
    }

    #[test]
    fn mutations_import_with_their_input() {
        let found = imported(r#"mutation { postCreate(input: {title: "x"}) { id } }"#);
        assert_eq!(found.operation.label(), "Mutation.postCreate");
        assert_eq!(
            found.values.get(&arg(&["input"])).unwrap(),
            r#"{"title":"x"}"#
        );
    }

    #[test]
    fn operation_name_picks_the_operation_and_an_unknown_one_is_an_error() {
        let document = "query A { me { id } } query B { me { name } }";
        let body =
            |name: &str| serde_json::json!({"query": document, "operationName": name}).to_string();
        assert_eq!(imported(&body("B")).selection, selected("Query.me", "name"));
        assert_eq!(imported(document).selection, selected("Query.me", "id"));
        assert_eq!(
            import(&design_schema(), &body("C")),
            Err(ImportError::UnknownOperation("C".to_string()))
        );
    }

    #[test]
    fn named_fragments_are_expanded_and_a_cycle_ends() {
        let found = imported(
            "{ me { ...F } } fragment F on User { id ...G } fragment G on User { name ...F }",
        );
        assert_eq!(found.selection, selected("Query.me", "id,name"));
    }

    #[test]
    fn inline_fragments_become_branches_and_unknown_types_are_discarded() {
        let found = imported(
            r#"{ search(term: "x") { __typename ... on User { name } ... on Post { title } ... on Nope { id } } }"#,
        );
        assert_eq!(
            found.selection,
            selected("Query.search", "on:User.name,on:Post.title")
        );
        assert_eq!(found.values.get(&arg(&["term"])).unwrap(), "x");
        assert_eq!(found.discarded, 1);
    }

    #[test]
    fn a_fragment_on_an_interface_the_type_implements_keeps_its_fields() {
        let found = imported("{ me { ... on Node { id } name } }");
        assert_eq!(found.selection, selected("Query.me", "id,name"));
    }

    #[test]
    fn arguments_of_a_field_with_nothing_selected_are_dropped_with_it() {
        let found = imported("{ me { id posts(first: 3) { nope } } }");
        assert_eq!(found.selection, selected("Query.me", "id"));
        assert!(found.values.is_empty());
    }

    #[test]
    fn text_that_is_not_graphql_is_a_parse_error() {
        assert!(matches!(
            import(&design_schema(), "{ me { id "),
            Err(ImportError::Parse(_))
        ));
        assert_eq!(
            import(&design_schema(), "fragment F on User { id }"),
            Err(ImportError::NoOperation)
        );
    }

    #[test]
    fn only_queries_are_told_apart_from_search_text() {
        for text in [
            "{ me { id } }",
            "  query Me { me { id } }",
            "mutation M($i: PostInput!) { postCreate(input: $i) { id } }",
            r#"{"query": "{ me { id } }"}"#,
        ] {
            assert!(looks_like_query(text), "{text}");
        }
        for text in ["userRead", "query", "query users", "mutation", "me }x"] {
            assert!(!looks_like_query(text), "{text}");
        }
    }

    #[test]
    fn what_a_copy_puts_on_the_clipboard_imports_back_to_the_same_design() {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.me").unwrap();
        let selection = parse_select(&schema, &op, "id,posts.title,friends.name").unwrap();
        let query = generate(&schema, &op, &selection).unwrap();
        let path = query.variables[0].path.clone();
        let values: Values = [(path.clone(), "4".to_string())].into();
        let copy = crate::export::request_json(&schema, &query, &values);

        let found = import(&schema, &copy.text).unwrap();
        assert_eq!(found.operation, op);
        assert_eq!(found.selection, selection);
        assert_eq!(found.values, values);
        assert_eq!(found.discarded, 0);
    }
}
