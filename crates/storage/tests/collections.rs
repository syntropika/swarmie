#![cfg(feature = "sqlite")]

use futures::{
    FutureExt,
    executor::{LocalPool, block_on},
};
use std::rc::Rc;
use swarmie_activity::{
    ActivityInput, Command as ActivityCommand, Snapshot as Inbox, Storage as ActivityStorage,
};
use swarmie_projects::{
    Command as ProjectCommand, Snapshot as Projects, Storage as ProjectStorage,
};
use swarmie_storage::sqlite::{SqliteActivityStorage, SqliteOwner, SqliteProjectsStorage};
use tembra::{Host, Reference};

fn project(id: &str) -> ProjectCommand {
    ProjectCommand::Create {
        id: id.into(),
        name: "Website".into(),
        description: "Details".into(),
    }
}

#[test]
fn projects_keep_membership_and_node_folders_across_reopen_and_principal_scopes() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("collections.sqlite");
    let storage = SqliteProjectsStorage::open(&file).unwrap();
    block_on(async {
        let mut current = storage.load("alice").await.unwrap();
        for command in [
            project("one"),
            project("two"),
            ProjectCommand::Assign {
                conversation: "chat".into(),
                project: "one".into(),
            },
            ProjectCommand::Folders {
                project: "one".into(),
                node: "node-a".into(),
                paths: vec!["/with space ".into(), "/with space ".into()],
            },
            ProjectCommand::Folders {
                project: "one".into(),
                node: "node-b".into(),
                paths: vec!["/other".into()],
            },
        ] {
            let next = current.clone().apply(command).unwrap();
            storage
                .replace("alice", current.revision, next.clone())
                .await
                .unwrap();
            current = next;
        }
        assert!(storage.load("bob").await.unwrap().records.is_empty());
        assert_eq!(current.personal_folders[0].paths, ["/with space "]);
        let stale = current
            .clone()
            .apply(ProjectCommand::Unassign {
                conversation: "chat".into(),
            })
            .unwrap();
        assert!(matches!(
            storage.replace("alice", 0, stale).await,
            Err(swarmie_projects::StorageError::Invalid(_))
        ));
        storage.close().await.unwrap();
    });
    let storage = SqliteProjectsStorage::open(&file).unwrap();
    let current = block_on(storage.load("alice")).unwrap();
    assert_eq!(current.links["chat"], "one");
    assert_eq!(current.personal_folders.len(), 2);
    assert_eq!(current.records.len(), 2);
    block_on(storage.close()).unwrap();
}

#[test]
fn concurrent_project_replacement_refuses_stale_revision_without_losing_data() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("collections.sqlite");
    let first = SqliteProjectsStorage::open(&file).unwrap();
    let second = SqliteProjectsStorage::open(&file).unwrap();
    block_on(async {
        let a = first
            .load("alice")
            .await
            .unwrap()
            .apply(project("one"))
            .unwrap();
        let b = second
            .load("alice")
            .await
            .unwrap()
            .apply(project("two"))
            .unwrap();
        first.replace("alice", 0, a.clone()).await.unwrap();
        assert!(matches!(
            second.replace("alice", 0, b).await,
            Err(swarmie_projects::StorageError::Conflict)
        ));
        assert_eq!(second.load("alice").await.unwrap(), a);
        first.close().await.unwrap();
        second.close().await.unwrap();
    });
}

fn notice(owner: &str, entity: &str, text: &str) -> ActivityCommand {
    ActivityCommand::Publish {
        input: ActivityInput {
            owner: owner.into(),
            entity: entity.into(),
            title: "Completed".into(),
            text: text.into(),
            conversation_id: Some("chat".into()),
        },
    }
}

#[test]
fn inbox_deduplicates_promotes_and_persists_read_state_without_any_ui() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("collections.sqlite");
    let storage = SqliteActivityStorage::open(&file).unwrap();
    block_on(async {
        let mut current = Inbox::default();
        for (time, command) in [
            (1, notice("tools", "one", "First")),
            (2, notice("tools", "two", "Second")),
            (3, notice("tools", "one", "Updated")),
            (4, notice("tasks", "one", "Other owner")),
        ] {
            let next = current.clone().apply(command, time).unwrap();
            storage
                .replace("alice", current.revision, next.clone())
                .await
                .unwrap();
            current = next;
        }
        assert_eq!(current.items.len(), 3);
        assert_eq!(current.items[1].text, "Updated");
        let read = current
            .clone()
            .apply(
                ActivityCommand::Read {
                    id: current.items[0].id.clone(),
                },
                5,
            )
            .unwrap();
        storage
            .replace("alice", current.revision, read)
            .await
            .unwrap();
        assert!(storage.load("bob").await.unwrap().items.is_empty());
        storage.close().await.unwrap();
    });
    let storage = SqliteActivityStorage::open(&file).unwrap();
    let current = block_on(storage.load("alice")).unwrap();
    assert!(!current.items[0].unread);
    assert!(current.items[1].unread);
    assert_eq!(current.items[1].conversation_id.as_deref(), Some("chat"));
    block_on(storage.close()).unwrap();
}

fn call<T: serde::de::DeserializeOwned + 'static, A: serde::Serialize + 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: Rc<Reference>,
    method: &'static str,
    args: A,
) -> tembra::Result<T> {
    pool.run_until(host.root("alice", move |context| {
        async move { reference.call(&context, method, args).await }.boxed_local()
    }))
}

#[test]
fn capability_objects_authorize_publish_observe_and_close_the_actual_storage_resources() {
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(directory.path().join("collections.sqlite"));
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let authorize = |principal: &str| {
        if principal == "alice" {
            Ok(())
        } else {
            Err(tembra::Error::new(
                tembra::Code::Forbidden,
                "test.principal",
                "unbound principal",
            ))
        }
    };
    host.install(vec![
        swarmie_projects::orb(owner.projects_factory(), authorize),
        swarmie_activity::orb(owner.activity_factory(), authorize),
    ])
    .unwrap();
    assert!(
        host.get("swarmie.projects", vec!["main".into()], "bob")
            .is_err()
    );
    let projects = Rc::new(
        host.get("swarmie.projects", vec!["main".into()], "alice")
            .unwrap(),
    );
    let inbox = Rc::new(
        host.get("swarmie.activity", vec!["main".into()], "alice")
            .unwrap(),
    );
    pool.run_until(projects.ready()).unwrap();
    pool.run_until(inbox.ready()).unwrap();
    let created: swarmie_projects::Revision = call(
        &mut pool,
        &host,
        projects.clone(),
        "change",
        (project("one"),),
    )
    .unwrap();
    assert_eq!(created.revision, 1);
    let published: swarmie_activity::Revision = call(
        &mut pool,
        &host,
        inbox.clone(),
        "change",
        (notice("fixture.plugin", "ready", "Ready"),),
    )
    .unwrap();
    assert_eq!(published.revision, 1);
    let state = inbox.state::<swarmie_activity::Revision>().unwrap();
    assert_eq!(state.value().unwrap().as_ref(), &published);
    projects.release().unwrap();
    inbox.release().unwrap();
    pool.run_until_stalled();
    let reopened = Rc::new(
        host.get("swarmie.activity", vec!["main".into()], "alice")
            .unwrap(),
    );
    pool.run_until(reopened.ready()).unwrap();
    let listed: swarmie_activity::Page =
        call(&mut pool, &host, reopened.clone(), "list", ()).unwrap();
    assert_eq!(listed.revision, published.revision);
    let decoded: Inbox = serde_json::from_str(&listed.data).unwrap();
    assert_eq!(decoded.items.len(), 1);
    reopened.release().unwrap();
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn project_import_never_overwrites_an_existing_collection_and_invalid_membership_is_refused() {
    let source = Projects::default().apply(project("legacy")).unwrap();
    let imported = Projects::default()
        .apply(ProjectCommand::Import {
            data: source.clone(),
        })
        .unwrap();
    assert_eq!(imported.records, source.records);
    assert!(
        imported
            .clone()
            .apply(ProjectCommand::Import { data: source })
            .is_err()
    );
    assert!(
        imported
            .apply(ProjectCommand::Assign {
                conversation: "chat".into(),
                project: "missing".into()
            })
            .is_err()
    );
}

#[test]
fn lossless_project_pages_and_small_receipts_preserve_a_single_item_larger_than_the_carrier() {
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(directory.path().join("large.sqlite"));
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_projects::orb(
        owner.projects_factory(),
        |_| Ok(()),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get("swarmie.projects", vec!["main".into()], "alice")
            .unwrap(),
    );
    pool.run_until(reference.ready()).unwrap();
    let description = "\0🙂\"\\\n".repeat(30_000);
    let receipt: swarmie_projects::Revision = call(
        &mut pool,
        &host,
        reference.clone(),
        "change",
        (ProjectCommand::Create {
            id: "large".into(),
            name: "Large project".into(),
            description: description.clone(),
        },),
    )
    .unwrap();
    assert!(serde_json::to_vec(&receipt).unwrap().len() < 128);
    assert!(
        serde_json::to_vec(
            reference
                .state::<swarmie_projects::Revision>()
                .unwrap()
                .value()
                .unwrap()
                .as_ref()
        )
        .unwrap()
        .len()
            < 128
    );
    let first: swarmie_projects::Page =
        call(&mut pool, &host, reference.clone(), "list", ()).unwrap();
    let stale = first.next.clone().unwrap();
    let mut page = first;
    let mut text = String::new();
    let mut pages = 0;
    loop {
        assert!(serde_json::to_vec(&page).unwrap().len() <= 48 * 1024);
        assert_eq!(page.revision, receipt.revision);
        text.push_str(&page.data);
        pages += 1;
        let Some(next) = page.next else {
            break;
        };
        page = call(&mut pool, &host, reference.clone(), "page", (next,)).unwrap();
    }
    assert!(pages > 1);
    let snapshot: Projects = serde_json::from_str(&text).unwrap();
    assert_eq!(snapshot.records[0].description, description.trim());
    let _: swarmie_projects::Revision = call(
        &mut pool,
        &host,
        reference.clone(),
        "change",
        (project("second"),),
    )
    .unwrap();
    let stale_result: tembra::Result<swarmie_projects::Page> =
        call(&mut pool, &host, reference.clone(), "page", (stale,));
    assert_eq!(stale_result.unwrap_err().code, tembra::Code::Conflict);
    reference.release().unwrap();
    pool.run_until(host.close()).unwrap();
}

#[test]
fn lossless_activity_pages_preserve_large_entries_without_limiting_the_durable_inbox() {
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(directory.path().join("large.sqlite"));
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_activity::orb(
        owner.activity_factory(),
        |_| Ok(()),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get("swarmie.activity", vec!["main".into()], "alice")
            .unwrap(),
    );
    pool.run_until(reference.ready()).unwrap();
    let text = "\0🙂\"\\\n".repeat(30_000);
    let receipt: swarmie_activity::Revision = call(
        &mut pool,
        &host,
        reference.clone(),
        "change",
        (notice("fixture", "large", &text),),
    )
    .unwrap();
    assert!(serde_json::to_vec(&receipt).unwrap().len() < 128);
    let mut page: swarmie_activity::Page =
        call(&mut pool, &host, reference.clone(), "list", ()).unwrap();
    let stale = page.next.clone().unwrap();
    let mut joined = String::new();
    loop {
        assert!(serde_json::to_vec(&page).unwrap().len() <= 48 * 1024);
        assert_eq!(page.revision, receipt.revision);
        joined.push_str(&page.data);
        let Some(next) = page.next else {
            break;
        };
        page = call(&mut pool, &host, reference.clone(), "page", (next,)).unwrap();
    }
    let snapshot: Inbox = serde_json::from_str(&joined).unwrap();
    assert_eq!(snapshot.items[0].text, text);
    let _: swarmie_activity::Revision = call(
        &mut pool,
        &host,
        reference.clone(),
        "change",
        (notice("fixture", "second", "Next"),),
    )
    .unwrap();
    let stale_result: tembra::Result<swarmie_activity::Page> =
        call(&mut pool, &host, reference.clone(), "page", (stale,));
    assert_eq!(stale_result.unwrap_err().code, tembra::Code::Conflict);
    reference.release().unwrap();
    pool.run_until(host.close()).unwrap();
}
