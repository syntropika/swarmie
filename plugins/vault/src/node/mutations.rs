use super::*;

impl VaultNode {
    /// Agent-originated replacement is value-only and must satisfy the *existing* scopes.
    /// Policy edits belong to the separate native management action with an explicit policy view.
    pub fn check_agent_mutation(
        &self,
        session: &VaultSession,
        execution: &ExecutionBinding,
        reference: SecretReference,
        expected: Option<Uuid>,
        metadata: &SecretMetadata,
    ) -> Result<()> {
        self.check(session)?;
        if !session.owner().same_owner(execution.shell()) {
            return Err(Error::Forbidden);
        }
        execution.eligible(reference, metadata)?;
        match (self.latest(reference), expected) {
            (Ok(record), Some(expected))
                if record.header.revision == expected
                    && record.header.kind == RecordKind::Value =>
            {
                execution.eligible(reference, &record.header.metadata)?;
                if record.header.metadata != *metadata {
                    return Err(Error::Forbidden);
                }
            }
            (Err(Error::NotFound), None) => {}
            (Err(error), _) => return Err(error),
            _ => return Err(Error::Stale),
        }
        Ok(())
    }
    /// Creates a private entry before the human's exact Save/Replace/Delete action.
    pub fn prepare_mutation(
        &self,
        session: &VaultSession,
        reference: SecretReference,
        expected: Option<Uuid>,
        metadata: SecretMetadata,
        value: Option<SecretValue>,
    ) -> Result<PreparedMutation> {
        self.prepare_borrowed(session, reference, expected, metadata, value.as_ref())
    }
    /// Human entry adapter can notify explicitly permitted native sinks without cloning a value.
    pub fn prepare_entry(
        &self,
        session: &VaultSession,
        reference: SecretReference,
        expected: Option<Uuid>,
        metadata: SecretMetadata,
        value: &SecretValue,
    ) -> Result<PreparedMutation> {
        self.prepare_borrowed(session, reference, expected, metadata, Some(value))
    }
    fn prepare_borrowed(
        &self,
        session: &VaultSession,
        reference: SecretReference,
        expected: Option<Uuid>,
        metadata: SecretMetadata,
        value: Option<&SecretValue>,
    ) -> Result<PreparedMutation> {
        self.check(session)?;
        metadata.validate()?;
        if reference.account != self.0.identity.account || reference.vault != self.0.identity.vault
        {
            return Err(Error::Forbidden);
        }
        let previous = match self.latest(reference) {
            Ok(record) => Some(record),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        if previous.as_ref().map(|record| record.header.revision) != expected {
            return Err(Error::Stale);
        }
        if previous.as_ref().is_some_and(|record| {
            record.header.metadata != metadata
                && record.header.metadata.policy_generation == metadata.policy_generation
        }) {
            return Err(Error::Invalid);
        }
        let intent = MutationIntent {
            reference,
            expected,
            metadata,
            kind: if value.is_some() {
                RecordKind::Value
            } else {
                RecordKind::Deleted
            },
            prepared_entry: Uuid::new_v4(),
        };
        // Seal before any approval or network wait. The prepared entry retains no plaintext or V.
        let candidate = self.check(session)?.seal(
            RecordHeader {
                version: 1,
                identity: self.0.identity,
                secret: reference.secret,
                revision: intent.prepared_entry,
                previous: expected,
                successor: next_successor(previous.as_ref())?,
                metadata: intent.metadata.clone(),
                kind: intent.kind,
            },
            value,
        )?;
        Ok(PreparedMutation {
            owner: session.owner().clone(),
            intent,
            candidate,
        })
    }

    pub async fn write(
        &self,
        session: &VaultSession,
        prepared: PreparedMutation,
        decision: HumanDecision,
    ) -> Result<(Uuid, crate::CommitOutcome)> {
        self.write_private(session, prepared, decision, None).await
    }
    /// Native human entry hooks run only after current-mode, policy/base and save admission.
    pub async fn write_entry(
        &self,
        session: &VaultSession,
        prepared: PreparedMutation,
        decision: HumanDecision,
        entries: &crate::PrivateEntries,
    ) -> Result<(Uuid, crate::CommitOutcome)> {
        self.write_private(session, prepared, decision, Some(entries))
            .await
    }
    async fn write_private(
        &self,
        session: &VaultSession,
        prepared: PreparedMutation,
        decision: HumanDecision,
        entries: Option<&crate::PrivateEntries>,
    ) -> Result<(Uuid, crate::CommitOutcome)> {
        self.refresh_current(session).await?;
        self.check(session)?;
        if !prepared.owner.same_owner(session.owner()) {
            return Err(Error::Forbidden);
        }
        let PreparedMutation {
            intent, candidate, ..
        } = prepared;
        if intent.reference.account != self.0.identity.account
            || intent.reference.vault != self.0.identity.vault
        {
            return Err(Error::Forbidden);
        }
        match self.cloud().mode(self.0.identity).await? {
            VaultMode::Current => {}
            VaultMode::ArchivedReadable if intent.kind == RecordKind::Deleted => {}
            VaultMode::ArchivedReadable => return Err(Error::VaultNotCurrent),
            VaultMode::Exposed if intent.kind == RecordKind::Value => {
                return Err(Error::RootExposed);
            }
            VaultMode::Exposed => {}
        }
        self.check(session)?;
        if entries.is_some() && intent.kind != RecordKind::Value {
            return Err(Error::Invalid);
        }
        intent.metadata.validate()?;
        decision.consume_owner(session.owner(), &("vault-mutation/v1", &intent))?;
        let previous = match self.latest(intent.reference) {
            Ok(r) => Some(r),
            Err(Error::NotFound) => None,
            Err(e) => return Err(e),
        };
        if previous.as_ref().map(|r| r.header.revision) != intent.expected {
            return Err(Error::Stale);
        }
        if previous.as_ref().is_some_and(|r| {
            r.header.metadata != intent.metadata
                && r.header.metadata.policy_generation == intent.metadata.policy_generation
        }) {
            return Err(Error::Invalid);
        }
        let command = crate::RecordCommand::prepare(&self.0.keys, self.0.device, candidate)?;
        let mutation = command.operation;
        let mut snapshot = self.0.state.borrow().snapshot.clone();
        if snapshot.pending.len() + snapshot.rejected.len() >= 64 {
            return Err(Error::Exhausted);
        }
        snapshot.pending.push(command.clone());
        self.persist(snapshot)?;
        if let Some(entries) = entries {
            let key = self.check(session)?;
            let value = key.open(command.record())?;
            if let Err(error) = entries.accept(&intent, &value) {
                self.settle_mutation(command, crate::CommitOutcome::NotDispatched)?;
                return Err(error);
            }
            drop(value);
            drop(key);
            if let Err(error) = self.check(session) {
                self.settle_mutation(command, crate::CommitOutcome::NotDispatched)?;
                return Err(error);
            }
        }
        let outcome = self.cloud().commit(command.clone()).await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(_) => crate::CommitOutcome::Unknown,
        };
        self.settle_mutation(command, outcome)?;
        if outcome == crate::CommitOutcome::Accepted {
            let _ = self.synchronize(session).await;
        }
        Ok((mutation, outcome))
    }

    /// Recognition never changes the command nonce or reissues an external producer.
    pub async fn recognize(
        &self,
        session: &VaultSession,
        mutation: Uuid,
    ) -> Result<crate::CommitOutcome> {
        self.check(session)?;
        if !self
            .0
            .state
            .borrow()
            .snapshot
            .pending
            .iter()
            .any(|c| c.operation == mutation)
        {
            return Err(Error::NotFound);
        }
        let command = self
            .0
            .state
            .borrow()
            .snapshot
            .pending
            .iter()
            .find(|command| command.operation == mutation)
            .cloned()
            .ok_or(Error::NotFound)?;
        let outcome = self.cloud().recognize(command.clone()).await?;
        self.settle_mutation(command, outcome)?;
        if outcome == crate::CommitOutcome::Accepted {
            let _ = self.synchronize(session).await;
        }
        Ok(outcome)
    }

    fn settle_mutation(
        &self,
        command: crate::RecordCommand,
        outcome: crate::CommitOutcome,
    ) -> Result<()> {
        use crate::CommitOutcome::*;
        if outcome == Unknown {
            return Ok(());
        }
        let mut snapshot = self.0.state.borrow().snapshot.clone();
        if outcome == Accepted {
            if !snapshot
                .records
                .iter()
                .any(|record| record.header.revision == command.operation)
                && !snapshot.accepted.contains(&command.operation)
            {
                snapshot.accepted.push(command.operation);
            }
        } else {
            snapshot
                .pending
                .retain(|pending| pending.operation != command.operation);
            snapshot
                .accepted
                .retain(|operation| *operation != command.operation);
            if !snapshot
                .rejected
                .iter()
                .any(|draft| draft.command.operation == command.operation)
            {
                snapshot
                    .rejected
                    .push(crate::RejectedMutation { command, outcome });
            }
        }
        self.persist(snapshot)
    }
    /// Exact signed command is fenced before an uncertain mutation can be abandoned.
    /// Accepted means it already took effect; it is never rolled back or silently rewritten.
    pub async fn abandon_mutation(
        &self,
        session: &VaultSession,
        mutation: Uuid,
        decision: HumanDecision,
    ) -> Result<crate::CommitOutcome> {
        self.check(session)?;
        let command = self
            .0
            .state
            .borrow()
            .snapshot
            .pending
            .iter()
            .find(|command| command.operation == mutation)
            .cloned()
            .ok_or(Error::NotFound)?;
        decision.consume_owner(
            session.owner(),
            &(
                "vault-abandon-mutation/v1",
                self.identity(),
                mutation,
                crate::digest(&command.signing_bytes()?),
            ),
        )?;
        let outcome = self.cloud().fence(command.clone()).await?;
        self.settle_mutation(command, outcome)?;
        if outcome == crate::CommitOutcome::Accepted {
            let _ = self.synchronize(session).await;
        }
        Ok(outcome)
    }
    pub fn rejected_mutations(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
    ) -> Result<Vec<(MutationIntent, crate::CommitOutcome)>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        Ok(self
            .0
            .state
            .borrow()
            .snapshot
            .rejected
            .iter()
            .map(|draft| {
                let header = &draft.command.record().header;
                (
                    MutationIntent {
                        reference: SecretReference {
                            account: header.identity.account,
                            vault: header.identity.vault,
                            secret: header.secret,
                        },
                        expected: header.previous,
                        metadata: header.metadata.clone(),
                        kind: header.kind,
                        prepared_entry: header.revision,
                    },
                    draft.outcome,
                )
            })
            .collect())
    }
    /// Value stays private while the native UI prepares a separately approved replacement/rebase.
    pub fn prepare_rejected(
        &self,
        session: &VaultSession,
        mutation: Uuid,
        metadata: SecretMetadata,
        expected: Option<Uuid>,
    ) -> Result<PreparedMutation> {
        let key = self.check(session)?;
        let record = self
            .0
            .state
            .borrow()
            .snapshot
            .rejected
            .iter()
            .find(|draft| draft.command.operation == mutation)
            .map(|draft| draft.command.record().clone())
            .ok_or(Error::NotFound)?;
        let value = if record.header.kind == RecordKind::Value {
            Some(key.open(&record)?)
        } else {
            None
        };
        self.prepare_mutation(
            session,
            SecretReference {
                account: self.identity().account,
                vault: self.identity().vault,
                secret: record.header.secret,
            },
            expected,
            metadata,
            value,
        )
    }
    pub fn discard_rejected(
        &self,
        session: &VaultSession,
        mutation: Uuid,
        decision: HumanDecision,
    ) -> Result<()> {
        self.check(session)?;
        decision.consume_owner(
            session.owner(),
            &("vault-discard-rejected/v1", self.identity(), mutation),
        )?;
        let mut snapshot = self.0.state.borrow().snapshot.clone();
        if !snapshot
            .rejected
            .iter()
            .any(|draft| draft.command.operation == mutation)
        {
            return Err(Error::NotFound);
        }
        snapshot
            .rejected
            .retain(|draft| draft.command.operation != mutation);
        self.persist(snapshot)
    }
}
