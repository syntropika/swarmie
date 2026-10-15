use fs2::FileExt;
use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstRootNode},
};
use serde_json::Value;
use std::{fs, io::Write, path::Path};
use swarmie::{Error, Result};

pub(super) fn read(path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            let metadata = if metadata.file_type().is_symlink() {
                fs::metadata(path)?
            } else {
                metadata
            };
            if !metadata.is_file() || metadata.len() > 1024 * 1024 {
                return Err(Error::Invalid(
                    "Native MCP configuration must be a regular file under 1 MiB".into(),
                ));
            }
            fs::read_to_string(path).map_err(Into::into)
        }
    }
}
pub(super) fn json(text: &str) -> Result<Value> {
    let root = CstRootNode::parse(
        if text.is_empty() { "{}" } else { text },
        &ParseOptions::default(),
    )
    .map_err(|_| Error::Invalid("Native JSON configuration is malformed".into()))?;
    validate_keys(
        &root
            .value()
            .ok_or_else(|| Error::Invalid("Native JSON configuration is empty".into()))?,
    )?;
    let value: Value =
        jsonc_parser::parse_to_serde_value(&root.to_string(), &ParseOptions::default())
            .map_err(|_| Error::Invalid("Native JSON configuration is malformed".into()))?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(Error::Invalid(
            "Native JSON configuration must be an object".into(),
        ))
    }
}
fn validate_keys(node: &jsonc_parser::cst::CstNode) -> Result<()> {
    if let Some(object) = node.as_object() {
        let mut keys = std::collections::BTreeSet::new();
        for property in object.properties() {
            if !keys.insert(property.decoded_name()) {
                return Err(Error::Invalid(
                    "Native JSON configuration contains duplicate keys".into(),
                ));
            }
            if let Some(value) = property.value() {
                validate_keys(&value)?;
            }
        }
    }
    if let Some(array) = node.as_array() {
        for value in array.elements() {
            validate_keys(&value)?;
        }
    }
    Ok(())
}
fn input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.as_str().into()),
        Value::Array(value) => CstInputValue::Array(value.iter().map(input).collect()),
        Value::Object(value) => CstInputValue::Object(
            value
                .iter()
                .map(|(key, value)| (key.as_str().into(), input(value)))
                .collect(),
        ),
    }
}
pub(super) fn edit_json(
    text: &str,
    path: &[String],
    name: &str,
    value: Option<&Value>,
    patch: bool,
) -> Result<String> {
    let root = CstRootNode::parse(
        if text.is_empty() { "{}\n" } else { text },
        &ParseOptions::default(),
    )
    .map_err(|_| Error::Invalid("Native JSON configuration is malformed".into()))?;
    let mut object = root
        .object_value()
        .ok_or_else(|| Error::Invalid("Native configuration must be an object".into()))?;
    for key in path {
        if object.get(key).is_none() {
            object.append(key, CstInputValue::Object(vec![]));
        }
        object = object
            .get(key)
            .and_then(|value| value.object_value())
            .ok_or_else(|| Error::Invalid("Native MCP section must be an object".into()))?;
    }
    match value {
        None => {
            if let Some(property) = object.get(name) {
                property.remove();
            }
        }
        Some(value) if patch && object.get(name).is_some() => {
            let server = object
                .get(name)
                .unwrap()
                .object_value()
                .ok_or_else(|| Error::Invalid("Native MCP server must be an object".into()))?;
            for (key, value) in value
                .as_object()
                .ok_or_else(|| Error::Invalid("MCP settings must be an object".into()))?
            {
                if let Some(property) = server.get(key) {
                    property.set_value(input(value));
                } else {
                    server.append(key, input(value));
                }
            }
        }
        Some(value) => {
            if let Some(property) = object.get(name) {
                property.set_value(input(value));
            } else {
                object.append(name, input(value));
            }
        }
    }
    Ok(root.to_string())
}
pub(super) fn toml(text: &str) -> Result<Value> {
    toml_edit::de::from_str(if text.is_empty() { "" } else { text })
        .map_err(|_| Error::Invalid("Native TOML configuration is malformed".into()))
}
pub(super) fn edit_toml(
    text: &str,
    name: &str,
    value: Option<&Value>,
    patch: bool,
) -> Result<String> {
    let mut document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| Error::Invalid("Native TOML configuration is malformed".into()))?;
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_like_mut()
        .ok_or_else(|| Error::Invalid("Native MCP section must be a table".into()))?;
    if let Some(value) = value {
        let replacement = toml_edit::ser::to_document(value)
            .map_err(|_| Error::Invalid("MCP settings cannot be encoded".into()))?;
        if patch && servers.get(name).is_some() {
            let table = servers
                .get_mut(name)
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or_else(|| Error::Invalid("Native MCP server must be a table".into()))?;
            for (key, value) in replacement.as_table() {
                let mut value = value.clone();
                if let Some(previous) = table.get(key).and_then(toml_edit::Item::as_value)
                    && let Some(next) = value.as_value_mut()
                {
                    *next.decor_mut() = previous.decor().clone();
                }
                table.insert(key, value);
            }
        } else {
            servers.insert(name, toml_edit::Item::Table(replacement.as_table().clone()));
        }
    } else {
        servers.remove(name);
    }
    Ok(document.to_string())
}
/// Serialize cooperating writers, preserve permissions and compare again before atomic replacement.
pub(super) fn write(path: &Path, expected: &str, text: &str) -> Result<()> {
    if !cfg!(unix) {
        return Err(Error::Unsupported(
            "Native MCP file writes are qualified on Unix platforms only".into(),
        ));
    }
    let canonical;
    let path = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        canonical = fs::canonicalize(path)?;
        canonical.as_path()
    } else {
        path
    };
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Native configuration has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let lock_path = parent.join(format!(
        ".{}.swarmie-lock",
        path.file_name().unwrap().to_string_lossy()
    ));
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock_exclusive()?;
    if read(path)? != expected {
        return Err(Error::Rejected(
            "Native configuration changed; refresh before trying again".into(),
        ));
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.permissions().readonly() {
            return Err(Error::Rejected("Native configuration is read-only".into()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let created = file.as_file().metadata()?;
            if created.uid() != metadata.uid() || created.gid() != metadata.gid() {
                std::os::unix::fs::chown(file.path(), Some(metadata.uid()), Some(metadata.gid()))?;
            }
        }
        file.as_file().set_permissions(metadata.permissions())?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    if read(path)? != expected {
        return Err(Error::Rejected(
            "Native configuration changed; refresh before trying again".into(),
        ));
    }
    file.persist(path).map_err(|error| Error::Io(error.error))?;
    #[cfg(unix)]
    {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
