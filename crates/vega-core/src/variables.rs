//! Typed values for the variables of a generated query: which editor a type needs and
//! whether the text typed so far is a valid value for it. Pure data: no terminal code.

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};

use crate::querygen::VariableDef;
use crate::schema::{Schema, TypeKind, TypeRef};
use crate::selection::Path;

/// What the user typed for each variable, keyed by where its argument sits in the tree.
pub type Values = BTreeMap<Path, String>;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Nothing typed and nothing needed: the variable is left out of the request.
    Unset,
    Value(Value),
    /// Why the text typed so far cannot be sent.
    Invalid(String),
}

/// How the user edits a value of a given type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editor {
    /// Free text (numbers, IDs, strings, custom scalars).
    Text,
    /// `Boolean`: cycles with Enter.
    Toggle,
    /// An enum: pick one of its values.
    Choice(Vec<String>),
    /// A list or an input object: one JSON value.
    Json,
}

fn strip_non_null(ty: &TypeRef) -> &TypeRef {
    match (ty.kind, ty.of_type.as_deref()) {
        (TypeKind::NonNull, Some(inner)) => inner,
        _ => ty,
    }
}

fn enum_names(schema: &Schema, name: &str) -> Vec<String> {
    schema
        .type_by_name(name)
        .and_then(|ty| ty.enum_values.as_ref())
        .map(|values| values.iter().map(|value| value.name.clone()).collect())
        .unwrap_or_default()
}

pub fn editor_for(schema: &Schema, ty: &TypeRef) -> Editor {
    let ty = strip_non_null(ty);
    match ty.kind {
        TypeKind::List | TypeKind::InputObject => Editor::Json,
        TypeKind::Enum => {
            Editor::Choice(enum_names(schema, ty.name.as_deref().unwrap_or_default()))
        }
        TypeKind::Scalar if ty.name.as_deref() == Some("Boolean") => Editor::Toggle,
        _ => Editor::Text,
    }
}

/// The state of `def` given the text typed for it (`None` when nothing was typed).
pub fn state(schema: &Schema, def: &VariableDef, raw: Option<&str>) -> State {
    let text = raw.unwrap_or_default();
    if text.trim().is_empty() {
        return if def.required {
            State::Invalid("required".to_string())
        } else {
            State::Unset
        };
    }
    match parse(schema, &def.type_ref, text) {
        Ok(value) => State::Value(value),
        Err(reason) => State::Invalid(reason),
    }
}

/// A value as shown to the user: strings without quotes, everything else compact JSON.
pub fn display(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// The `variables` object of a request: every valid value, unset ones left out. The error
/// names the first variable that is invalid or missing (`$id: required`).
pub fn to_json(
    schema: &Schema,
    defs: &[VariableDef],
    values: &Values,
) -> Result<Map<String, Value>, String> {
    let mut map = Map::new();
    for def in defs {
        match state(schema, def, values.get(&def.path).map(String::as_str)) {
            State::Value(value) => {
                map.insert(def.name.clone(), value);
            }
            State::Unset => {}
            State::Invalid(reason) => return Err(format!("${}: {reason}", def.name)),
        }
    }
    Ok(map)
}

fn parse(schema: &Schema, original: &TypeRef, text: &str) -> Result<Value, String> {
    let ty = strip_non_null(original);
    match ty.kind {
        TypeKind::List | TypeKind::InputObject => {
            let value: Value =
                serde_json::from_str(text.trim()).map_err(|e| format!("not valid JSON: {e}"))?;
            validate(schema, original, &value)?;
            Ok(value)
        }
        _ => parse_scalar(schema, ty, text),
    }
}

fn parse_scalar(schema: &Schema, ty: &TypeRef, text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    let name = ty.name.as_deref().unwrap_or_default();
    match (ty.kind, name) {
        (TypeKind::Enum, _) => {
            if enum_names(schema, name).iter().any(|n| n == trimmed) {
                Ok(Value::String(trimmed.to_string()))
            } else {
                Err(format!("`{trimmed}` is not a value of {name}"))
            }
        }
        (_, "Int") => trimmed
            .parse::<i32>()
            .map(Value::from)
            .map_err(|_| "expected a whole number".to_string()),
        (_, "Float") => trimmed
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "expected a number".to_string()),
        (_, "Boolean") => match trimmed {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("expected true or false".to_string()),
        },
        // ID, String and custom scalars: the text as typed.
        _ => Ok(Value::String(text.to_string())),
    }
}

fn validate(schema: &Schema, ty: &TypeRef, value: &Value) -> Result<(), String> {
    match ty.kind {
        TypeKind::NonNull => {
            if value.is_null() {
                return Err("must not be null".to_string());
            }
            match ty.of_type.as_deref() {
                Some(inner) => validate(schema, inner, value),
                None => Ok(()),
            }
        }
        _ if value.is_null() => Ok(()),
        TypeKind::List => {
            let Some(items) = value.as_array() else {
                return Err("expected a list".to_string());
            };
            let Some(inner) = ty.of_type.as_deref() else {
                return Ok(());
            };
            for (index, item) in items.iter().enumerate() {
                validate(schema, inner, item).map_err(|reason| format!("[{index}]: {reason}"))?;
            }
            Ok(())
        }
        TypeKind::InputObject => {
            validate_object(schema, ty.name.as_deref().unwrap_or_default(), value)
        }
        _ => validate_scalar(schema, ty, value),
    }
}

fn validate_object(schema: &Schema, name: &str, value: &Value) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err(format!("expected an object of type {name}"));
    };
    let Some(fields) = schema
        .type_by_name(name)
        .and_then(|ty| ty.input_fields.as_ref())
    else {
        return Ok(());
    };
    for key in object.keys() {
        if !fields.iter().any(|field| &field.name == key) {
            return Err(format!("unknown field `{key}`"));
        }
    }
    for field in fields {
        match object.get(&field.name) {
            Some(inner) => validate(schema, &field.ty, inner)
                .map_err(|reason| format!("{}: {reason}", field.name))?,
            None if field.is_required() => return Err(format!("missing field `{}`", field.name)),
            None => {}
        }
    }
    Ok(())
}

fn validate_scalar(schema: &Schema, ty: &TypeRef, value: &Value) -> Result<(), String> {
    let name = ty.name.as_deref().unwrap_or_default();
    let ok = |condition: bool, reason: &str| {
        if condition {
            Ok(())
        } else {
            Err(reason.to_string())
        }
    };
    match (ty.kind, name) {
        (TypeKind::Enum, _) => match value.as_str() {
            Some(text) if enum_names(schema, name).iter().any(|n| n == text) => Ok(()),
            Some(text) => Err(format!("`{text}` is not a value of {name}")),
            None => Err(format!("expected a value of {name}")),
        },
        (_, "Int") => ok(
            value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
            "expected a whole number",
        ),
        (_, "Float") => ok(value.is_number(), "expected a number"),
        (_, "Boolean") => ok(value.is_boolean(), "expected true or false"),
        (_, "String") => ok(value.is_string(), "expected a string"),
        // IDs may be written as strings or whole numbers.
        (_, "ID") => ok(
            value.is_string() || value.as_i64().is_some(),
            "expected an ID",
        ),
        // Custom scalars are not validated.
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::querygen::{generate, GeneratedQuery};
    use crate::selection::{parse_select, OperationRef};
    use crate::testutil::design_schema;
    use serde_json::json;

    fn users() -> (Schema, GeneratedQuery) {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.users").unwrap();
        let selection = parse_select(
            &schema,
            &op,
            "id,arg:role,arg:active,arg:floor,arg:ids,arg:filter,arg:limit",
        )
        .unwrap();
        let query = generate(&schema, &op, &selection).unwrap();
        (schema, query)
    }

    fn state_of(name: &str, raw: &str) -> State {
        let (schema, query) = users();
        let def = query.variables.iter().find(|v| v.name == name).unwrap();
        state(&schema, def, Some(raw))
    }

    fn invalid(reason: &str) -> State {
        State::Invalid(reason.to_string())
    }

    #[test]
    fn scalars_and_enums_are_parsed_by_type() {
        let cases: Vec<(&str, &str, State)> = vec![
            ("active", "true", State::Value(json!(true))),
            ("active", "false", State::Value(json!(false))),
            ("active", "yes", invalid("expected true or false")),
            ("floor", "1.5", State::Value(json!(1.5))),
            ("floor", "-2", State::Value(json!(-2.0))),
            ("floor", "abc", invalid("expected a number")),
            ("limit", "5", State::Value(json!(5))),
            ("limit", "5.5", invalid("expected a whole number")),
            ("limit", "99999999999", invalid("expected a whole number")),
            ("role", "ADMIN", State::Value(json!("ADMIN"))),
            ("role", "root", invalid("`root` is not a value of Role")),
        ];
        for (name, raw, expected) in cases {
            assert_eq!(state_of(name, raw), expected, "{name} = {raw}");
        }
    }

    #[test]
    fn lists_and_input_objects_are_json_validated_against_the_type() {
        let cases: Vec<(&str, &str, State)> = vec![
            ("ids", r#"["1","2"]"#, State::Value(json!(["1", "2"]))),
            ("ids", "[1, 2]", State::Value(json!([1, 2]))),
            ("ids", r#""x""#, invalid("expected a list")),
            ("ids", "[null]", invalid("[0]: must not be null")),
            (
                "filter",
                r#"{"title":"x"}"#,
                State::Value(json!({"title": "x"})),
            ),
            ("filter", "{}", invalid("missing field `title`")),
            (
                "filter",
                r#"{"title":"x","z":1}"#,
                invalid("unknown field `z`"),
            ),
            (
                "filter",
                r#"{"title":1}"#,
                invalid("title: expected a string"),
            ),
            ("filter", "null", State::Value(serde_json::Value::Null)),
        ];
        for (name, raw, expected) in cases {
            assert_eq!(state_of(name, raw), expected, "{name} = {raw}");
        }
        assert!(matches!(
            state_of("ids", "["),
            State::Invalid(reason) if reason.starts_with("not valid JSON")
        ));
    }

    fn hand_built(ty_json: &str) -> VariableDef {
        VariableDef {
            name: "v".to_string(),
            ty: String::new(),
            required: true,
            default: None,
            path: Vec::new(),
            type_ref: serde_json::from_str::<TypeRef>(ty_json).unwrap(),
        }
    }

    #[test]
    fn required_list_and_input_object_reject_null_but_accept_valid_values() {
        let schema = design_schema();
        let required_ids = hand_built(
            r#"{"kind":"NON_NULL","name":null,"ofType":{"kind":"LIST","name":null,"ofType":{"kind":"NON_NULL","name":null,"ofType":{"kind":"SCALAR","name":"ID","ofType":null}}}}"#,
        );
        let required_post = hand_built(
            r#"{"kind":"NON_NULL","name":null,"ofType":{"kind":"INPUT_OBJECT","name":"PostInput","ofType":null}}"#,
        );
        let cases = [
            (&required_ids, "null", invalid("must not be null")),
            (&required_ids, r#"["1"]"#, State::Value(json!(["1"]))),
            (&required_post, "null", invalid("must not be null")),
            (
                &required_post,
                r#"{"title":"x"}"#,
                State::Value(json!({"title": "x"})),
            ),
        ];
        for (def, raw, expected) in cases {
            assert_eq!(state(&schema, def, Some(raw)), expected, "{raw}");
        }
    }

    #[test]
    fn nested_lists_report_each_index_in_the_path() {
        let schema = design_schema();
        let def = hand_built(
            r#"{"kind":"LIST","name":null,"ofType":{"kind":"LIST","name":null,"ofType":{"kind":"SCALAR","name":"Int","ofType":null}}}"#,
        );
        assert_eq!(
            state(&schema, &def, Some(r#"[[1],[2,"x"]]"#)),
            invalid("[1]: [1]: expected a whole number")
        );
    }

    #[test]
    fn empty_text_leaves_an_optional_variable_unset() {
        assert_eq!(state_of("role", ""), State::Unset);
        assert_eq!(state_of("role", "   "), State::Unset);
        // `limit` is non-null but has a default, so it is optional too.
        assert_eq!(state_of("limit", ""), State::Unset);
    }

    #[test]
    fn required_variables_cannot_be_empty_and_text_keeps_its_spaces() {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.node").unwrap();
        let selection = parse_select(&schema, &op, "id").unwrap();
        let query = generate(&schema, &op, &selection).unwrap();
        let def = &query.variables[0];
        assert_eq!(state(&schema, def, None), invalid("required"));
        assert_eq!(state(&schema, def, Some("")), invalid("required"));
        assert_eq!(state(&schema, def, Some("42")), State::Value(json!("42")));
        assert_eq!(
            state(&schema, def, Some(" a b ")),
            State::Value(json!(" a b "))
        );
    }

    #[test]
    fn editors_follow_the_type() {
        let (schema, query) = users();
        let editor = |name: &str| {
            let def = query.variables.iter().find(|v| v.name == name).unwrap();
            editor_for(&schema, &def.type_ref)
        };
        assert_eq!(
            editor("role"),
            Editor::Choice(vec!["ADMIN".to_string(), "VIEWER".to_string()])
        );
        assert_eq!(editor("active"), Editor::Toggle);
        assert_eq!(editor("floor"), Editor::Text);
        assert_eq!(editor("limit"), Editor::Text);
        assert_eq!(editor("ids"), Editor::Json);
        assert_eq!(editor("filter"), Editor::Json);
    }

    #[test]
    fn display_shows_strings_without_quotes() {
        assert_eq!(display(&json!("42")), "42");
        assert_eq!(display(&json!(7)), "7");
        assert_eq!(display(&json!({"a": 1})), r#"{"a":1}"#);
    }

    #[test]
    fn to_json_keeps_valid_values_and_omits_unset_ones() {
        let (schema, query) = users();
        let mut values = Values::new();
        let role = query.variables.iter().find(|v| v.name == "role").unwrap();
        values.insert(role.path.clone(), "ADMIN".to_string());
        let map = to_json(&schema, &query.variables, &values).unwrap();
        assert_eq!(serde_json::Value::Object(map), json!({"role": "ADMIN"}));
    }

    #[test]
    fn to_json_names_the_first_problem() {
        let (schema, query) = users();
        let mut values = Values::new();
        let floor = query.variables.iter().find(|v| v.name == "floor").unwrap();
        values.insert(floor.path.clone(), "abc".to_string());
        assert_eq!(
            to_json(&schema, &query.variables, &values),
            Err("$floor: expected a number".to_string())
        );

        let op = OperationRef::parse(&schema, "Query.node").unwrap();
        let selection = parse_select(&schema, &op, "id").unwrap();
        let node = generate(&schema, &op, &selection).unwrap();
        assert_eq!(
            to_json(&schema, &node.variables, &Values::new()),
            Err("$id: required".to_string())
        );
    }
}
