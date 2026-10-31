use futures::{FutureExt, executor::block_on, future::LocalBoxFuture};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use swarmie_vault::*;
use uuid::Uuid;

#[derive(Default)]
struct Store(RefCell<BTreeMap<VaultIdentity, VaultSnapshot>>, Cell<bool>);
impl VaultStorage for Store {
    fn load(&self, identity: VaultIdentity) -> Result<VaultSnapshot> {
        Ok(self.0.borrow().get(&identity).cloned().unwrap_or_default())
    }
    fn compare_exchange(
        &self,
        identity: VaultIdentity,
        expected: u64,
        next: &VaultSnapshot,
    ) -> Result<()> {
        if self.1.get() {
            return Err(Error::Unavailable);
        }
        let mut all = self.0.borrow_mut();
        if all.get(&identity).map_or(0, |s| s.revision) != expected {
            return Err(Error::Stale);
        }
        all.insert(identity, next.clone());
        Ok(())
    }
}

struct Cloud {
    identity: VaultIdentity,
    enrollment: SignedEnrollment,
    envelope: DeviceEnvelope,
    online: Cell<bool>,
    ambiguous: Cell<bool>,
    undispatched: Cell<bool>,
    fences: RefCell<BTreeSet<Uuid>>,
    check_error: Cell<Option<Error>>,
    changes_error: Cell<Option<Error>>,
    mode: Cell<VaultMode>,
    records: RefCell<Vec<EncryptedRecord>>,
    admitted: RefCell<Option<ClientAdmission>>,
}
impl VaultCloud for Cloud {
    fn current_authority(&self, _: VaultIdentity) -> LocalBoxFuture<'_, Result<AuthorityCheck>> {
        async move {
            if let Some(error) = self.check_error.get() {
                return Err(error);
            }
            if !self.online.get() {
                return Err(Error::Offline);
            }
            let admission = self.admitted.borrow().clone().ok_or(Error::Forbidden)?;
            Ok(AuthorityCheck {
                identity: self.identity,
                sequence: admission.sequence,
                actor_family: admission.family,
                current: (self.mode.get() != VaultMode::ArchivedReadable)
                    .then_some(self.identity.vault),
                exposed: self.mode.get() == VaultMode::Exposed,
                active_families: BTreeSet::from([admission.family]),
                devices: vec![DeviceAuthority {
                    device: admission.intent.node,
                    fingerprint: admission.intent.client.clone(),
                    roles: BTreeSet::from([Role::Decrypt, Role::Manage, Role::EnrollDevices]),
                    family: Some(admission.family),
                    removed: false,
                }],
                generations: vec![AuthorizationGeneration {
                    client: admission.intent.client.clone(),
                    node: admission.intent.node,
                    generation: admission.generation,
                }],
            })
        }
        .boxed_local()
    }

    fn release<'a>(
        &'a self,
        request: ReleaseRequest,
    ) -> LocalBoxFuture<'a, Result<ReleasedEnvelope>> {
        async move {
            if !self.online.get() {
                return Err(Error::Offline);
            }
            if request.identity != self.identity
                || request.signing_key != self.enrollment.statement.signing_key
            {
                return Err(Error::Forbidden);
            }
            verify_signature(
                &request.signing_key,
                &canonical(&(
                    "vault-release/v1",
                    request.identity,
                    request.device,
                    &request.client,
                    request.family,
                    request.shell_installation,
                    request.shell_generation,
                    &request.signing_key,
                    request.nonce,
                ))?,
                &request.signature,
            )?;
            let admission = ClientAdmission {
                ticket: Uuid::new_v4(),
                family: request.family,
                generation: 1,
                sequence: 0,
                intent: TicketIntent {
                    identity: self.identity,
                    node: request.device,
                    client: request.client.clone(),
                    shell: request.shell_generation,
                    resume: vec![6; 32],
                    exporter: vec![7; 32],
                    challenge: Uuid::new_v4(),
                    nonce: request.nonce,
                },
            };
            *self.admitted.borrow_mut() = Some(admission.clone());
            Ok(ReleasedEnvelope {
                enrollment: self.enrollment.clone(),
                envelope: self.envelope.clone(),
                admission,
            })
        }
        .boxed_local()
    }
    fn mode<'a>(&'a self, _: VaultIdentity) -> LocalBoxFuture<'a, Result<VaultMode>> {
        async move {
            if self.online.get() {
                Ok(self.mode.get())
            } else {
                Err(Error::Offline)
            }
        }
        .boxed_local()
    }
    fn commit<'a>(&'a self, command: RecordCommand) -> LocalBoxFuture<'a, Result<CommitOutcome>> {
        let record = command.record().clone();
        async move {
            if self.fences.borrow().contains(&command.operation) {
                return Ok(CommitOutcome::NotDispatched);
            }
            if self.undispatched.get() {
                return Ok(CommitOutcome::Unknown);
            }
            if !self.online.get() {
                return Ok(CommitOutcome::NotDispatched);
            }
            verify_signature(
                &self.enrollment.statement.signing_key,
                &command.signing_bytes()?,
                &command.signature,
            )?;
            let mut all = self.records.borrow_mut();
            if all
                .iter()
                .any(|r| r.header.revision == record.header.revision)
            {
                return Ok(CommitOutcome::Accepted);
            }
            let prior = all
                .iter()
                .rev()
                .find(|r| r.header.secret == record.header.secret);
            if prior.map(|r| r.header.revision) != record.header.previous {
                return Ok(CommitOutcome::RejectedStale);
            }
            if record.header.kind == RecordKind::Value {
                let mut latest = BTreeMap::new();
                for r in all.iter() {
                    latest.insert(r.header.secret, r);
                }
                if latest.values().any(|r| {
                    r.header.secret != record.header.secret
                        && r.header.kind == RecordKind::Value
                        && r.header.metadata.name == record.header.metadata.name
                }) {
                    return Ok(CommitOutcome::RejectedNameConflict);
                }
            }
            all.push(record);
            Ok(if self.ambiguous.get() {
                CommitOutcome::Unknown
            } else {
                CommitOutcome::Accepted
            })
        }
        .boxed_local()
    }
    fn recognize<'a>(
        &'a self,
        command: RecordCommand,
    ) -> LocalBoxFuture<'a, Result<CommitOutcome>> {
        let mutation = command.operation;
        async move {
            Ok(
                if self
                    .records
                    .borrow()
                    .iter()
                    .any(|r| r.header.revision == mutation)
                {
                    CommitOutcome::Accepted
                } else {
                    if self.fences.borrow().contains(&mutation) {
                        CommitOutcome::NotDispatched
                    } else {
                        CommitOutcome::Unknown
                    }
                },
            )
        }
        .boxed_local()
    }
    fn fence(&self, command: RecordCommand) -> LocalBoxFuture<'_, Result<CommitOutcome>> {
        async move {
            if self
                .records
                .borrow()
                .iter()
                .any(|record| record.header.revision == command.operation)
            {
                return Ok(CommitOutcome::Accepted);
            }
            self.fences.borrow_mut().insert(command.operation);
            Ok(CommitOutcome::NotDispatched)
        }
        .boxed_local()
    }
    fn changes<'a>(
        &'a self,
        _: VaultIdentity,
        after: u64,
    ) -> LocalBoxFuture<'a, Result<CloudRecordPage>> {
        async move {
            if let Some(error) = self.changes_error.get() {
                return Err(error);
            }
            let records: Vec<_> = self
                .records
                .borrow()
                .iter()
                .skip(after as usize)
                .take(128)
                .cloned()
                .collect();
            let next = after + records.len() as u64;
            Ok(CloudRecordPage {
                identity: self.identity,
                after,
                records,
                next,
                more: next < self.records.borrow().len() as u64,
                current: true,
                exposed: false,
            })
        }
        .boxed_local()
    }
}

struct Fixture {
    node: VaultNode,
    store: Rc<Store>,
    cloud: Rc<Cloud>,
    authority: Authority,
    human: HumanSession,
    session: VaultSession,
    binding: ExecutionBinding,
    plugin: PluginGrant,
    keys: Rc<DeviceKeys>,
    pins: TrustPins,
    device: Uuid,
}
fn fixture(approval: Approval) -> (Fixture, MutationIntent) {
    let identity = VaultIdentity {
        account: Uuid::new_v4(),
        vault: Uuid::new_v4(),
        epoch: Uuid::new_v4(),
    };
    let key = VaultKey::generate();
    let keys = Rc::new(DeviceKeys::generate());
    let device = Uuid::new_v4();
    let envelope = DeviceKeys::envelope(
        &key,
        &keys.encryption_public().unwrap(),
        identity,
        device,
        &keys.signing_public(),
    )
    .unwrap();
    let commitment = key.commitment(identity).unwrap();
    let envelope_digest = digest(&canonical(&envelope).unwrap());
    let public = keys.signing_public();
    let statement = EnrollmentStatement {
        version: 1,
        identity,
        recipient: device,
        signing_key: public.clone(),
        encryption_key: Some(keys.encryption_public().unwrap()),
        roles: BTreeSet::from([Role::Decrypt, Role::Manage, Role::EnrollDevices]),
        approver: public.clone(),
        nonce: Uuid::new_v4(),
        envelope_digest: Some(envelope_digest.clone()),
        commitment: commitment.clone(),
    };
    let enrollment = keys.enroll(statement).unwrap();
    let pins = TrustPins {
        identity,
        root: public.clone(),
        chain: vec![],
        approver: public,
        statement_digest: digest(&canonical(&enrollment).unwrap()),
        envelope_digest,
        commitment,
    };
    let cloud = Rc::new(Cloud {
        identity,
        enrollment,
        envelope,
        online: Cell::new(true),
        ambiguous: Cell::new(false),
        undispatched: Cell::new(false),
        fences: RefCell::new(BTreeSet::new()),
        check_error: Cell::new(None),
        changes_error: Cell::new(None),
        mode: Cell::new(VaultMode::Current),
        records: RefCell::new(vec![]),
        admitted: RefCell::new(None),
    });
    let authority = Authority::new();
    let store = Rc::new(Store::default());
    let node = VaultNode::open(
        identity,
        device,
        keys.clone(),
        pins.clone(),
        store.clone(),
        cloud.clone(),
        authority.clone(),
    )
    .unwrap();
    let human = authority.authenticate_shell(ShellIdentity {
        cloud: Some(CloudOwnerIdentity {
            client: digest(&cloud.enrollment.statement.signing_key),
            family: Uuid::new_v4(),
        }),
        account: Some(identity.account),
        installation: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    });
    let binding = authority
        .execution(
            &human,
            ExecutionIdentity {
                account: Some(identity.account),
                execution: Uuid::new_v4(),
                generation: Uuid::new_v4(),
                project: Some(Uuid::new_v4()),
                agent: Some(Uuid::new_v4()),
                conversation: Some(Uuid::new_v4()),
                main_agent: true,
            },
        )
        .unwrap();
    let session = block_on(node.unlock(&human)).unwrap();
    let plugin = PluginGrant::bundled(CorePlugin::Vault, "executor".into());
    let metadata = SecretMetadata {
        name: "deployment".into(),
        purpose: "synthetic fixture".into(),
        projects: Scope::Unrestricted,
        agents: Scope::Unrestricted,
        approval,
        policy_generation: Uuid::new_v4(),
        target_account: None,
    };
    let intent = MutationIntent {
        reference: SecretReference {
            account: identity.account,
            vault: identity.vault,
            secret: Uuid::new_v4(),
        },
        expected: None,
        metadata,
        kind: RecordKind::Value,
        prepared_entry: Uuid::new_v4(),
    };
    (
        Fixture {
            node,
            store,
            cloud,
            authority,
            human,
            session,
            binding,
            plugin,
            keys,
            pins,
            device,
        },
        intent,
    )
}
fn write(f: &Fixture, intent: MutationIntent, value: &str) -> (Uuid, CommitOutcome) {
    let prepared = f
        .node
        .prepare_mutation(
            &f.session,
            intent.reference,
            intent.expected,
            intent.metadata,
            Some(SecretValue::new(value.as_bytes().to_vec()).unwrap()),
        )
        .unwrap();
    let decision = f
        .human
        .decide(&("vault-mutation/v1", prepared.intent()))
        .unwrap();
    block_on(f.node.write(&f.session, prepared, decision)).unwrap()
}
fn admit_capture(
    f: &Fixture,
    handle: &CaptureHandle,
    decision: HumanDecision,
    settlement: std::time::Duration,
) -> Result<SecretCaptureSink> {
    block_on(f.node.admit_capture(handle, decision, settlement))
}

fn use_intent(f: &Fixture, intent: &MutationIntent) -> UseIntent {
    UseIntent {
        reference: intent.reference,
        metadata: intent.metadata.clone(),
        operation: Uuid::new_v4(),
        attempt: Uuid::new_v4(),
        provider: "executor".into(),
        destination: "https://example.invalid/deploy".into(),
        execution: f.binding.identity().clone(),
        policy_generation: intent.metadata.policy_generation,
        permission_generation: f.plugin.generation(),
    }
}

#[test]
fn encrypted_records_authenticate_metadata_and_never_store_canary_plaintext() {
    let (f, intent) = fixture(Approval::None);
    assert_eq!(
        write(&f, intent.clone(), "synthetic-private-canary").1,
        CommitOutcome::Accepted
    );
    let bytes = serde_json::to_vec(&*f.cloud.records.borrow()).unwrap();
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains("synthetic-private-canary")
    );
    let lease = f
        .node
        .lease(&f.session, &f.binding, &f.plugin, &use_intent(&f, &intent))
        .unwrap();
    assert_eq!(
        lease.mask("echo synthetic-private-canary"),
        format!("echo {}", intent.reference)
    );
    assert_eq!(
        lease.deliver(|v| Ok(v.to_vec())).unwrap(),
        b"synthetic-private-canary"
    );
}

#[test]
fn operation_approval_is_one_attempt_and_latest_replacement_is_used() {
    let (f, mut intent) = fixture(Approval::Operation);
    let (first, _) = write(&f, intent.clone(), "old-canary");
    let use_it = use_intent(&f, &intent);
    assert!(matches!(
        f.node.lease(&f.session, &f.binding, &f.plugin, &use_it),
        Err(Error::ApprovalRequired)
    ));
    let decision = f.human.decide(&("vault-use/v1", &use_it, false)).unwrap();
    f.node
        .approve_use(
            &f.session,
            &f.binding,
            &f.plugin,
            use_it.clone(),
            false,
            decision,
        )
        .unwrap();
    intent.expected = Some(first);
    intent.prepared_entry = Uuid::new_v4();
    write(&f, intent, "new-canary");
    assert_eq!(
        f.node
            .lease(&f.session, &f.binding, &f.plugin, &use_it)
            .unwrap()
            .deliver(|v| Ok(v.to_vec()))
            .unwrap(),
        b"new-canary"
    );
    assert!(matches!(
        f.node.lease(&f.session, &f.binding, &f.plugin, &use_it),
        Err(Error::ApprovalRequired)
    ));
}

#[test]
fn child_cannot_inherit_main_conversation_grant_and_scopes_intersect() {
    let (f, mut intent) = fixture(Approval::Conversation);
    intent.metadata.projects = Scope::Only(BTreeSet::from([f.binding.identity().project.unwrap()]));
    intent.metadata.agents = Scope::Only(BTreeSet::from([f.binding.identity().agent.unwrap()]));
    write(&f, intent.clone(), "canary");
    let use_it = use_intent(&f, &intent);
    f.node
        .approve_use(
            &f.session,
            &f.binding,
            &f.plugin,
            use_it.clone(),
            true,
            f.human.decide(&("vault-use/v1", &use_it, true)).unwrap(),
        )
        .unwrap();
    assert!(
        f.node
            .lease(&f.session, &f.binding, &f.plugin, &use_it)
            .is_ok()
    );
    let mut child = f.binding.identity().clone();
    child.execution = Uuid::new_v4();
    child.main_agent = false;
    let child = f.authority.execution(&f.human, child).unwrap();
    let mut child_intent = use_it.clone();
    child_intent.execution = child.identity().clone();
    assert!(matches!(
        f.node.lease(&f.session, &child, &f.plugin, &child_intent),
        Err(Error::ApprovalRequired)
    ));
    let mut foreign = f.binding.identity().clone();
    foreign.project = Some(Uuid::new_v4());
    let foreign = f.authority.execution(&f.human, foreign).unwrap();
    assert!(
        f.node
            .discover(&f.session, &foreign, &f.plugin)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn offline_use_survives_but_fresh_unlock_and_mutation_fail_and_logout_clears_keys() {
    let (f, intent) = fixture(Approval::None);
    write(&f, intent.clone(), "offline-canary");
    f.cloud.online.set(false);
    assert!(
        f.node
            .lease(&f.session, &f.binding, &f.plugin, &use_intent(&f, &intent))
            .is_ok()
    );
    assert!(matches!(
        block_on(f.node.unlock(&f.human)),
        Err(Error::Offline)
    ));
    f.human.close();
    assert!(f.node.is_locked());
    assert!(matches!(
        f.node
            .lease(&f.session, &f.binding, &f.plugin, &use_intent(&f, &intent)),
        Err(Error::Revoked)
    ));
}

#[test]
fn output_mask_handles_every_chunk_split_and_overlapping_occurrences() {
    let (f, intent) = fixture(Approval::None);
    write(&f, intent.clone(), "private-canary");
    let raw = b"prefix private-canaryprivate-canary suffix";
    for split in 0..=raw.len() {
        let lease = f
            .node
            .lease(&f.session, &f.binding, &f.plugin, &use_intent(&f, &intent))
            .unwrap();
        let mut mask = OutputMask::new(vec![lease]).unwrap();
        let mut result = mask.push(&raw[..split]);
        result.extend(mask.push(&raw[split..]));
        result.extend(mask.finish());
        assert_eq!(
            String::from_utf8(result).unwrap(),
            format!("prefix {0}{0} suffix", intent.reference)
        );
    }
}

#[test]
fn blind_candidate_is_encrypted_before_receipt_and_store_does_not_repeat_producer() {
    let (f, mutation) = fixture(Approval::Operation);
    let intent = CaptureIntent {
        identity: f.node.identity(),
        originating_operation: Uuid::new_v4(),
        execution: f.binding.identity().clone(),
        provider: "executor".into(),
        producer_generation: f.plugin.generation(),
        destination: "native:generate".into(),
        profile: "native-random/v1".into(),
        secret: mutation.reference.secret,
        expected: None,
        metadata: mutation.metadata.clone(),
        attempt: Uuid::new_v4(),
        settlement_ms: 30_000,
    };
    let handle =
        block_on(
            f.node
                .prepare_capture(&f.session, &f.binding, &f.plugin, intent.clone()),
        )
        .unwrap();
    let repeated = block_on(
        f.node
            .prepare_capture(&f.session, &f.binding, &f.plugin, intent),
    )
    .unwrap();
    let capture = handle.record().unwrap();
    assert_eq!(capture.id, repeated.record().unwrap().id);
    let decision = f
        .human
        .decide(&("vault-blind-save/v1", capture.id, &capture.intent))
        .unwrap();
    let sink = admit_capture(&f, &handle, decision, std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(handle.record().unwrap().state, CaptureState::Dispatched);
    assert!(
        f.node
            .discover(&f.session, &f.binding, &f.plugin)
            .unwrap()
            .is_empty()
    );
    let receipt = sink
        .push(SecretValue::new(b"new-generated-canary".to_vec()).unwrap())
        .unwrap();
    assert_eq!(receipt.state, CaptureState::CandidateHeld);
    let bytes = serde_json::to_vec(&handle.record().unwrap()).unwrap();
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains("new-generated-canary")
    );
    let saved = block_on(f.node.store_capture(&handle)).unwrap();
    assert_eq!(saved.state, CaptureState::Saved);
    assert_eq!(saved.reference, Some(mutation.reference));
    assert_eq!(block_on(f.node.store_capture(&handle)).unwrap(), saved);
    assert_eq!(f.cloud.records.borrow().len(), 1);
}

#[test]
fn late_candidate_can_be_sealed_after_session_end_without_reopening_general_vault() {
    let (f, mutation) = fixture(Approval::None);
    let intent = CaptureIntent {
        identity: f.node.identity(),
        originating_operation: Uuid::new_v4(),
        execution: f.binding.identity().clone(),
        provider: "executor".into(),
        producer_generation: f.plugin.generation(),
        destination: "native:generate".into(),
        profile: "native-random/v1".into(),
        secret: mutation.reference.secret,
        expected: None,
        metadata: mutation.metadata,
        attempt: Uuid::new_v4(),
        settlement_ms: 30_000,
    };
    let handle = block_on(
        f.node
            .prepare_capture(&f.session, &f.binding, &f.plugin, intent),
    )
    .unwrap();
    let c = handle.record().unwrap();
    let decision = f
        .human
        .decide(&("vault-blind-save/v1", c.id, &c.intent))
        .unwrap();
    let sink = admit_capture(&f, &handle, decision, std::time::Duration::from_secs(30)).unwrap();
    f.human.close();
    assert!(f.node.is_locked());
    assert_eq!(
        sink.push(SecretValue::new(b"late-canary".to_vec()).unwrap())
            .unwrap()
            .state,
        CaptureState::CandidateHeld
    );
    assert!(f.node.is_locked());
    assert!(matches!(
        block_on(f.node.store_capture(&handle)),
        Err(Error::Revoked)
    ));
}

#[test]
fn hosted_cursor_does_not_skip_an_earlier_unobserved_record() {
    let (f, first) = fixture(Approval::None);
    f.cloud.ambiguous.set(true);
    assert_eq!(
        write(&f, first.clone(), "first-canary").1,
        CommitOutcome::Unknown
    );
    assert_eq!(f.store.load(f.node.identity()).unwrap().cloud_cursor, 0);
    let mut second = first.clone();
    second.reference.secret = Uuid::new_v4();
    second.metadata.name = "second".into();
    f.cloud.ambiguous.set(false);
    assert_eq!(
        write(&f, second, "second-canary").1,
        CommitOutcome::Accepted
    );
    let snapshot = f.store.load(f.node.identity()).unwrap();
    assert_eq!(snapshot.cloud_cursor, 2);
    assert_eq!(snapshot.records.len(), 2);
    assert!(snapshot.pending.is_empty());
    assert_eq!(
        f.node
            .discover(&f.session, &f.binding, &f.plugin)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn failed_candidate_journal_keeps_encrypted_candidate_for_persistence_retry() {
    let (f, mutation) = fixture(Approval::None);
    let intent = CaptureIntent {
        identity: f.node.identity(),
        originating_operation: Uuid::new_v4(),
        execution: f.binding.identity().clone(),
        provider: "executor".into(),
        producer_generation: f.plugin.generation(),
        destination: "native:generate".into(),
        profile: "native-random/v1".into(),
        secret: mutation.reference.secret,
        expected: None,
        metadata: mutation.metadata,
        attempt: Uuid::new_v4(),
        settlement_ms: 30_000,
    };
    let handle = block_on(
        f.node
            .prepare_capture(&f.session, &f.binding, &f.plugin, intent),
    )
    .unwrap();
    let c = handle.record().unwrap();
    let decision = f
        .human
        .decide(&("vault-blind-save/v1", c.id, &c.intent))
        .unwrap();
    let sink = admit_capture(&f, &handle, decision, std::time::Duration::from_secs(30)).unwrap();
    f.store.1.set(true);
    let held = sink
        .push(SecretValue::new(b"retained-canary".to_vec()).unwrap())
        .unwrap_err();
    assert_eq!(held.error, Error::Unavailable);
    f.store.1.set(false);
    assert_eq!(held.retry().unwrap().state, CaptureState::CandidateHeld);
    assert_eq!(
        block_on(f.node.store_capture(&handle)).unwrap().state,
        CaptureState::Saved
    );
    assert_eq!(f.cloud.records.borrow().len(), 1);
}

#[test]
fn a_foreign_native_authority_cannot_unlock_a_node_for_the_same_account() {
    let (f, _) = fixture(Approval::None);
    let foreign = Authority::new();
    let forged = foreign.authenticate_shell(f.human.identity().clone());
    assert!(matches!(
        block_on(f.node.unlock(&forged)),
        Err(Error::Revoked)
    ));
}

#[cfg(all(feature = "executors", unix))]
mod native_executors {
    use super::*;
    use swarmie_vault::executors::*;
    struct Decisions {
        human: HumanSession,
        count: Cell<usize>,
    }
    impl UseDecisions for Decisions {
        fn request<'a>(
            &'a self,
            _execution: &'a ExecutionBinding,
            intent: &'a UseIntent,
        ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
            async move {
                self.count.set(self.count.get() + 1);
                Ok((self.human.decide(&("vault-use/v1", intent, false))?, false))
            }
            .boxed_local()
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn bash_receives_data_in_environment_and_masks_both_private_output_streams() {
        let (f, mutation) = fixture(Approval::Operation);
        let value = "synthetic-$(touch injected-marker);'\"$";
        write(&f, mutation.clone(), value);
        let decisions = Decisions {
            human: f.human.clone(),
            count: Cell::new(0),
        };
        let attempt = ExecutionAttempt {
            node: &f.node,
            session: &f.session,
            execution: &f.binding,
            plugin: &f.plugin,
            decisions: &decisions,
            operation: Uuid::new_v4(),
            attempt: Uuid::new_v4(),
        };
        let dir = tempfile::tempdir().unwrap();
        let request = BashRequest {
            source:
                "printf '%s' \"$SWARMIE_SECRET_KEY\"; printf '%s' \"$SWARMIE_SECRET_SECOND\" >&2"
                    .into(),
            cwd: dir.path().to_path_buf(),
            environment: BTreeMap::from([
                ("SWARMIE_SECRET_KEY".into(), mutation.reference),
                ("SWARMIE_SECRET_SECOND".into(), mutation.reference),
            ]),
            timeout_ms: 2000,
        };
        let receipt = BashExecutor::new("/bin/bash".into())
            .execute(&attempt, request)
            .await
            .unwrap();
        assert_eq!(receipt.exit, Some(0));
        assert_eq!(receipt.stdout, mutation.reference.to_string());
        assert_eq!(receipt.stderr, mutation.reference.to_string());
        assert_eq!(decisions.count.get(), 1);
        assert!(!dir.path().join("injected-marker").exists());
        assert!(!serde_json::to_string(&receipt).unwrap().contains(value));
    }
}

#[cfg(feature = "executors")]
#[path = "../../../tests/fixtures/https-server.rs"]
mod https;

#[cfg(feature = "executors")]
mod native_https {
    use super::*;
    use swarmie_vault::executors::*;
    struct NoDecisions;
    impl UseDecisions for NoDecisions {
        fn request<'a>(
            &'a self,
            _execution: &'a ExecutionBinding,
            _: &'a UseIntent,
        ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
            async { Err(Error::Forbidden) }.boxed_local()
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn real_https_injects_headers_and_json_fields_and_masks_decoded_reflections() {
        let (f, mutation) = fixture(Approval::None);
        write(&f, mutation.clone(), "synthetic-quote-\"-canary");
        let (url, server) = https::serve(1, |request| {
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/credential");
            assert_eq!(
                request.headers["authorization"],
                "Bearer synthetic-quote-\"-canary"
            );
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["credential"], "synthetic-quote-\"-canary");
            https::Reply {
                status: 200,
                headers: BTreeMap::new(),
                body: serde_json::to_vec(&body).unwrap(),
            }
        })
        .await;
        let executor = HttpExecutor::new(Some(https::certificate())).unwrap();
        let attempt = ExecutionAttempt {
            node: &f.node,
            session: &f.session,
            execution: &f.binding,
            plugin: &f.plugin,
            decisions: &NoDecisions,
            operation: Uuid::new_v4(),
            attempt: Uuid::new_v4(),
        };
        let receipt = executor
            .execute(
                &attempt,
                HttpRequest {
                    url: format!("{url}credential"),
                    method: "POST".into(),
                    body: Some(serde_json::json!({"credential":mutation.reference.to_string()})),
                    credentials: BTreeMap::from([(
                        "Authorization".into(),
                        HeaderCredential {
                            reference: mutation.reference,
                            prefix: "Bearer ".into(),
                        },
                    )]),
                    fields: vec![JsonCredential {
                        pointer: "/credential".into(),
                        reference: mutation.reference,
                    }],
                    timeout_ms: 2000,
                },
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&receipt.body).unwrap();
        assert_eq!(body["credential"], mutation.reference.to_string());
        assert_eq!(receipt.status, 200);
        server.await.unwrap();
    }
}

#[cfg(all(feature = "executors", unix))]
#[tokio::test(flavor = "current_thread")]
async fn actual_blind_bash_job_survives_its_view_and_owner_close_without_residual_output() {
    use executors::*;
    struct NoInputs;
    impl UseDecisions for NoInputs {
        fn request<'a>(
            &'a self,
            _execution: &'a ExecutionBinding,
            _: &'a UseIntent,
        ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
            async { Err(Error::Forbidden) }.boxed_local()
        }
    }
    tokio::task::LocalSet::new().run_until(async {
        let (f, mutation) = fixture(Approval::None);
        let directory = tempfile::tempdir().unwrap();
        let source = CaptureSource::BashJson { request:BashRequest {
            source: "printf started > capture-started; sleep 0.05; token=produced-$BASHPID; printf '{\"token\":\"%s\",\"duplicate\":\"%s\",\"encoded\":\"' \"$token\" \"$token\"; printf %s \"$token\" | base64 | tr -d '\\n'; printf '\"}'; printf %s \"$token\" >&2".into(),
            cwd:directory.path().into(), environment:BTreeMap::new(), timeout_ms:5000,
        }, pointer:"/token".into() };
        let intent = CaptureIntent { identity:f.node.identity(), originating_operation:Uuid::new_v4(), execution:f.binding.identity().clone(),
            provider:f.plugin.provider().into(), producer_generation:f.plugin.generation(), destination:source.destination().unwrap(), profile:source.profile().unwrap(),
            secret:mutation.reference.secret, expected:None, metadata:mutation.metadata, attempt:Uuid::new_v4(), settlement_ms:source.timeout_ms() };
        let handle = Rc::new(f.node.prepare_capture(&f.session,&f.binding,&f.plugin,intent).await.unwrap());
        let record=handle.record().unwrap();
        let decision=f.human.decide(&("vault-blind-save/v1",record.id,&record.intent)).unwrap();
        let producers=Rc::new(CaptureProducers { node:f.node.clone(), session:f.session.clone(), execution:f.binding.clone(), plugin:f.plugin.clone(),
            decisions:Rc::new(NoInputs), bash:Rc::new(BashExecutor::new("/bin/bash".into())), http:Rc::new(HttpExecutor::new(None).unwrap()) });
        let job=producers.start(handle,source,decision).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5),async {
            while !directory.path().join("capture-started").exists() { tokio::task::yield_now().await; }
        }).await.unwrap();
        f.human.close();
        assert!(f.node.is_locked());
        tokio::time::timeout(std::time::Duration::from_secs(5),async {
            loop {
                if f.store.load(f.node.identity()).unwrap().captures[0].candidate.is_some() { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        let snapshot=f.store.load(f.node.identity()).unwrap();
        assert_eq!(snapshot.captures[0].state,CaptureState::CandidateHeld);
        assert!(snapshot.captures[0].dispatched);
        assert_eq!(snapshot.captures[0].candidate.as_ref().unwrap().header.secret,mutation.reference.secret);
        assert!(f.cloud.records.borrow().is_empty());
        assert_eq!(job.retry_persistence().unwrap(),None);
        // The entire raw stdout/stderr, including duplicates/base64, has no public result path.
        let safe=serde_json::to_string(&snapshot.captures[0].receipt()).unwrap();
        assert!(!safe.contains("produced-"));
    }).await;
}

#[test]
fn rejected_name_conflict_retains_ciphertext_and_does_not_wedge_unrelated_saves() {
    let (f, intent) = fixture(Approval::None);
    write(&f, intent.clone(), "first-private-value");
    let mut conflict = intent.clone();
    conflict.reference.secret = Uuid::new_v4();
    let (mutation, outcome) = write(&f, conflict, "held-conflicting-value");
    assert_eq!(outcome, CommitOutcome::RejectedNameConflict);
    assert!(
        f.node
            .pending_mutations(&f.session, &f.plugin)
            .unwrap()
            .is_empty()
    );
    let drafts = f.node.rejected_mutations(&f.session, &f.plugin).unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].0.prepared_entry, mutation);
    let persisted = serde_json::to_string(&f.store.load(f.node.identity()).unwrap()).unwrap();
    assert!(!persisted.contains("held-conflicting-value"));
    let mut unrelated = intent.clone();
    unrelated.reference.secret = Uuid::new_v4();
    unrelated.metadata.name = "another".into();
    assert_eq!(
        write(&f, unrelated, "another-private-value").1,
        CommitOutcome::Accepted
    );
    let mut renamed = drafts[0].0.metadata.clone();
    renamed.name = "resolved-name".into();
    renamed.policy_generation = Uuid::new_v4();
    let prepared = f
        .node
        .prepare_rejected(&f.session, mutation, renamed, None)
        .unwrap();
    let decision = f
        .human
        .decide(&("vault-mutation/v1", prepared.intent()))
        .unwrap();
    assert_eq!(
        block_on(f.node.write(&f.session, prepared, decision))
            .unwrap()
            .1,
        CommitOutcome::Accepted
    );
}

#[test]
fn an_agent_cannot_widen_existing_scopes_or_silently_change_replacement_policy() {
    let (f, mut intent) = fixture(Approval::Operation);
    intent.metadata.projects = Scope::Only(BTreeSet::from([Uuid::new_v4()]));
    let (revision, _) = write(&f, intent.clone(), "existing-private-value");
    let mut widened = intent.metadata.clone();
    widened.projects = Scope::Unrestricted;
    widened.approval = Approval::None;
    widened.policy_generation = Uuid::new_v4();
    assert_eq!(
        f.node.check_agent_mutation(
            &f.session,
            &f.binding,
            intent.reference,
            Some(revision),
            &widened
        ),
        Err(Error::Forbidden)
    );
    let capture = CaptureIntent {
        identity: f.node.identity(),
        originating_operation: Uuid::new_v4(),
        execution: f.binding.identity().clone(),
        provider: f.plugin.provider().into(),
        producer_generation: f.plugin.generation(),
        destination: "native:random".into(),
        profile: "test-scoped".into(),
        secret: intent.reference.secret,
        expected: Some(revision),
        metadata: widened,
        attempt: Uuid::new_v4(),
        settlement_ms: 1000,
    };
    assert!(matches!(
        block_on(
            f.node
                .prepare_capture(&f.session, &f.binding, &f.plugin, capture)
        ),
        Err(Error::Forbidden)
    ));
    assert!(f.store.load(f.node.identity()).unwrap().captures.is_empty());
}

#[test]
fn duplicate_capture_observers_do_not_cancel_the_owner_and_cancelled_prompts_release_capacity() {
    let (f, intent) = fixture(Approval::None);
    let make = || CaptureIntent {
        identity: f.node.identity(),
        originating_operation: Uuid::new_v4(),
        execution: f.binding.identity().clone(),
        provider: f.plugin.provider().into(),
        producer_generation: f.plugin.generation(),
        destination: "native:random".into(),
        profile: "test-cancel".into(),
        secret: Uuid::new_v4(),
        expected: None,
        metadata: intent.metadata.clone(),
        attempt: Uuid::new_v4(),
        settlement_ms: 1000,
    };
    let original = make();
    let handle =
        block_on(
            f.node
                .prepare_capture(&f.session, &f.binding, &f.plugin, original.clone()),
        )
        .unwrap();
    let mut redelivery = original;
    redelivery.attempt = Uuid::new_v4();
    let observer = block_on(
        f.node
            .prepare_capture(&f.session, &f.binding, &f.plugin, redelivery),
    )
    .unwrap();
    assert_eq!(observer.record().unwrap().id, handle.record().unwrap().id);
    drop(observer);
    assert_eq!(
        handle.record().unwrap().state,
        CaptureState::AwaitingApproval
    );
    drop(handle);
    for _ in 0..70 {
        drop(
            block_on(
                f.node
                    .prepare_capture(&f.session, &f.binding, &f.plugin, make()),
            )
            .unwrap(),
        );
    }
    assert!(
        f.store
            .load(f.node.identity())
            .unwrap()
            .captures
            .iter()
            .all(|capture| capture.state == CaptureState::Cancelled)
    );
}

#[test]
fn decoded_output_masks_longer_overlapping_secrets_before_shorter_ones() {
    let (f, short) = fixture(Approval::None);
    write(&f, short.clone(), "abc");
    let mut long = short.clone();
    long.reference.secret = Uuid::new_v4();
    long.metadata.name = "long".into();
    write(&f, long.clone(), "abcdef");
    let leases = vec![
        f.node
            .lease(&f.session, &f.binding, &f.plugin, &use_intent(&f, &short))
            .unwrap(),
        f.node
            .lease(&f.session, &f.binding, &f.plugin, &use_intent(&f, &long))
            .unwrap(),
    ];
    let mask = OutputMask::new(leases).unwrap();
    let mut value = serde_json::json!({"abcdef":"abcdef abc"});
    mask.mask_json(&mut value);
    assert_eq!(
        value[long.reference.to_string()],
        format!("{} {}", long.reference, short.reference)
    );
}

#[test]
fn tampered_recovery_journal_is_refused_before_it_can_restore_or_dispatch_authority() {
    let (f, _) = fixture(Approval::None);
    let identity = f.node.identity();
    assert!(!f.store.load(identity).unwrap().signature.is_empty());
    f.store.0.borrow_mut().get_mut(&identity).unwrap().authority = None;
    let reopened = VaultNode::open(
        identity,
        f.device,
        f.keys.clone(),
        f.pins.clone(),
        f.store.clone(),
        f.cloud.clone(),
        f.authority.clone(),
    );
    assert!(matches!(reopened, Err(Error::Integrity)));
}

#[test]
fn retiring_a_physical_attempt_fences_its_lease_without_ending_the_conversation() {
    let (f, mutation) = fixture(Approval::None);
    write(&f, mutation.clone(), "synthetic-physical-scope-canary");
    let physical = f.binding.scoped();
    let plugin = f.plugin.clone();
    let request = SecretUseRequest {
        reference: mutation.reference,
        operation: Uuid::new_v4(),
        attempt: Uuid::new_v4(),
        destination: "synthetic:executor".into(),
    };
    let intent = f
        .node
        .prepare_use(&f.session, &physical, &plugin, request)
        .unwrap();
    physical.close();
    assert!(matches!(
        f.node.lease(&f.session, &physical, &plugin, &intent),
        Err(Error::Forbidden)
    ));
    assert!(f.binding.is_live());
    assert!(f.session.owner().is_live());
}

#[test]
fn private_entry_contributions_require_the_pinned_accepted_permission_and_never_return_values() {
    struct Sink(Rc<Cell<usize>>);
    impl PrivateEntrySink for Sink {
        fn accept(&self, intent: &MutationIntent, value: &SecretValue) -> Result<()> {
            assert_eq!(intent.metadata.name, "deployment");
            assert!(!value.expose().is_empty());
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    let (f, intent) = fixture(Approval::None);
    let calls = Rc::new(Cell::new(0));
    let provider = "extension.synthetic".to_owned();
    let permissions = BTreeSet::from([Permission::AcceptAndStore]);
    let decision = f
        .human
        .decide(&("plugin-permissions/v1", &provider, &permissions))
        .unwrap();
    let grant = PluginGrant::accept(&f.human, decision, provider.clone(), permissions).unwrap();
    assert!(!grant.is_core());
    let hook = PrivateEntryHook::new(
        provider.clone(),
        grant.clone(),
        Rc::new(Sink(calls.clone())),
    )
    .unwrap();
    assert!(
        PrivateEntryHook::new(
            "other-provider".into(),
            grant.clone(),
            Rc::new(Sink(calls.clone()))
        )
        .is_err()
    );
    let entries = PrivateEntries::new(move || vec![hook.clone()]);
    let value = SecretValue::new(b"synthetic-entry-canary".to_vec()).unwrap();
    entries.accept(&intent, &value).unwrap();
    assert_eq!(calls.get(), 1);
    grant.revoke();
    assert!(entries.accept(&intent, &value).is_err());
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_definitive_check_or_record_refusal_cannot_later_be_hidden_as_warm_offline() {
    for records in [false, true] {
        let (f, mutation) = fixture(Approval::None);
        write(&f, mutation.clone(), "synthetic-refusal-canary");
        let intent = use_intent(&f, &mutation);
        if records {
            f.cloud.changes_error.set(Some(Error::CloudRefused));
        } else {
            f.cloud.check_error.set(Some(Error::CloudRefused));
        }
        assert_eq!(
            block_on(f.node.refresh_current(&f.session)),
            Err(Error::CloudRefused)
        );
        f.cloud.check_error.set(Some(Error::Offline));
        f.cloud.changes_error.set(None);
        assert!(block_on(f.node.refresh_current(&f.session)).is_err());
        assert!(
            f.node
                .lease(&f.session, &f.binding, &f.plugin, &intent)
                .is_err()
        );
        if records {
            // An authority-only success cannot clear a known refusal from the record boundary.
            f.cloud.check_error.set(None);
            f.cloud.changes_error.set(Some(Error::Offline));
            block_on(f.node.refresh_authority(&f.session)).unwrap();
            assert!(block_on(f.node.refresh_current(&f.session)).is_err());
            assert!(
                f.node
                    .lease(&f.session, &f.binding, &f.plugin, &intent)
                    .is_err()
            );
        }
        f.cloud.check_error.set(None);
        f.cloud.changes_error.set(None);
        block_on(f.node.refresh_current(&f.session)).unwrap();
        assert!(
            f.node
                .lease(&f.session, &f.binding, &f.plugin, &intent)
                .is_ok()
        );
    }
}

#[test]
fn private_entry_hooks_run_once_only_after_current_online_save_admission() {
    struct Hook {
        calls: Rc<Cell<usize>>,
        cloud: Rc<Cloud>,
    }
    impl PrivateEntrySink for Hook {
        fn accept(&self, _: &MutationIntent, _: &SecretValue) -> Result<()> {
            assert!(self.cloud.records.borrow().is_empty());
            self.calls.set(self.calls.get() + 1);
            Ok(())
        }
    }
    for mode in [
        None,
        Some(VaultMode::ArchivedReadable),
        Some(VaultMode::Exposed),
        Some(VaultMode::Current),
    ] {
        let (f, intent) = fixture(Approval::None);
        let calls = Rc::new(Cell::new(0));
        let hook = PrivateEntryHook::new(
            "executor".into(),
            f.plugin.clone(),
            Rc::new(Hook {
                calls: calls.clone(),
                cloud: f.cloud.clone(),
            }),
        )
        .unwrap();
        let entries = PrivateEntries::new(move || vec![hook.clone()]);
        let value = SecretValue::new(b"synthetic-hook-admission".to_vec()).unwrap();
        let prepared = f
            .node
            .prepare_entry(&f.session, intent.reference, None, intent.metadata, &value)
            .unwrap();
        drop(value);
        let decision = f
            .human
            .decide(&("vault-mutation/v1", prepared.intent()))
            .unwrap();
        match mode {
            Some(mode) => f.cloud.mode.set(mode),
            None => f.cloud.online.set(false),
        }
        let result = block_on(f.node.write_entry(&f.session, prepared, decision, &entries));
        if mode == Some(VaultMode::Current) {
            assert_eq!(result.unwrap().1, CommitOutcome::Accepted);
            assert_eq!(calls.get(), 1);
        } else {
            assert!(result.is_err());
            assert_eq!(calls.get(), 0);
            assert!(f.cloud.records.borrow().is_empty());
        }
    }
}

#[test]
fn a_fenced_capture_can_rebase_or_discard_its_same_candidate_without_repeating_issuance() {
    for discard in [false, true] {
        let (f, mutation) = fixture(Approval::None);
        let intent = CaptureIntent {
            identity: f.node.identity(),
            originating_operation: Uuid::new_v4(),
            execution: f.binding.identity().clone(),
            provider: "executor".into(),
            producer_generation: f.plugin.generation(),
            destination: "native:generate".into(),
            profile: "native-random/v1".into(),
            secret: mutation.reference.secret,
            expected: None,
            metadata: mutation.metadata.clone(),
            attempt: Uuid::new_v4(),
            settlement_ms: 30_000,
        };
        let handle = block_on(
            f.node
                .prepare_capture(&f.session, &f.binding, &f.plugin, intent),
        )
        .unwrap();
        let record = handle.record().unwrap();
        let decision = f
            .human
            .decide(&("vault-blind-save/v1", record.id, &record.intent))
            .unwrap();
        admit_capture(&f, &handle, decision, std::time::Duration::from_secs(30))
            .unwrap()
            .push(SecretValue::new(b"same-issued-candidate".to_vec()).unwrap())
            .unwrap();
        f.cloud.undispatched.set(true);
        block_on(f.node.store_capture(&handle)).unwrap();
        assert_eq!(handle.record().unwrap().state, CaptureState::PendingStore);
        let record = handle.record().unwrap();
        let payload = f
            .node
            .capture_fence_digest(&f.session, &f.plugin, record.id)
            .unwrap();
        let decision = f
            .human
            .decide(&(
                "vault-fence-capture/v1",
                record.id,
                record.mutation,
                payload,
            ))
            .unwrap();
        assert_eq!(
            block_on(f.node.fence_capture(&handle, decision)).unwrap(),
            CommitOutcome::NotDispatched
        );
        let record = handle.record().unwrap();
        assert_eq!(record.state, CaptureState::NeedsDecision);
        if discard {
            let decision = f
                .human
                .decide(&(
                    "vault-discard-capture/v1",
                    record.id,
                    record.mutation,
                    "issuer-credential-may-remain-live",
                ))
                .unwrap();
            assert_eq!(
                block_on(f.node.discard_capture(&handle, decision))
                    .unwrap()
                    .state,
                CaptureState::Discarded
            );
        } else {
            let mut metadata = mutation.metadata;
            metadata.name = "renamed-issued-candidate".into();
            metadata.policy_generation = Uuid::new_v4();
            let decision = f
                .human
                .decide(&(
                    "vault-rebase-capture/v1",
                    record.id,
                    record.mutation,
                    &metadata,
                    Option::<Uuid>::None,
                ))
                .unwrap();
            assert_eq!(
                block_on(f.node.rebase_capture(&handle, metadata, None, decision))
                    .unwrap()
                    .state,
                CaptureState::CandidateHeld
            );
            f.cloud.undispatched.set(false);
            assert_eq!(
                block_on(f.node.store_capture(&handle)).unwrap().state,
                CaptureState::Saved
            );
            assert_eq!(f.cloud.records.borrow().len(), 1);
        }
    }
}

#[cfg(all(feature = "executors", unix))]
#[tokio::test(flavor = "current_thread")]
async fn normal_bash_completion_kills_background_members_that_closed_their_output() {
    use swarmie_vault::executors::*;
    struct Refuse;
    impl UseDecisions for Refuse {
        fn request<'a>(
            &'a self,
            _: &'a ExecutionBinding,
            _: &'a UseIntent,
        ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
            async { panic!("no credentials requested") }.boxed_local()
        }
    }
    let (f, _) = fixture(Approval::None);
    let directory = tempfile::tempdir().unwrap();
    let attempt = ExecutionAttempt {
        node: &f.node,
        session: &f.session,
        execution: &f.binding,
        plugin: &f.plugin,
        decisions: &Refuse,
        operation: Uuid::new_v4(),
        attempt: Uuid::new_v4(),
    };
    let receipt = BashExecutor::new("/bin/bash".into())
        .execute(
            &attempt,
            BashRequest {
                source: "(sleep 0.2; printf stale > marker) >/dev/null 2>&1 &".into(),
                cwd: directory.path().into(),
                environment: BTreeMap::new(),
                timeout_ms: 3000,
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.exit, Some(0));
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    assert!(!directory.path().join("marker").exists());
}

#[cfg(all(feature = "executors", unix))]
#[tokio::test(flavor = "current_thread")]
async fn bash_output_cut_mid_reflection_withholds_the_pending_secret_prefix() {
    use swarmie_vault::executors::*;
    struct Refuse;
    impl UseDecisions for Refuse {
        fn request<'a>(
            &'a self,
            _: &'a ExecutionBinding,
            _: &'a UseIntent,
        ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
            async { panic!("no additional approval expected") }.boxed_local()
        }
    }
    let (f, mutation) = fixture(Approval::None);
    let canary = format!("SYNTHETIC_{}", "A".repeat(65_526));
    assert_eq!(canary.len(), 65_536);
    write(&f, mutation.clone(), &canary);
    let directory = tempfile::tempdir().unwrap();
    let attempt = ExecutionAttempt {
        node: &f.node,
        session: &f.session,
        execution: &f.binding,
        plugin: &f.plugin,
        decisions: &Refuse,
        operation: Uuid::new_v4(),
        attempt: Uuid::new_v4(),
    };
    let receipt = BashExecutor::new("/bin/bash".into())
        .execute(
            &attempt,
            BashRequest {
                source:
                    "head -c 1040000 /dev/zero | tr '\\0' z; printf '%s' \"$SWARMIE_SECRET_LONG\""
                        .into(),
                cwd: directory.path().into(),
                environment: BTreeMap::from([("SWARMIE_SECRET_LONG".into(), mutation.reference)]),
                timeout_ms: 5000,
            },
        )
        .await
        .unwrap();
    assert!(receipt.truncated);
    assert!(!receipt.stdout.contains("SYNTHETIC_"));
    assert!(receipt.stdout.contains("withheld"));
}

#[cfg(feature = "executors")]
#[tokio::test(flavor = "current_thread")]
async fn actual_blind_https_capture_saves_one_value_and_withholds_residual_and_failed_extraction() {
    use executors::*;
    struct NoInputs;
    impl UseDecisions for NoInputs {
        fn request<'a>(
            &'a self,
            _: &'a ExecutionBinding,
            _: &'a UseIntent,
        ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
            async { Err(Error::Forbidden) }.boxed_local()
        }
    }
    tokio::task::LocalSet::new().run_until(async {
        for found in [true, false] {
            let (f, mutation)=fixture(Approval::None);
            let (endpoint,server)=https::serve(1,|request| {
                assert_eq!(request.method,"POST");
                assert_eq!(request.path,"/issue");
                https::Reply {status:200,headers:BTreeMap::new(),body:br#"{"token":"synthetic-capture-http-canary","duplicate":"synthetic-capture-http-canary","encoded":"c3ludGhldGljLWNhcHR1cmUtaHR0cC1jYW5hcnk="}"#.to_vec()}
            }).await;
            let source=CaptureSource::HttpJson {request:HttpRequest {
                url:format!("{endpoint}issue"),method:"POST".into(),body:None,credentials:BTreeMap::new(),fields:vec![],timeout_ms:5000,
            },pointer:if found {"/token"} else {"/missing"}.into()};
            let intent=CaptureIntent {identity:f.node.identity(),originating_operation:Uuid::new_v4(),execution:f.binding.identity().clone(),provider:f.plugin.provider().into(),producer_generation:f.plugin.generation(),destination:source.destination().unwrap(),profile:source.profile().unwrap(),secret:mutation.reference.secret,expected:None,metadata:mutation.metadata,attempt:Uuid::new_v4(),settlement_ms:source.timeout_ms()};
            let handle=Rc::new(f.node.prepare_capture(&f.session,&f.binding,&f.plugin,intent).await.unwrap());
            let record=handle.record().unwrap();
            let decision=f.human.decide(&("vault-blind-save/v1",record.id,&record.intent)).unwrap();
            let producers=Rc::new(CaptureProducers {node:f.node.clone(),session:f.session.clone(),execution:f.binding.clone(),plugin:f.plugin.clone(),decisions:Rc::new(NoInputs),bash:Rc::new(BashExecutor::new("/bin/bash".into())),http:Rc::new(HttpExecutor::new(Some(https::certificate())).unwrap())});
            let job=producers.start(handle.clone(),source,decision).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5),job.finished()).await.unwrap().unwrap();
            server.await.unwrap();
            let record=handle.record().unwrap();
            assert_eq!(record.state,if found {CaptureState::Saved}else{CaptureState::IssuedUnknown});
            assert_eq!(f.cloud.records.borrow().len(),usize::from(found));
            let safe=serde_json::to_string(&record.receipt()).unwrap();
            assert!(!safe.contains("synthetic-capture-http-canary"));
            assert!(!safe.contains("c3ludGhldGljLWNhcHR1cmUtaHR0cC1jYW5hcnk="));
            let journal=serde_json::to_string(&f.store.load(f.node.identity()).unwrap()).unwrap();
            assert!(!journal.contains("synthetic-capture-http-canary"));
            if found {
                let intent=f.node.prepare_use(&f.session,&f.binding,&f.plugin,SecretUseRequest{reference:mutation.reference,operation:Uuid::new_v4(),attempt:Uuid::new_v4(),destination:"https://example.invalid/use".into()}).unwrap();
                assert_eq!(f.node.lease(&f.session,&f.binding,&f.plugin,&intent).unwrap().deliver(|value|Ok(value.to_vec())).unwrap(),b"synthetic-capture-http-canary");
            }
        }
    }).await;
}
