use crate::{Command, CommandId};
use futures::FutureExt;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use tembra::{Declaration, Resource};

struct Entry {
    command: Command,
    token: Rc<()>,
}

type Entries = RefCell<BTreeMap<CommandId, Entry>>;

/// Application metadata only. It does not store live objects or run handlers.
#[derive(Clone, Default)]
pub struct Catalog(Rc<Entries>);

impl Catalog {
    pub fn register(&self, command: Command) -> tembra::Result<Registration> {
        command.validate()?;
        let mut entries = self.0.borrow_mut();
        if entries.values().any(|entry| {
            entry.command.id == command.id
                || entry
                    .command
                    .aliases
                    .iter()
                    .any(|alias| command.aliases.contains(alias))
        }) {
            return Err(tembra::Error::new(
                tembra::Code::Conflict,
                "swarmie.commands.duplicate",
                "command identity or alias is already registered",
            ));
        }
        let id = command.id.clone();
        let token = Rc::new(());
        entries.insert(
            id.clone(),
            Entry {
                command,
                token: token.clone(),
            },
        );
        Ok(Registration {
            owner: Rc::downgrade(&self.0),
            id,
            token,
        })
    }

    /// Bind metadata withdrawal to Tembra's declaring resource lifecycle.
    pub fn register_owned(
        &self,
        declaration: &Declaration,
        command: Command,
    ) -> tembra::Result<Resource<Registration>> {
        let key = serde_json::to_string(&("commands.registration", &command.id))
            .map_err(crate::invalid)?;
        let catalog = self.clone();
        declaration.resource(
            &key,
            1,
            move || catalog.register(command),
            |registration, _| {
                async move {
                    registration.withdraw();
                    Ok(())
                }
                .boxed_local()
            },
        )
    }

    pub fn list(&self) -> Vec<Command> {
        self.0
            .borrow()
            .values()
            .map(|entry| entry.command.clone())
            .collect()
    }

    pub fn get(&self, id: &CommandId) -> tembra::Result<Command> {
        self.0
            .borrow()
            .get(id)
            .map(|entry| entry.command.clone())
            .ok_or_else(|| {
                tembra::Error::new(
                    tembra::Code::Unavailable,
                    "swarmie.commands.missing",
                    "command is not available",
                )
            })
    }
}

#[must_use = "dropping the registration withdraws the command"]
pub struct Registration {
    owner: Weak<Entries>,
    id: CommandId,
    token: Rc<()>,
}

impl Registration {
    pub fn withdraw(&self) {
        if let Some(owner) = self.owner.upgrade() {
            let mut entries = owner.borrow_mut();
            if entries
                .get(&self.id)
                .is_some_and(|entry| Rc::ptr_eq(&entry.token, &self.token))
            {
                entries.remove(&self.id);
            }
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.withdraw();
    }
}
