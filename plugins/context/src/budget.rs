use serde::{Deserialize, Serialize};
use std::rc::Rc;
use swarmie_providers::{CountPrecision, PreparationPolicy, PreparedDescription};
use tembra::Context;

pub const COMPACTION_REQUIRED: &str = "swarmie.context.compaction_required";
pub const INPUT_TOO_LARGE: &str = "swarmie.context.input_too_large";

pub fn compaction_required(error: &tembra::Error) -> bool {
    error.code == tembra::Code::Exhausted && error.reason == COMPACTION_REQUIRED
}

pub fn input_too_large(error: &tembra::Error) -> bool {
    error.code == tembra::Code::Exhausted && error.reason == INPUT_TOO_LARGE
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetDecision {
    Ready { budget: Budget },
    CompactionRequired { budget: Budget, fixed: Budget },
    InputTooLarge { budget: Budget, fixed: Budget },
}

impl BudgetDecision {
    pub fn budget(&self) -> &Budget {
        match self {
            Self::Ready { budget }
            | Self::CompactionRequired { budget, .. }
            | Self::InputTooLarge { budget, .. } => budget,
        }
    }
}

/// Arithmetic over the actual final preparation. The reserve is the output cap
/// selected by the adapter; model token limits and payload bytes are separate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub used: u64,
    pub context: u64,
    pub input_limit: u64,
    pub output_reserve: u64,
    pub headroom: u64,
    pub available: u64,
    pub excess: u64,
    pub precision: CountPrecision,
}

impl Budget {
    pub fn calculate(description: &PreparedDescription, headroom: u64) -> tembra::Result<Self> {
        description.validate()?;
        let reserved = description
            .output_reserve
            .checked_add(headroom)
            .ok_or_else(|| crate::invalid("context reservation overflow"))?;
        let context = description.model.context.tokens.get();
        let usable = context.checked_sub(reserved).ok_or_else(|| {
            crate::invalid("output reserve and headroom exceed the context window")
        })?;
        let input_limit = usable.min(description.model.input.tokens.get());
        let used = description.measurement.input_tokens;
        Ok(Self {
            used,
            context,
            input_limit,
            output_reserve: description.output_reserve,
            headroom,
            available: input_limit.saturating_sub(used),
            excess: used.saturating_sub(input_limit),
            precision: description.measurement.precision,
        })
    }

    pub fn requires_compaction(&self) -> bool {
        self.excess != 0
    }
}

type Observer = dyn Fn(&Context, &PreparedDescription, &BudgetDecision) -> tembra::Result<()>;

/// Pure budget admission before model dispatch intent is confirmed. The optional
/// receiver observes safe metrics only; it must not invoke models or tools.
#[derive(Clone, Default)]
pub struct BudgetPolicy {
    headroom: u64,
    observer: Option<Rc<Observer>>,
}

impl BudgetPolicy {
    pub fn new(headroom: u64) -> Self {
        Self {
            headroom,
            observer: None,
        }
    }

    pub fn observe(
        mut self,
        observer: impl Fn(&Context, &PreparedDescription, &BudgetDecision) -> tembra::Result<()>
        + 'static,
    ) -> Self {
        self.observer = Some(Rc::new(observer));
        self
    }

    /// A fixed floor is required before overflow can trigger automatic compaction.
    pub fn decision(
        &self,
        description: &PreparedDescription,
        fixed: Option<&PreparedDescription>,
    ) -> tembra::Result<BudgetDecision> {
        let budget = Budget::calculate(description, self.headroom)?;
        if !budget.requires_compaction() {
            return Ok(BudgetDecision::Ready { budget });
        }
        let fixed = fixed.ok_or_else(|| {
            tembra::Error::new(
                tembra::Code::Unavailable,
                "swarmie.context.fixed_budget",
                "the fixed request budget is unavailable",
            )
        })?;
        if fixed.model != description.model
            || fixed.output_reserve != description.output_reserve
            || fixed.output_reservation != description.output_reservation
            || fixed.streaming != description.streaming
            || fixed.measurement.method != description.measurement.method
            || fixed.measurement.precision != description.measurement.precision
        {
            return Err(crate::invalid(
                "fixed request uses another model or measurement route",
            ));
        }
        let fixed = Budget::calculate(fixed, self.headroom)?;
        Ok(if fixed.requires_compaction() {
            BudgetDecision::InputTooLarge { budget, fixed }
        } else {
            BudgetDecision::CompactionRequired { budget, fixed }
        })
    }
}

impl PreparationPolicy for BudgetPolicy {
    fn fixed_request(
        &self,
        request: &swarmie_providers::ModelRequest,
        description: &PreparedDescription,
    ) -> tembra::Result<Option<swarmie_providers::ModelRequest>> {
        if !Budget::calculate(description, self.headroom)?.requires_compaction() {
            return Ok(None);
        }
        let mut fixed = request.clone();
        // Inspection only: never dispatch this derivative or alter actual history.
        // Keep the complete latest user entry, its following tool chain, current
        // instructions and all tool definitions. The owner can retain a larger
        // fixed suffix at planning/activation when multiple inputs remain pending.
        if let Some(first) = fixed
            .context
            .messages
            .iter()
            .rposition(|message| matches!(message, swarmie_providers::Message::User { .. }))
        {
            fixed.context.messages.drain(..first);
            fixed.context.sources = fixed.context.sources.map(|sources| sources.at(0));
        } else {
            return Err(tembra::Error::new(
                tembra::Code::Unavailable,
                "swarmie.context.fixed_budget",
                "a fixed request without a real User anchor requires the owner's explicit selection budget",
            ));
        }
        Ok(Some(fixed))
    }

    fn check(
        &self,
        context: &Context,
        description: &PreparedDescription,
        fixed: Option<&PreparedDescription>,
    ) -> tembra::Result<()> {
        context.check()?;
        let decision = self.decision(description, fixed)?;
        if let Some(observe) = &self.observer {
            observe(context, description, &decision)?;
        }
        match decision {
            BudgetDecision::Ready { .. } => Ok(()),
            BudgetDecision::CompactionRequired { .. } => Err(tembra::Error::new(
                tembra::Code::Exhausted,
                COMPACTION_REQUIRED,
                "the prepared model request requires context compaction",
            )),
            BudgetDecision::InputTooLarge { .. } => Err(tembra::Error::new(
                tembra::Code::Exhausted,
                INPUT_TOO_LARGE,
                "the fixed input and current contributions exceed this model's usable budget",
            )),
        }
    }
}
