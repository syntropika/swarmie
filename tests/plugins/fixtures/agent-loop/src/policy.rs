//! Mandatory policy provider, independent from optional lifecycle hooks.

use std::{cell::RefCell, rc::Rc};

use swarmie_agent_contracts::{ToolAuthorization, ToolCall, ToolPolicy, ToolPolicyProvider};
use tembra::{core, prelude::*};

#[derive(Clone, Default)]
pub struct PolicyProbe {
    pub calls: Rc<RefCell<Vec<ToolCall>>>,
    pub gate: Rc<RefCell<Option<crate::gate::AgentGateRef>>>,
}

pub fn facet(allowed: bool, probe: PolicyProbe) -> Result<NativeFacet> {
    NativeFacet::new("fixture.agent-policy", vec![], move |_, setup| {
        let probe = probe.clone();
        setup.serve_with_options(
            ToolPolicy,
            move |_, _, _| {
                Ok(Policy {
                    allowed,
                    probe: probe.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Policy {
    allowed: bool,
    probe: PolicyProbe,
}

impl ToolPolicyProvider for Policy {
    async fn authorize(&self, call: ToolCall, _context: Context) -> Result<ToolAuthorization> {
        self.probe.calls.borrow_mut().push(call);
        let gate = self.probe.gate.borrow().clone();
        if let Some(gate) = gate {
            gate.await_gate().await?;
        }
        Ok(ToolAuthorization {
            allowed: self.allowed,
            reason: (!self.allowed).then(|| "fixture policy denied execution".into()),
        })
    }
}
