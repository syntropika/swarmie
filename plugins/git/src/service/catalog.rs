use super::*;
use crate::{ArchiveListing, PageCursor};

#[derive(Serialize)]
#[serde(untagged)]
enum Entry {
    Archive(Box<ArchiveReceipt>),
    Unavailable(ArchiveUnavailable),
}

pub(super) fn archive_page(
    mut listing: ArchiveListing,
    page: Option<PageCursor>,
) -> Result<WireResponse> {
    listing
        .archives
        .sort_by(|left, right| left.archive_id.cmp(&right.archive_id));
    listing
        .unavailable
        .sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
    let entries: Vec<Entry> = listing
        .archives
        .into_iter()
        .map(|entry| Entry::Archive(Box::new(entry)))
        .chain(listing.unavailable.into_iter().map(Entry::Unavailable))
        .collect();
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&entries).map_err(invalid_json)?)
    );
    let offset = match page {
        Some(page) if page.digest == digest && page.offset > 0 && page.offset <= entries.len() => {
            page.offset
        }
        Some(_) => return Err(Error::Stale),
        None => 0,
    };
    let total = entries.len();
    let mut bytes = 0;
    let mut consumed = 0;
    let mut archives = Vec::new();
    let mut unavailable = Vec::new();
    for entry in entries.into_iter().skip(offset) {
        let size = serde_json::to_vec(&entry).map_err(invalid_json)?.len();
        let entry = if size > 20 * 1024 {
            Entry::Unavailable(ArchiveUnavailable{entry_id:format!("{:x}",Sha256::digest(serde_json::to_vec(&entry).map_err(invalid_json)?)),message:"This archive catalog entry exceeds its display limit. Its files remain retained.".into()})
        } else {
            entry
        };
        let size = serde_json::to_vec(&entry).map_err(invalid_json)?.len();
        if consumed > 0 && bytes + size > 24 * 1024 {
            break;
        }
        bytes += size;
        consumed += 1;
        match entry {
            Entry::Archive(entry) => archives.push(*entry),
            Entry::Unavailable(entry) => unavailable.push(entry),
        }
    }
    let next = offset + consumed;
    Ok(WireResponse::Managed(ManagedResponse::Archives {
        archives,
        unavailable,
        next_page: (next < total).then_some(PageCursor {
            offset: next,
            digest,
        }),
    }))
}
