use crate::{
    BlockReason, Catalog, CommitOutcome, InstalledPlugin, Inventory, MountFailure, OrbInfo,
    PluginChange, PluginId, PluginSnapshot, PluginStatus, PluginStorage, PluginView,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    rc::Rc,
};
use tembra::{Code, Error, Host, ResourceSignal};

#[derive(Clone, Copy)]
pub(crate) enum Reason {
    Storage,
    InvalidInventory,
    Busy,
    Conflict,
    RecoveryRequired,
    Forbidden,
    Closed,
    Address,
}

impl Reason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "swarmie.plugin_manager.storage",
            Self::InvalidInventory => "swarmie.plugin_manager.invalid_inventory",
            Self::Busy => "swarmie.plugin_manager.busy",
            Self::Conflict => "swarmie.plugin_manager.conflict",
            Self::RecoveryRequired => "swarmie.plugin_manager.recovery_required",
            Self::Forbidden => "swarmie.plugin_manager.forbidden",
            Self::Closed => "swarmie.plugin_manager.closed",
            Self::Address => "swarmie.plugin_manager.address",
        }
    }
}

pub(crate) fn storage_error(error: crate::StorageError) -> Error {
    Error::new(
        Code::Unavailable,
        Reason::Storage.as_str(),
        error.to_string(),
    )
}

struct Mounted {
    version: NonZeroU32,
    orbs: Vec<OrbInfo>,
}

pub(crate) struct PluginManager {
    host: Host,
    catalog: Catalog,
    storage: Rc<dyn PluginStorage>,
    inventory: RefCell<Inventory>,
    mounted: RefCell<BTreeMap<PluginId, Mounted>>,
    snapshot: RefCell<PluginSnapshot>,
    busy: Cell<bool>,
    closed: Cell<bool>,
    health_subscription: RefCell<Option<swarmie_storage_core::Subscription>>,
}

impl PluginManager {
    pub(crate) async fn open(
        host: Host,
        catalog: Catalog,
        storage: Rc<dyn PluginStorage>,
        signal: ResourceSignal,
    ) -> tembra::Result<Self> {
        let owner = storage.owner();
        owner.check().map_err(|error| storage_error(error.into()))?;
        let index = catalog.index();
        let health_subscription = owner.changes().subscribe(move || index.invalidate());
        let mut inventory = storage.inventory().await.map_err(storage_error)?;
        validate_inventory(&inventory)?;
        crate::required::initialize(&catalog, &storage, &mut inventory, &signal).await?;
        owner.check().map_err(|error| storage_error(error.into()))?;
        signal.check()?;
        let manager = Self {
            host,
            catalog,
            snapshot: RefCell::new(PluginSnapshot {
                inventory_revision: inventory.revision,
                plugins: Vec::new(),
            }),
            inventory: RefCell::new(inventory),
            storage,
            mounted: RefCell::new(BTreeMap::new()),
            busy: Cell::new(false),
            closed: Cell::new(false),
            health_subscription: RefCell::new(Some(health_subscription)),
        };
        manager.reconcile();
        if !manager.catalog.index().required_active() {
            return Err(crate::required::unavailable(
                "required plugins could not be mounted",
            ));
        }
        Ok(manager)
    }

    pub(crate) fn snapshot(&self) -> PluginSnapshot {
        self.snapshot.borrow().clone()
    }

    pub(crate) async fn configure(
        &self,
        plugin: InstalledPlugin,
    ) -> tembra::Result<PluginSnapshot> {
        let _guard = self.acquire()?;
        let mut next = self.inventory.borrow().clone();
        next.plugins.retain(|record| record.id != plugin.id);
        next.plugins.push(plugin.clone());
        next.plugins.sort_by(|left, right| left.id.cmp(&right.id));
        crate::required::validate(&self.catalog, &next)?;
        match self
            .storage
            .commit(next.revision, vec![PluginChange::Put(plugin)])
            .await
            .map_err(|error| {
                self.catalog.index().invalidate();
                storage_error(error)
            })? {
            CommitOutcome::Rejected { current_revision } => {
                self.catalog.index().invalidate();
                return Err(Error::new(
                    Code::Conflict,
                    Reason::Conflict.as_str(),
                    format!(
                        "inventory changed to revision {current_revision}; refresh before retrying"
                    ),
                ));
            }
            CommitOutcome::Unknown { message } => {
                self.storage.owner().require_recovery();
                self.catalog.index().invalidate();
                return Err(Error::new(
                    Code::Unavailable,
                    Reason::RecoveryRequired.as_str(),
                    message,
                ));
            }
            CommitOutcome::Applied { revision } => next.revision = revision,
        }
        *self.inventory.borrow_mut() = next;
        self.reconcile();
        Ok(self.snapshot())
    }

    pub(crate) async fn refresh(&self) -> tembra::Result<PluginSnapshot> {
        let _guard = self.acquire()?;
        if !self.catalog.2.is_empty() {
            self.catalog.index().invalidate();
        }
        let inventory = self.storage.inventory().await.map_err(storage_error)?;
        validate_inventory(&inventory)?;
        if let Err(error) = crate::required::validate(&self.catalog, &inventory) {
            self.catalog.index().invalidate();
            return Err(error);
        }
        *self.inventory.borrow_mut() = inventory;
        self.reconcile();
        Ok(self.snapshot())
    }

    pub(crate) fn close(&self) {
        self.closed.set(true);
        self.health_subscription.borrow_mut().take();
        self.catalog.index().invalidate();
    }

    fn acquire(&self) -> tembra::Result<MutationGuard<'_>> {
        if self.closed.get() {
            return Err(Error::new(
                Code::Closed,
                Reason::Closed.as_str(),
                "plugin administration is closed",
            ));
        }
        if self.storage.owner().check().is_err() {
            self.catalog.index().invalidate();
            return Err(Error::new(
                Code::Unavailable,
                Reason::RecoveryRequired.as_str(),
                "storage outcome is uncertain; close and recover the storage owner before reopening",
            ));
        }
        if self.busy.replace(true) {
            return Err(Error::new(
                Code::Conflict,
                Reason::Busy.as_str(),
                "another inventory mutation is still settling",
            ));
        }
        Ok(MutationGuard(&self.busy))
    }

    fn reconcile(&self) {
        let inventory = self.inventory.borrow();
        let mut views = BTreeMap::new();
        let mut pending = BTreeSet::new();
        for desired in &inventory.plugins {
            let mounted = self.mounted.borrow();
            let installed = mounted.get(&desired.id);
            let observed = if let Some(plugin) = installed {
                PluginStatus::Mounted {
                    orbs: plugin.orbs.clone(),
                }
            } else if !desired.enabled {
                PluginStatus::Disabled
            } else if let Some(definition) = self.catalog.0.get(&desired.id) {
                if definition.version != desired.version {
                    PluginStatus::Failed {
                        failure: MountFailure::VersionMismatch {
                            available: definition.version,
                        },
                    }
                } else {
                    pending.insert(desired.id.clone());
                    PluginStatus::Blocked {
                        reason: BlockReason::Dependencies {
                            plugins: definition.dependencies.clone(),
                        },
                    }
                }
            } else {
                PluginStatus::Failed {
                    failure: MountFailure::DefinitionMissing,
                }
            };
            let restart_required = installed
                .is_some_and(|mounted| !desired.enabled || mounted.version != desired.version);
            drop(mounted);
            views.insert(
                desired.id.clone(),
                PluginView {
                    id: desired.id.clone(),
                    desired: Some(desired.clone()),
                    observed,
                    restart_required,
                },
            );
        }
        for (id, mounted) in self.mounted.borrow().iter() {
            views.entry(id.clone()).or_insert_with(|| PluginView {
                id: id.clone(),
                desired: None,
                observed: PluginStatus::Mounted {
                    orbs: mounted.orbs.clone(),
                },
                restart_required: true,
            });
        }
        loop {
            let ready: Vec<_> = pending
                .iter()
                .filter(|id| {
                    self.catalog.0[*id]
                        .dependencies
                        .iter()
                        .all(|dependency| views.get(dependency).is_some_and(enabled_and_mounted))
                })
                .cloned()
                .collect();
            if ready.is_empty() {
                break;
            }
            for id in ready {
                pending.remove(&id);
                let definition = &self.catalog.0[&id];
                let outcome = self.mount(definition);
                if let PluginStatus::Mounted { orbs } = &outcome {
                    self.mounted.borrow_mut().insert(
                        id.clone(),
                        Mounted {
                            version: definition.version,
                            orbs: orbs.clone(),
                        },
                    );
                }
                views
                    .get_mut(&id)
                    .expect("pending plugin has a view")
                    .observed = outcome;
            }
        }
        for id in pending {
            let dependencies = self.catalog.0[&id]
                .dependencies
                .iter()
                .filter(|dependency| !views.get(*dependency).is_some_and(enabled_and_mounted))
                .cloned()
                .collect();
            views
                .get_mut(&id)
                .expect("pending plugin has a view")
                .observed = PluginStatus::Blocked {
                reason: BlockReason::Dependencies {
                    plugins: dependencies,
                },
            };
        }
        let mut active: BTreeSet<_> = views
            .iter()
            .filter(|(_, view)| enabled_and_mounted(view) && !view.restart_required)
            .map(|(id, _)| id.clone())
            .collect();
        loop {
            let before = active.clone();
            active.retain(|id| {
                self.catalog.0[id]
                    .dependencies
                    .iter()
                    .all(|dependency| before.contains(dependency))
            });
            if before == active {
                break;
            }
        }
        if self.storage.owner().check().is_err() {
            active.clear();
        }
        self.catalog.1.publish(
            self.mounted
                .borrow()
                .keys()
                .map(|id| (id.clone(), self.catalog.0[id].contributions.clone()))
                .collect(),
            active,
        );
        *self.snapshot.borrow_mut() = PluginSnapshot {
            inventory_revision: inventory.revision,
            plugins: views.into_values().collect(),
        };
    }

    fn mount(&self, definition: &crate::CorePlugin) -> PluginStatus {
        let orbs = match (definition.create)(crate::PluginContext {
            host: self.host.clone(),
            index: self.catalog.index(),
        }) {
            Ok(orbs) if orbs.is_empty() => {
                return PluginStatus::Failed {
                    failure: MountFailure::EmptyPlugin,
                };
            }
            Ok(orbs) => orbs,
            Err(error) => {
                return PluginStatus::Failed {
                    failure: MountFailure::Factory {
                        message: error.to_string(),
                    },
                };
            }
        };
        let before: BTreeSet<_> = self
            .host
            .kernel()
            .installed_orbs()
            .iter()
            .map(|orb| orb.id)
            .collect();
        if let Err(error) = self.host.install(orbs) {
            return PluginStatus::Failed {
                failure: MountFailure::Installation {
                    message: error.to_string(),
                },
            };
        }
        let orbs = self
            .host
            .kernel()
            .installed_orbs()
            .into_iter()
            .filter(|orb| !before.contains(&orb.id))
            .map(|orb| OrbInfo {
                name: orb.declaration.name,
                api_version: orb.declaration.api_version,
            })
            .collect();
        PluginStatus::Mounted { orbs }
    }
}

fn enabled_and_mounted(view: &PluginView) -> bool {
    view.desired.as_ref().is_some_and(|desired| desired.enabled)
        && matches!(view.observed, PluginStatus::Mounted { .. })
}

fn validate_inventory(inventory: &Inventory) -> tembra::Result<()> {
    let mut ids = BTreeSet::new();
    if inventory
        .plugins
        .iter()
        .any(|record| !ids.insert(&record.id))
    {
        return Err(Error::new(
            Code::Invalid,
            Reason::InvalidInventory.as_str(),
            "duplicate plugin identities in storage",
        ));
    }
    Ok(())
}

struct MutationGuard<'a>(&'a Cell<bool>);

impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
