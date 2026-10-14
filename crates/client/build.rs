use std::{env, fs, path::PathBuf};

use wit_parser::{Resolve, Type, TypeDefKind};

fn title(name: &str) -> String {
    name.split('-')
        .map(|part| format!("{}{}", part[..1].to_uppercase(), &part[1..]))
        .collect()
}

fn rust_type(resolve: &Resolve, ty: Type) -> String {
    match ty {
        Type::String => "String".into(),
        Type::U64 => "u64".into(),
        Type::S64 => "i64".into(),
        Type::Id(id) => {
            let def = &resolve.types[id];
            if let Some(name) = &def.name {
                return title(name);
            }
            match &def.kind {
                TypeDefKind::Option(ty) => format!("Option<{}>", rust_type(resolve, *ty)),
                TypeDefKind::List(ty) => format!("Vec<{}>", rust_type(resolve, *ty)),
                TypeDefKind::Result(result) => format!(
                    "Result<{}, {}>",
                    result
                        .ok
                        .map(|ty| rust_type(resolve, ty))
                        .unwrap_or("()".into()),
                    result
                        .err
                        .map(|ty| rust_type(resolve, ty))
                        .unwrap_or("()".into())
                ),
                other => panic!("unsupported boundary type: {other:?}"),
            }
        }
        other => panic!("unsupported boundary primitive: {other:?}"),
    }
}

fn main() {
    let path =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../sdk/wit/client.wit");
    println!("cargo:rerun-if-changed={}", path.display());
    let mut resolve = Resolve::default();
    let package = resolve.push_file(&path).expect("valid client WIT");
    let interface = &resolve.interfaces[resolve.packages[package].interfaces["native"]];
    let mut output = String::new();
    for (name, id) in &interface.types {
        output.push_str("#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\n");
        match &resolve.types[*id].kind {
            TypeDefKind::Record(record) => {
                output.push_str(&format!(
                    "#[serde(deny_unknown_fields)]\npub struct {} {{\n",
                    title(name)
                ));
                for field in &record.fields {
                    output.push_str(&format!(
                        "pub {}: {},\n",
                        field.name.replace('-', "_"),
                        rust_type(&resolve, field.ty)
                    ));
                }
            }
            TypeDefKind::Enum(cases) => {
                output.push_str(&format!(
                    "#[serde(rename_all = \"snake_case\")]\npub enum {} {{\n",
                    title(name)
                ));
                for case in &cases.cases {
                    output.push_str(&format!("{},\n", title(&case.name)));
                }
            }
            other => panic!("unsupported named type: {other:?}"),
        }
        output.push_str("}\n");
    }
    output.push_str("#[derive(Debug, serde::Deserialize)]\n#[serde(tag = \"action\", rename_all = \"snake_case\", deny_unknown_fields)]\npub enum Command {\n");
    for function in interface.functions.values() {
        output.push_str(&format!("{} {{", title(&function.name)));
        for param in &function.params {
            output.push_str(&format!(
                "{}: {},",
                param.name.replace('-', "_"),
                rust_type(&resolve, param.ty)
            ));
        }
        output.push_str("},\n");
    }
    output.push_str("}\npub trait Native {\n");
    for function in interface.functions.values() {
        output.push_str(&format!(
            "async fn {}(&self",
            function.name.replace('-', "_")
        ));
        for param in &function.params {
            output.push_str(&format!(
                ", {}: {}",
                param.name.replace('-', "_"),
                rust_type(&resolve, param.ty)
            ));
        }
        output.push_str(&format!(
            ") -> {};\n",
            rust_type(&resolve, function.result.unwrap())
        ));
    }
    output.push_str("}\npub async fn dispatch(host: &impl Native, command: Command) -> Result<serde_json::Value, ClientError> {\nmatch command {\n");
    for function in interface.functions.values() {
        let names: Vec<_> = function
            .params
            .iter()
            .map(|p| p.name.replace('-', "_"))
            .collect();
        output.push_str(&format!("Command::{} {{ {} }} => serde_json::to_value(host.{}({}).await?).map_err(|_| ClientError::internal()),\n", title(&function.name), names.join(","), function.name.replace('-', "_"), names.join(",")));
    }
    output.push_str("}}\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("boundary.rs"),
        output,
    )
    .unwrap();
}
