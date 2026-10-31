//! Human recovery never starts a producer or exposes the retained candidate value.
use super::*;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    RecognizeMutation {
        mutation: Uuid,
    },
    FenceMutation {
        mutation: Uuid,
    },
    RetryDraft {
        mutation: Uuid,
        metadata: SecretMetadata,
        expected: Option<Uuid>,
    },
    DiscardDraft {
        mutation: Uuid,
    },
    Capture {
        capture: Uuid,
        action: CaptureAction,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureAction {
    Recognize,
    Fence,
    Store,
    Rebase {
        metadata: SecretMetadata,
        expected: Option<Uuid>,
    },
    Discard,
}
impl Control {
    pub(super) async fn recovery(&self, request: Request) -> tembra::Result<()> {
        let active = self.active()?;
        match request {
            Request::RecognizeMutation { mutation } => {
                active
                    .node
                    .recognize(&active.session, mutation)
                    .await
                    .map_err(failure)?;
            }
            Request::FenceMutation { mutation } => {
                let payload = active
                    .node
                    .mutation_fence_digest(&active.session, &self.grant, mutation)
                    .map_err(failure)?;
                let decision = active
                    .human
                    .decide(&(
                        "vault-abandon-mutation/v1",
                        active.node.identity(),
                        mutation,
                        payload,
                    ))
                    .map_err(failure)?;
                active
                    .node
                    .abandon_mutation(&active.session, mutation, decision)
                    .await
                    .map_err(failure)?;
            }
            Request::RetryDraft {
                mutation,
                metadata,
                expected,
            } => {
                let prepared = active
                    .node
                    .prepare_rejected(&active.session, mutation, metadata, expected)
                    .map_err(failure)?;
                let decision = active
                    .human
                    .decide(&("vault-mutation/v1", prepared.intent()))
                    .map_err(failure)?;
                let (_, outcome) = active
                    .node
                    .write(&active.session, prepared, decision)
                    .await
                    .map_err(failure)?;
                if outcome == CommitOutcome::Accepted {
                    let decision = active
                        .human
                        .decide(&(
                            "vault-discard-rejected/v1",
                            active.node.identity(),
                            mutation,
                        ))
                        .map_err(failure)?;
                    active
                        .node
                        .discard_rejected(&active.session, mutation, decision)
                        .map_err(failure)?;
                }
            }
            Request::DiscardDraft { mutation } => {
                let decision = active
                    .human
                    .decide(&(
                        "vault-discard-rejected/v1",
                        active.node.identity(),
                        mutation,
                    ))
                    .map_err(failure)?;
                active
                    .node
                    .discard_rejected(&active.session, mutation, decision)
                    .map_err(failure)?;
            }
            Request::Capture { capture, action } => {
                let summary = active
                    .node
                    .capture_summaries(&active.session, &self.grant)
                    .map_err(failure)?
                    .into_iter()
                    .find(|summary| summary.id == capture)
                    .ok_or_else(unavailable)?;
                let binding = self
                    .authority
                    .execution(
                        &active.human,
                        ExecutionIdentity {
                            account: active.human.identity().account,
                            execution: Uuid::new_v4(),
                            generation: Uuid::new_v4(),
                            project: None,
                            agent: None,
                            conversation: None,
                            main_agent: false,
                        },
                    )
                    .map_err(failure)?;
                let decision = active
                    .human
                    .decide(&(
                        "vault-manage-capture/v1",
                        active.node.identity(),
                        capture,
                        summary.original_operation,
                        summary.mutation,
                    ))
                    .map_err(failure)?;
                let handle = active
                    .node
                    .manage_capture(&active.session, &binding, &self.grant, capture, decision)
                    .map_err(failure)?;
                match action {
                    CaptureAction::Recognize => {
                        active
                            .node
                            .recognize_capture(&handle)
                            .await
                            .map_err(failure)?;
                    }
                    CaptureAction::Fence => {
                        let payload = active
                            .node
                            .capture_fence_digest(&active.session, &self.grant, capture)
                            .map_err(failure)?;
                        let decision = active
                            .human
                            .decide(&("vault-fence-capture/v1", capture, summary.mutation, payload))
                            .map_err(failure)?;
                        active
                            .node
                            .fence_capture(&handle, decision)
                            .await
                            .map_err(failure)?;
                    }
                    CaptureAction::Store => {
                        active.node.store_capture(&handle).await.map_err(failure)?;
                    }
                    CaptureAction::Rebase { metadata, expected } => {
                        let decision = active
                            .human
                            .decide(&(
                                "vault-rebase-capture/v1",
                                capture,
                                summary.mutation,
                                &metadata,
                                expected,
                            ))
                            .map_err(failure)?;
                        active
                            .node
                            .rebase_capture(&handle, metadata, expected, decision)
                            .await
                            .map_err(failure)?;
                    }
                    CaptureAction::Discard => {
                        let decision = active
                            .human
                            .decide(&(
                                "vault-discard-capture/v1",
                                capture,
                                summary.mutation,
                                "issuer-credential-may-remain-live",
                            ))
                            .map_err(failure)?;
                        active
                            .node
                            .discard_capture(&handle, decision)
                            .await
                            .map_err(failure)?;
                        self.jobs
                            .borrow_mut()
                            .retain(|job| job.target() != (active.node.identity(), capture));
                    }
                }
                binding.close();
            }
        }
        Ok(())
    }
}
