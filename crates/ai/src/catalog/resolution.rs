use crate::catalog::{
    Capability, Evidence, Layer, Limit, LimitClaim, Metadata, Precision, Preparation,
    ResolutionError, Snapshot, SourceRecord, UnknownReason,
};

pub(crate) fn validate_record(record: &SourceRecord) -> Result<(), ResolutionError> {
    for claim in record
        .limits
        .context
        .iter()
        .chain(&record.limits.input)
        .chain(&record.limits.output)
    {
        validate_evidence(&claim.evidence)?;
        if claim.evidence.layer == Layer::Adapter {
            return Err(ResolutionError::InvalidEvidence);
        }
    }
    validate_capability(&record.tools)?;
    validate_capability(&record.streaming)?;
    validate_capability(&record.native_compaction)?;
    validate_capability(&record.native_output_maximum)?;
    if let Capability::Supported { value, .. } = &record.native_output_maximum {
        crate::catalog::cache::validate_configuration_revision(&value.configuration_revision)?;
        validate_evidence(&value.fact)?;
        if !matches!(value.fact.layer, Layer::Provider | Layer::Deployment) {
            return Err(ResolutionError::InvalidEvidence);
        }
    }
    if let Capability::Supported { value, .. } = &record.native_compaction {
        for text in [
            &value.method,
            &value.input_format,
            &value.output_format,
            &value.continuation_format,
        ] {
            if text.trim().is_empty() || text.len() > 1024 || text.chars().any(char::is_control) {
                return Err(ResolutionError::InvalidEvidence);
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_evidence(evidence: &Evidence) -> Result<(), ResolutionError> {
    if [&evidence.source, &evidence.revision].iter().any(|text| {
        text.trim().is_empty() || text.len() > 4096 || text.chars().any(char::is_control)
    }) || evidence
        .expires_at_ms
        .is_some_and(|expiry| expiry <= evidence.observed_at_ms)
    {
        return Err(ResolutionError::InvalidEvidence);
    }
    Ok(())
}

fn validate_capability<T>(capability: &Capability<T>) -> Result<(), ResolutionError> {
    match capability {
        Capability::Unknown { .. } => Ok(()),
        Capability::Supported { evidence, .. } | Capability::Unsupported { evidence, .. } => {
            validate_evidence(evidence)?;
            if evidence.layer != Layer::Adapter {
                return Err(ResolutionError::InvalidEvidence);
            }
            Ok(())
        }
    }
}

struct Resolver {
    now: u64,
    stale_for: u64,
    discarded: Vec<LimitClaim>,
    stale: Vec<Evidence>,
    until: Option<u64>,
}

impl Resolver {
    fn admit(&mut self, evidence: &Evidence, allow_stale: bool) -> bool {
        if evidence.observed_at_ms > self.now {
            self.until = Some(self.until.map_or(evidence.observed_at_ms, |old| {
                old.min(evidence.observed_at_ms)
            }));
            return false;
        }
        if let Some(expiry) = evidence.expires_at_ms {
            let until = expiry.saturating_add(if allow_stale { self.stale_for } else { 0 });
            if self.now >= until {
                return false;
            }
            self.until = Some(self.until.map_or(until, |old| old.min(until)));
            // Reprepare at the fresh/stale boundary so stale use is visible.
            if self.now < expiry {
                self.until = Some(self.until.unwrap_or(expiry).min(expiry));
            } else if !self.stale.contains(evidence) {
                self.stale.push(evidence.clone());
            }
        }
        true
    }

    fn limit(
        &mut self,
        claims: impl Iterator<Item = LimitClaim>,
        context: Option<&Limit>,
        derive_from_context: bool,
    ) -> Metadata<Limit> {
        let mut accepted = Vec::new();
        let mut reason = UnknownReason::Missing;
        for claim in claims {
            if claim.precision == Precision::Estimated {
                reason = UnknownReason::EstimatedOnly;
                self.discarded.push(claim);
            } else if !self.admit(&claim.evidence, true) {
                reason = if claim.evidence.observed_at_ms > self.now {
                    UnknownReason::NotYetValid
                } else {
                    UnknownReason::Expired
                };
                self.discarded.push(claim);
            } else {
                accepted.push(claim);
            }
        }
        accepted.sort_by_cached_key(|claim| {
            serde_json::to_string(claim).expect("metadata is serializable")
        });
        accepted.dedup();
        let has_fact = accepted
            .iter()
            .any(|claim| claim.evidence.layer != Layer::Policy);
        if !has_fact && !(derive_from_context && context.is_some()) {
            self.discarded.extend(accepted);
            return Metadata::Unknown { reason };
        }
        let ceiling = accepted.iter().map(|claim| claim.tokens).min();
        let derived = !has_fact && context.is_some();
        let bounded =
            context.is_some_and(|c| ceiling.is_none_or(|tokens| c.tokens < tokens)) || derived;
        let tokens = match (ceiling, context) {
            (Some(tokens), Some(context)) => tokens.min(context.tokens),
            (Some(tokens), None) => tokens,
            (None, Some(context)) => context.tokens,
            (None, None) => return Metadata::Unknown { reason },
        };
        let mut precision = if bounded {
            Precision::Conservative
        } else {
            Precision::Reported
        };
        if accepted
            .iter()
            .any(|claim| claim.precision == Precision::Conservative)
        {
            precision = Precision::Conservative;
        }
        if let Some(context) = context {
            accepted.extend(context.claims.clone());
        }
        Metadata::Known {
            value: Limit {
                tokens,
                precision,
                claims: accepted,
                bounded_by_context: bounded,
            },
        }
    }

    fn capability<T: Clone + PartialEq>(
        &mut self,
        field: &'static str,
        claims: impl Iterator<Item = Capability<T>>,
    ) -> Result<Capability<T>, ResolutionError> {
        let mut result = Capability::default();
        for claim in claims {
            let evidence = match &claim {
                Capability::Unknown { reason } => {
                    if matches!(
                        result,
                        Capability::Unknown {
                            reason: UnknownReason::Missing
                        }
                    ) {
                        result = Capability::Unknown {
                            reason: reason.clone(),
                        };
                    }
                    continue;
                }
                Capability::Unsupported { evidence, .. }
                | Capability::Supported { evidence, .. } => evidence,
            };
            // A stale native qualification is not an executable capability.
            if !self.admit(evidence, false) {
                if matches!(result, Capability::Unknown { .. }) {
                    result = Capability::Unknown {
                        reason: if evidence.observed_at_ms > self.now {
                            UnknownReason::NotYetValid
                        } else {
                            UnknownReason::Expired
                        },
                    };
                }
                continue;
            }
            if !matches!(result, Capability::Unknown { .. }) && result != claim {
                return Err(ResolutionError::ConflictingCapability(field));
            }
            result = claim;
        }
        Ok(result)
    }

    fn native_output_maximum(
        &mut self,
        preparation: &Preparation,
        records: &[SourceRecord],
    ) -> Result<Capability<crate::catalog::NativeOutputMaximum>, ResolutionError> {
        let mut claims = Vec::with_capacity(records.len());
        for record in records {
            let mut claim = record.native_output_maximum.clone();
            if let Capability::Supported { value, .. } = &claim {
                if value.configuration_revision != preparation.configuration_revision {
                    return Err(ResolutionError::ConfigurationMismatch(
                        "native_output_maximum",
                    ));
                }
                // A hard native bound needs fresh source facts as well as fresh
                // adapter qualification. Ordinary stale-limit grace cannot apply.
                if !self.admit(&value.fact, false) {
                    claim = Capability::Unknown {
                        reason: if value.fact.observed_at_ms > self.now {
                            UnknownReason::NotYetValid
                        } else {
                            UnknownReason::Expired
                        },
                    };
                }
            }
            claims.push(claim);
        }
        self.capability("native_output_maximum", claims.into_iter())
    }
}

pub(crate) fn resolve(
    preparation: Preparation,
    records: &[SourceRecord],
    now: u64,
) -> Result<Snapshot, ResolutionError> {
    let mut resolver = Resolver {
        now,
        stale_for: preparation.allow_stale_for_ms,
        discarded: vec![],
        stale: vec![],
        until: None,
    };
    let context = resolver.limit(
        records
            .iter()
            .flat_map(|record| record.limits.context.clone()),
        None,
        false,
    );
    let input = resolver.limit(
        records
            .iter()
            .flat_map(|record| record.limits.input.clone()),
        context.known(),
        true,
    );
    // Output cannot be inferred from a context window. An explicit output fact is required.
    let output = resolver.limit(
        records
            .iter()
            .flat_map(|record| record.limits.output.clone()),
        context.known(),
        false,
    );
    let tools = resolver.capability("tools", records.iter().map(|record| record.tools.clone()))?;
    let streaming = resolver.capability(
        "streaming",
        records.iter().map(|record| record.streaming.clone()),
    )?;
    let native_compaction = resolver.capability(
        "native_compaction",
        records
            .iter()
            .map(|record| record.native_compaction.clone()),
    )?;
    let native_output_maximum = resolver.native_output_maximum(&preparation, records)?;
    let required = &preparation.requirements;
    for (field, needed, value) in [
        ("context", required.context, &context),
        ("input", required.input, &input),
        ("output", required.output, &output),
    ] {
        if needed && let Metadata::Unknown { reason } = value {
            return Err(ResolutionError::MissingLimit {
                field,
                reason: reason.clone(),
            });
        }
    }
    for (field, needed, supported) in [
        (
            "tools",
            required.tools,
            matches!(tools, Capability::Supported { .. }),
        ),
        (
            "streaming",
            required.streaming,
            matches!(streaming, Capability::Supported { .. }),
        ),
        (
            "native_compaction",
            required.native_compaction,
            matches!(native_compaction, Capability::Supported { .. }),
        ),
    ] {
        if needed && !supported {
            return Err(ResolutionError::MissingCapability(field));
        }
    }
    Ok(Snapshot {
        preparation,
        generation: String::new(),
        context,
        input,
        output,
        tools,
        streaming,
        native_compaction,
        native_output_maximum,
        discarded_limits: resolver.discarded,
        stale_evidence: resolver.stale,
        prepared_at_ms: now,
        valid_until_ms: resolver.until,
    })
}
