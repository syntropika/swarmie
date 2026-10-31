//! Logical owner retirement and narrower physical scopes are separate authority reductions.
use swarmie_permissions::*;
use uuid::Uuid;
fn shell(authority: &Authority) -> HumanSession {
    authority.authenticate_shell(ShellIdentity {
        cloud: None,
        account: None,
        installation: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    })
}
#[test]
fn physical_scope_close_does_not_end_the_conversation_and_logical_close_fences_every_scope() {
    let authority = Authority::new();
    let human = shell(&authority);
    let logical = authority
        .execution(
            &human,
            ExecutionIdentity {
                account: None,
                execution: Uuid::new_v4(),
                generation: Uuid::new_v4(),
                project: None,
                agent: None,
                conversation: Some(Uuid::new_v4()),
                main_agent: true,
            },
        )
        .unwrap();
    let first = logical.scoped();
    let second = logical.scoped();
    let nested = second.scoped();
    assert_eq!(first.identity(), second.identity());
    first.close();
    assert!(!first.is_live());
    assert!(logical.is_live());
    assert!(second.is_live());
    logical.close();
    assert!(!second.is_live());
    assert!(!nested.is_live());
    assert!(human.is_live());
}
#[test]
fn application_close_ends_independent_shell_owners_including_a_vault_independent_detector() {
    let authority = Authority::new();
    let anonymous = shell(&authority);
    let account = shell(&authority);
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = count.clone();
    anonymous.on_close(Box::new(move || observed.set(observed.get() + 1)));
    authority.end_all();
    assert!(!anonymous.is_live());
    assert!(!account.is_live());
    assert_eq!(count.get(), 1);
    authority.end_all();
    assert_eq!(count.get(), 1);
}

#[test]
fn model_credentials_cannot_discover_manage_or_capture_secrets() {
    let grant = PluginGrant::bundled(CorePlugin::ModelCredentials, "models.synthetic".into());
    assert!(
        grant
            .check("models.synthetic", Permission::RequestUse)
            .is_ok()
    );
    assert!(
        grant
            .check("models.synthetic", Permission::ReceiveForExecution)
            .is_ok()
    );
    for permission in [
        Permission::Discover,
        Permission::Manage,
        Permission::AcceptAndStore,
        Permission::RequestCapture,
    ] {
        assert!(grant.check("models.synthetic", permission).is_err());
    }
}

#[cfg(feature = "tembra")]
#[test]
fn independent_context_owners_cannot_exchange_equal_numbered_leases_and_next_turn_keeps_the_logical_owner()
 {
    use futures::{FutureExt, executor::LocalPool};
    let authority = Authority::new();
    let human = shell(&authority);
    let logical = authority
        .execution(
            &human,
            ExecutionIdentity {
                account: None,
                execution: Uuid::new_v4(),
                generation: Uuid::new_v4(),
                project: None,
                agent: None,
                conversation: Some(Uuid::new_v4()),
                main_agent: true,
            },
        )
        .unwrap();
    let a = ExecutionContexts::new(authority.clone());
    let b = ExecutionContexts::new(authority);
    let mut pool = LocalPool::new();
    let host = tembra::Host::new(pool.spawner());
    pool.run_until(host.root("same-owner", move |context| {
        async move {
            let first = a.bind(&context, logical.clone())?;
            let other = b.bind(&context, logical.clone())?;
            assert!(a.resolve(first.context()).is_ok());
            assert!(b.resolve(first.context()).is_err());
            assert!(a.resolve(other.context()).is_err());
            let identity = logical.identity().clone();
            drop(first);
            assert!(logical.is_live());
            assert!(b.resolve(other.context()).is_ok());
            drop(other);
            let next = a.bind(&context, logical.clone())?;
            assert_eq!(a.resolve(next.context())?.identity(), &identity);
            drop(next);
            assert!(logical.is_live());
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    pool.run_until(host.close()).unwrap();
}
