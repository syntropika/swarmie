use crate::{Error, Result, SecretLease};
use std::rc::Rc;
use zeroize::{Zeroize, Zeroizing};

/// Same-operation output custody. Call before *any* stdout/stderr/progress publication.
/// Exact supplied values are masked, including across arbitrary byte chunk boundaries.
/// Encoded/derived values and output from later operations are outside this guarantee.
pub struct OutputMask {
    leases: Vec<Rc<SecretLease>>,
    pending: Zeroizing<Vec<u8>>,
    overlap: usize,
}

impl OutputMask {
    pub fn new(leases: Vec<SecretLease>) -> Result<Self> {
        if leases.len() > 32 {
            return Err(Error::Invalid);
        }
        let mut longest = 1;
        for lease in &leases {
            lease.deliver(|value| {
                longest = longest.max(value.len());
                Ok(())
            })?;
        }
        Ok(Self {
            leases: leases.into_iter().map(Rc::new).collect(),
            pending: Zeroizing::new(vec![]),
            overlap: longest - 1,
        })
    }

    /// Separate stdout/stderr streams retain the same operation's private leases.
    pub fn fork(&self) -> Self {
        Self {
            leases: self.leases.clone(),
            pending: Zeroizing::new(vec![]),
            overlap: self.overlap,
        }
    }

    /// Supported JSON output is inspected after decoding as well as across raw byte chunks.
    pub fn mask_json(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                let private = Zeroizing::new(std::mem::take(text));
                *text = self.mask_text(&private);
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    self.mask_json(value);
                }
            }
            serde_json::Value::Object(values) => {
                let prior = std::mem::take(values);
                for (key, mut value) in prior {
                    let private = Zeroizing::new(key);
                    let key = self.mask_text(&private);
                    self.mask_json(&mut value);
                    values.insert(key, value);
                }
            }
            _ => {}
        }
    }

    pub fn has_secrets(&self) -> bool {
        !self.leases.is_empty()
    }
    fn mask_text(&self, text: &str) -> String {
        let mut mask = self.fork();
        let mut safe = mask.push(text.as_bytes());
        safe.extend(mask.finish());
        String::from_utf8(safe).expect("UTF-8 input and reference replacements")
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        self.release(false)
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.release(true)
    }

    pub fn finish_stream(&mut self) -> Vec<u8> {
        self.release(true)
    }

    fn release(&mut self, finished: bool) -> Vec<u8> {
        let safe = if finished {
            self.pending.len()
        } else {
            self.pending.len().saturating_sub(self.overlap)
        };
        let mut output = vec![];
        let mut consumed = 0;
        while consumed < safe {
            let mut matched: Option<(usize, Vec<u8>)> = None;
            for lease in &self.leases {
                // Infallible private delivery: the lease was acquired by this operation's owner.
                let _ = lease.deliver(|value| {
                    if self.pending[consumed..].starts_with(value)
                        && matched.as_ref().is_none_or(|m| value.len() > m.0)
                    {
                        matched = Some((value.len(), lease.reference.to_string().into_bytes()));
                    }
                    Ok(())
                });
            }
            if let Some((length, replacement)) = matched {
                output.extend_from_slice(&replacement);
                consumed += length;
            } else {
                output.push(self.pending[consumed]);
                consumed += 1;
            }
        }
        self.pending[..consumed].zeroize();
        self.pending.drain(..consumed);
        output
    }
}
