pub fn split_qualified_name(object_name: &str) -> (Option<String>, String) {
    if let Some((schema, name)) = object_name.split_once('.') {
        (Some(schema.to_string()), name.to_string())
    } else {
        (None, object_name.to_string())
    }
}