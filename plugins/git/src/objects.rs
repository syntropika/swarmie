//! Read-only object decoding. Native Git retains revision selection and all effects.

use std::fs;

use crate::{Cancellation, Commit, Error, RepositoryTarget, Result};

/// Bound decoded commit data independently of response serialization.
const HISTORY_OBJECT_BYTES: u64 = 4 * 1024 * 1024;

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("Git object observation failed: {error}"))
}

pub(crate) fn history(
    target: &RepositoryTarget,
    ids: &[&str],
    cancellation: &Cancellation,
) -> Result<Option<Vec<Commit>>> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if ids.len() > 200 {
        return Err(Error::OutputLimit);
    }
    // Isolated opening excludes global/system configuration, environment routing
    // and installation discovery. No command, filter or network feature is enabled.
    let options = gix::open::Options::isolated().config_overrides([
        if std::env::var_os("GIT_NO_REPLACE_OBJECTS").is_some() {
            "core.useReplaceRefs=false"
        } else {
            "core.useReplaceRefs=true"
        },
    ]);
    let repository = gix::open_opts(&target.git_dir, options).map_err(invalid)?;
    if fs::canonicalize(repository.git_dir())? != fs::canonicalize(&target.git_dir)?
        || fs::canonicalize(repository.common_dir())? != fs::canonicalize(&target.common_dir)?
    {
        return Err(Error::Stale);
    }
    let mut bytes = 0u64;
    let mut commits = Vec::with_capacity(ids.len());
    for record in ids {
        let mut fields = record.split_ascii_whitespace();
        let id = fields
            .next()
            .ok_or_else(|| invalid("missing commit identity"))?;
        let parents = fields
            .map(|value| {
                gix::hash::ObjectId::from_hex(value.as_bytes())
                    .map(|value| value.to_string())
                    .map_err(invalid)
            })
            .collect::<Result<Vec<_>>>()?;
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let id = gix::hash::ObjectId::from_hex(id.as_bytes()).map_err(invalid)?;
        let header = repository.find_header(id).map_err(invalid)?;
        bytes = bytes.checked_add(header.size()).ok_or(Error::OutputLimit)?;
        if bytes > HISTORY_OBJECT_BYTES {
            return Err(Error::OutputLimit);
        }
        let commit = repository.find_commit(id).map_err(invalid)?;
        if commit.data.len() as u64 > header.size() {
            return Err(Error::Stale);
        }
        let decoded = commit.decode().map_err(invalid)?;
        if decoded.encoding.is_some_and(|encoding| {
            !encoding.eq_ignore_ascii_case(b"utf-8") && !encoding.eq_ignore_ascii_case(b"utf8")
        }) {
            return Ok(None);
        }
        let author = commit.author().map_err(invalid)?;
        let summary = commit.message().map_err(invalid)?.summary();
        let (Ok(author_name), Ok(subject)) = (
            std::str::from_utf8(author.name.as_ref()),
            std::str::from_utf8(summary.as_ref()),
        ) else {
            return Ok(None);
        };
        commits.push(Commit {
            oid: id.to_string(),
            parents,
            author: author_name.to_owned(),
            authored_at: author
                .time()
                .map_err(invalid)?
                .format(gix::date::time::format::ISO8601_STRICT)
                .map_err(invalid)?,
            subject: subject.to_owned(),
        });
    }
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Some(commits))
}
