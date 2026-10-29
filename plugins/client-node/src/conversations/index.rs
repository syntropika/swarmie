//! Application metadata only; canonical messages remain in Conversations storage.
use super::{
    failure,
    types::{CatalogEntry, CatalogPage, Metadata},
};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf};

pub(super) struct Index(pub PathBuf);
impl Index {
    pub fn page(&self, cursor: Option<String>) -> tembra::Result<CatalogPage> {
        let entries: Vec<_> = self.read()?.into_iter().map(CatalogEntry::from).collect();
        let revision = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&entries)
                    .map_err(|_| failure("the conversation index could not be encoded"))?
            )
        );
        let offset = match cursor {
            None => 0,
            Some(cursor) => {
                let (seen, offset) = cursor
                    .split_once(':')
                    .ok_or_else(|| failure("invalid conversation cursor"))?;
                if seen != revision {
                    return Err(tembra::Error::new(
                        tembra::Code::Conflict,
                        "conversations.catalog_changed",
                        "the conversation catalog changed; reload its first page",
                    ));
                }
                offset
                    .parse::<usize>()
                    .map_err(|_| failure("invalid conversation cursor"))?
            }
        };
        if offset > entries.len() {
            return Err(failure("invalid conversation cursor"));
        }
        let total = entries.len();
        let mut page = Vec::new();
        let mut bytes = 0;
        for entry in entries.into_iter().skip(offset).take(32) {
            let size = serde_json::to_vec(&entry)
                .map_err(|_| failure("the conversation catalog could not be encoded"))?
                .len();
            if size > 40 * 1024 {
                return Err(failure(
                    "a conversation catalog entry exceeds the transport limit",
                ));
            }
            if bytes + size > 40 * 1024 {
                break;
            }
            bytes += size;
            page.push(entry);
        }
        let next = offset + page.len();
        Ok(CatalogPage {
            entries: page,
            next_cursor: (next < total).then(|| format!("{revision}:{next}")),
        })
    }
    pub fn read(&self) -> tembra::Result<Vec<Metadata>> {
        match std::fs::read(&self.0) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                failure("the conversation index could not be read; existing data was preserved")
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(_) => Err(failure(
                "the conversation index could not be read; existing data was preserved",
            )),
        }
    }
    pub fn get(&self, id: &str) -> tembra::Result<Option<Metadata>> {
        Ok(self.read()?.into_iter().find(|record| record.id == id))
    }

    pub fn write(&self, records: &[Metadata]) -> tembra::Result<()> {
        let parent = self
            .0
            .parent()
            .ok_or_else(|| failure("the conversation index is unavailable"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|_| failure("the conversation index could not be saved"))?;
        serde_json::to_writer(&mut temporary, records)
            .map_err(|_| failure("the conversation index could not be encoded"))?;
        temporary
            .flush()
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|_| failure("the conversation index could not be saved"))?;
        temporary
            .persist(&self.0)
            .map_err(|_| failure("the conversation index could not be saved"))?;
        Ok(())
    }
    pub fn change(&self, id: &str, change: impl FnOnce(&mut Metadata)) -> tembra::Result<()> {
        let mut records = self.read()?;
        let record = records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| failure("the conversation is not indexed"))?;
        change(record);
        self.write(&records)
    }
}
