use crate::model::invalid;
use serde::{Deserialize, Serialize};

/// Observable committed revision. Domain records remain in storage.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub revision: u64,
    pub offset: usize,
}

/// A lossless fragment of one immutable JSON snapshot. This is an owner read projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub revision: u64,
    pub data: String,
    pub next: Option<Cursor>,
}

pub(crate) struct Encoded {
    pub revision: u64,
    pub data: String,
}

impl Encoded {
    pub fn page(&self, offset: usize) -> tembra::Result<Page> {
        if offset >= self.data.len() || !self.data.is_char_boundary(offset) {
            return Err(invalid("invalid read cursor offset"));
        }
        // The actual escaped envelope, rather than the source fragment, sets the wire bound.
        let mut end = offset.saturating_add(8 * 1024).min(self.data.len());
        while !self.data.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        loop {
            let data = self
                .data
                .get(offset..end)
                .ok_or_else(|| invalid("invalid read cursor"))?;
            let page = Page {
                revision: self.revision,
                data: data.into(),
                next: (end < self.data.len()).then_some(Cursor {
                    revision: self.revision,
                    offset: end,
                }),
            };
            let bytes =
                serde_json::to_vec(&page).map_err(|_| invalid("read page could not be encoded"))?;
            if bytes.len() <= 48 * 1024 {
                return Ok(page);
            }
            end = end
                .checked_sub(1)
                .ok_or_else(|| invalid("read page cannot fit the carrier"))?;
            while !self.data.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            if end <= offset {
                return Err(invalid("read page cannot fit the carrier"));
            }
        }
    }
}
