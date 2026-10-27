use crate::{Connection, ConnectionStatus, Observation, ServerBinding, error::busy, unavailable};
use futures::lock::Mutex;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use swarmie_tools::{Registration, ToolProvider};
use tembra::Context;

/// Admission and catalog leases inside one Tembra-owned resource. Creating this
/// value never opens a process or a transport, including during recovery.
pub(crate) struct Mounted {
    binding: ServerBinding,
    connection: RefCell<Option<Rc<Connection>>>,
    registrations: RefCell<Vec<Registration>>,
    disabled: Cell<bool>,
    connecting: Mutex<()>,
    provider: ToolProvider,
    connect_guard: Option<Rc<dyn Fn() -> tembra::Result<()>>>,
}

impl Mounted {
    pub fn new(binding: ServerBinding) -> Self {
        let provider = ToolProvider {
            service: crate::SERVICE.into(),
            key: vec![binding.configuration.id.as_str().into()],
        };
        Self::bound(binding, provider, None)
    }

    pub fn bound(
        binding: ServerBinding,
        provider: ToolProvider,
        connect_guard: Option<Rc<dyn Fn() -> tembra::Result<()>>>,
    ) -> Self {
        Self {
            binding,
            connection: RefCell::new(None),
            registrations: RefCell::new(vec![]),
            disabled: Cell::new(false),
            connecting: Mutex::new(()),
            provider,
            connect_guard,
        }
    }

    pub fn connection(&self) -> Option<Rc<Connection>> {
        self.connection.borrow().clone()
    }

    pub fn observation(&self) -> Observation {
        if let Some(connection) = self.connection() {
            return connection.observation();
        }
        Observation {
            status: if self.disabled.get() {
                ConnectionStatus::Disabled
            } else if self.connecting.try_lock().is_none() {
                ConnectionStatus::Connecting
            } else {
                ConnectionStatus::Disconnected
            },
            protocol: None,
            server: None,
            tools: vec![],
        }
    }

    /// An explicit administration action, separate from provider resolution.
    pub async fn connect(&self, context: &Context) -> tembra::Result<Observation> {
        context.check()?;
        if let Some(guard) = &self.connect_guard {
            guard()?;
        }
        let _connecting = self.connecting.try_lock().ok_or_else(busy)?;
        self.disabled.set(false);
        let existing = self.connection();
        let fresh = existing.is_none();
        let connection = match existing {
            Some(connection) => connection,
            None => Rc::new(
                Connection::connect_bound(
                    self.binding.configuration.clone(),
                    context,
                    self.provider.clone(),
                )
                .await?,
            ),
        };
        let registered = (|| {
            context.check()?;
            if let Some(guard) = &self.connect_guard {
                guard()?;
            }
            if self.disabled.get() {
                return Err(unavailable("MCP was disabled while connecting"));
            }
            connection.enable()?;
            if !self.registrations.borrow().is_empty() {
                return Ok(None);
            }
            connection
                .definitions()
                .into_iter()
                .map(|tool| self.binding.catalog.register(tool))
                .collect::<tembra::Result<Vec<_>>>()
                .map(Some)
        })();
        match registered {
            Ok(registrations) => {
                if let Some(registrations) = registrations {
                    *self.registrations.borrow_mut() = registrations;
                }
                *self.connection.borrow_mut() = Some(connection.clone());
                Ok(connection.observation())
            }
            Err(error) => {
                connection.disable();
                if fresh {
                    let _cleanup = connection.close().await;
                }
                Err(error)
            }
        }
    }

    pub fn disable(&self) {
        self.disabled.set(true);
        if let Some(connection) = self.connection() {
            connection.disable();
        }
        self.registrations.borrow_mut().clear();
    }

    pub fn withdraw_unavailable(&self) {
        if self.connection().is_some_and(|connection| {
            connection.observation().status == ConnectionStatus::Unavailable
        }) {
            self.registrations.borrow_mut().clear();
        }
    }

    pub async fn close(&self) -> tembra::Result<()> {
        self.disable();
        if let Some(connection) = self.connection() {
            connection.close().await?;
        }
        Ok(())
    }
}
