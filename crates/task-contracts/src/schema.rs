/// Schemars represents internally tagged enums with oneOf. Their unique required `type`
/// constants make branches disjoint, so anyOf has identical validation semantics and fits
/// Tembra's portable schema subset. Apply only to the tagged enums owned by this crate.
pub(crate) fn tagged_union(schema: &mut tembra::schemars::Schema) {
    if let Some(object) = schema.as_object_mut()
        && let Some(branches) = object.remove("oneOf")
    {
        object.insert("anyOf".into(), branches);
    }
}
