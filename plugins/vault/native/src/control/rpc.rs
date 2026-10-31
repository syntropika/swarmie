//! Private shell declarations. The byte carrier cannot mint human or execution authority.
use super::*;
use tembra::{Service, State};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    metadata: SecretMetadata,
    value: String,
    reference: Option<String>,
    revision: Option<Uuid>,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Found {
    expected_current: Option<Uuid>,
}

pub(crate) fn orb(control: Rc<Control>) -> Orb {
    let background = Rc::downgrade(&control);
    Orb::new(SERVICE, 1)
        .create(move |declaration| {
            super::reconciliation::resource(&declaration, background.clone())
        })
        .service(
            Service::new(SERVICE, move |declaration| {
                if declaration.address()?.principal != PRINCIPAL {
                    return Err(unavailable());
                }
                Ok((
                    control.clone(),
                    declaration.state(control.project()?, 1, |_| Ok(()))?,
                ))
            })
            .method(
                "submit_entry",
                |env, context, (id, text): (Uuid, PrivateText)| async move {
                    authorize(&context)?;
                    env.0.submit_entry(id, text).await?;
                    publish(&env, &context)
                },
            )
            .method(
                "decide",
                |env, context, (id, choice): (Uuid, String)| async move {
                    authorize(&context)?;
                    env.0.clone().decide(id, choice).await?;
                    publish(&env, &context)
                },
            )
            .method(
                "resolve_founding",
                |env, context, (vault,): (Uuid,)| async move {
                    authorize(&context)?;
                    let account = env.0.account()?;
                    let root = env
                        .0
                        .configuration
                        .borrow()
                        .roots
                        .iter()
                        .find(|root| root.identity.vault == vault && root.pending.is_some())
                        .cloned()
                        .ok_or_else(unavailable)?;
                    let bundle = OsKeyring
                        .load_bundle(root.identity, root.device)
                        .map_err(failure)?;
                    let outcome = fence_founding(
                        &account,
                        root.pending.as_ref().ok_or_else(unavailable)?,
                        root.device,
                        &bundle.keys,
                    )
                    .await
                    .map_err(failure)?;
                    if outcome != CommitOutcome::Unknown {
                        env.0.settle_founding(
                            root.pending.as_ref().ok_or_else(unavailable)?,
                            outcome,
                        )?;
                    }
                    env.0.refresh().await;
                    publish(&env, &context)
                },
            )
            .method(
                "lifecycle",
                |env, context, (request,): (lifecycle::Request,)| async move {
                    authorize(&context)?;
                    let result = env.0.lifecycle(request).await?;
                    publish(&env, &context)?;
                    Ok(result)
                },
            )
            .method(
                "recovery",
                |env, context, (request,): (recovery::Request,)| async move {
                    authorize(&context)?;
                    env.0.recovery(request).await?;
                    publish(&env, &context)
                },
            )
            .method(
                "page",
                |env, context, (key, offset): (String, usize)| async move {
                    authorize(&context)?;
                    env.0.select_page(key, offset)?;
                    publish(&env, &context)
                },
            )
            .method("read", |env, context, (): ()| async move {
                authorize(&context)?;
                publish(&env, &context)
            })
            .method(
                "login",
                |env, context, (endpoints,): (Endpoints,)| async move {
                    authorize(&context)?;
                    env.0.clone().login(endpoints).await
                },
            )
            .method("logout", |env, context, (): ()| async move {
                authorize(&context)?;
                env.0.close();
                let account = env.0.account.borrow_mut().take();
                if let Some(account) = account {
                    let _ = account.logout().await;
                }
                env.0.configuration.borrow_mut().account = None;
                env.0.tried_unlock.set(false);
                env.0.needs_sign_in.set(false);
                *env.0.catalog.borrow_mut() = None;
                let installation = env.0.configuration.borrow().installation;
                *env.0.shell.borrow_mut() = env.0.authority.authenticate_shell(ShellIdentity {
                    cloud: None,
                    account: None,
                    installation,
                    generation: Uuid::new_v4(),
                });
                env.0.persist()?;
                publish(&env, &context)
            })
            .method(
                "unlock",
                |env, context, (identity,): (Option<VaultIdentity>,)| async move {
                    authorize(&context)?;
                    env.0.unlock(identity).await?;
                    publish(&env, &context)
                },
            )
            .method("synchronize", |env, context, (): ()| async move {
                authorize(&context)?;
                let active = env.0.active()?;
                active
                    .node
                    .synchronize(&active.session)
                    .await
                    .map_err(failure)?;
                publish(&env, &context)
            })
            .method("save", |env, context, (mut entry,): (Entry,)| async move {
                authorize(&context)?;
                let active = env.0.active()?;
                if !active
                    .node
                    .pending_mutations(&active.session, &env.0.grant)
                    .map_err(failure)?
                    .is_empty()
                {
                    return Err(unavailable());
                }
                let reference = if let Some(reference) = &entry.reference {
                    find(&active, &env.0.grant, reference)?
                } else {
                    SecretReference {
                        account: active.node.identity().account,
                        vault: active.node.identity().vault,
                        secret: Uuid::new_v4(),
                    }
                };
                let value = SecretValue::new(std::mem::take(&mut entry.value).into_bytes())
                    .map_err(failure)?;
                let prepared = active
                    .node
                    .prepare_entry(
                        &active.session,
                        reference,
                        entry.revision,
                        entry.metadata.clone(),
                        &value,
                    )
                    .map_err(failure)?;
                drop(value);
                let decision = active
                    .human
                    .decide(&("vault-mutation/v1", prepared.intent()))
                    .map_err(failure)?;
                active
                    .node
                    .write_entry(&active.session, prepared, decision, &env.0.entries)
                    .await
                    .map_err(failure)?;
                publish(&env, &context)
            })
            .method(
                "remove",
                |env, context, (reference, revision): (String, Uuid)| async move {
                    authorize(&context)?;
                    let active = env.0.active()?;
                    let selected = find(&active, &env.0.grant, &reference)?;
                    let record = active
                        .node
                        .list(&active.session, &env.0.grant)
                        .map_err(failure)?
                        .into_iter()
                        .find(|r| r.secret == selected.secret)
                        .ok_or_else(unavailable)?;
                    let prepared = active
                        .node
                        .prepare_mutation(
                            &active.session,
                            selected,
                            Some(revision),
                            record.metadata,
                            None,
                        )
                        .map_err(failure)?;
                    let decision = active
                        .human
                        .decide(&("vault-mutation/v1", prepared.intent()))
                        .map_err(failure)?;
                    active
                        .node
                        .write(&active.session, prepared, decision)
                        .await
                        .map_err(failure)?;
                    publish(&env, &context)
                },
            )
            .method("found", |env, context, (request,): (Found,)| async move {
                authorize(&context)?;
                let account = env.0.account()?;
                let keys = DeviceKeys::generate();
                let identity = VaultIdentity {
                    account: account.identity().account,
                    vault: Uuid::new_v4(),
                    epoch: Uuid::new_v4(),
                };
                let device = Uuid::new_v4();
                let human = env.0.authority.authenticate_shell(ShellIdentity {
                    cloud: Some(CloudOwnerIdentity {
                        client: digest(&keys.signing_public()),
                        family: account.identity().family,
                    }),
                    account: Some(identity.account),
                    installation: account.identity().installation,
                    generation: Uuid::new_v4(),
                });
                let intent = FoundIntent {
                    identity,
                    device,
                    expected_current: request.expected_current,
                };
                let decision = human
                    .decide(&("vault-found/v1", &intent))
                    .map_err(failure)?;
                let (bundle, command, receipt) =
                    found(&human, decision, intent, keys).map_err(failure)?;
                OsKeyring
                    .save_bundle(identity, device, &bundle)
                    .map_err(failure)?;
                env.0.configuration.borrow_mut().roots.push(Root {
                    identity,
                    device,
                    previous_devices: vec![],
                    pending_replacement: None,
                    pending: Some(receipt.clone()),
                    verified_code: None,
                });
                env.0.persist()?;
                let outcome = match submit_ceremony(&account, &command, &receipt).await {
                    Ok(outcome) => outcome,
                    Err(_) => CommitOutcome::Unknown,
                };
                if outcome == CommitOutcome::Unknown {
                    return Err(unavailable());
                }
                env.0.settle_founding(&receipt, outcome)?;
                if outcome == CommitOutcome::Accepted {
                    env.0.unlock(Some(identity)).await?;
                }
                publish(&env, &context)
            }),
        )
}
fn authorize(context: &Context) -> tembra::Result<()> {
    context.check()?;
    if context.principal()? != PRINCIPAL {
        return Err(unavailable());
    }
    Ok(())
}
fn publish(env: &(Rc<Control>, State<Value>), context: &Context) -> tembra::Result<Value> {
    authorize(context)?;
    let next = env.0.project()?;
    env.1.update(context, |_| Ok(next.clone()))?;
    Ok(next)
}
fn find(active: &Active, grant: &PluginGrant, reference: &str) -> tembra::Result<SecretReference> {
    active
        .node
        .list(&active.session, grant)
        .map_err(failure)?
        .into_iter()
        .find_map(|record| {
            let selected = SecretReference {
                account: record.identity.account,
                vault: record.identity.vault,
                secret: record.secret,
            };
            (selected.to_string() == reference).then_some(selected)
        })
        .ok_or_else(unavailable)
}
