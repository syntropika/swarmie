//! Byte-bounded private projections; large inventories remain navigable over the same reference.
use super::*;

const SECTION_BYTES: usize = 32_000;
const SECTION_ITEMS: usize = 20;

impl Control {
    pub(super) fn page(
        &self,
        key: &str,
        values: Vec<Value>,
        pages: &mut Value,
    ) -> tembra::Result<Value> {
        let total = values.len();
        let offset = self
            .paging
            .borrow()
            .get(key)
            .copied()
            .unwrap_or(0)
            .min(total.saturating_sub(1));
        let mut used = 2;
        let mut page = vec![];
        for value in values.into_iter().skip(offset).take(SECTION_ITEMS) {
            let length = serde_json::to_vec(&value).map_err(failure)?.len() + 1;
            if !page.is_empty() && used + length > SECTION_BYTES {
                break;
            }
            if length > 80_000 {
                return Err(unavailable());
            }
            used += length;
            page.push(value);
        }
        let end = offset + page.len();
        pages[key] = json!({"offset":offset,"total":total,"next":if end<total {Some(end)}else{None},"previous":if offset>0{Some(offset.saturating_sub(SECTION_ITEMS))}else{None}});
        Ok(Value::Array(page))
    }
    pub(super) fn select_page(&self, key: String, offset: usize) -> tembra::Result<()> {
        if ![
            "entries",
            "captures",
            "mutations",
            "rejected",
            "cards",
            "offers",
            "devices",
        ]
        .contains(&key.as_str())
            || offset > 32_768
        {
            return Err(unavailable());
        }
        self.paging.borrow_mut().insert(key, offset);
        Ok(())
    }
}

impl Control {
    pub(super) fn mutation_view(
        &self,
        active: &Active,
        mutation: MutationIntent,
    ) -> tembra::Result<Value> {
        let latest = active
            .node
            .current_revision(&active.session, &self.grant, mutation.reference)
            .map_err(failure)?;
        let mut value = serde_json::to_value(mutation).map_err(failure)?;
        value["latest"] = json!(latest);
        Ok(value)
    }
}

impl Control {
    pub(super) fn project(&self) -> tembra::Result<Value> {
        let active = self.active.borrow().clone();
        let catalog = self.catalog.borrow();
        let current = catalog.as_ref().and_then(|catalog| catalog.current);
        let mut view = json!({"phase":if self.account.borrow().as_ref().is_some_and(AccountSession::is_live) {"locked"}else{"signed-out"},
            "entries":[],"pendingFoundings":self.configuration.borrow().roots.iter().filter_map(|root|root.pending.as_ref().map(|receipt|json!({"vault":root.identity.vault,"operation":receipt.operation}))).collect::<Vec<_>>(),"cards":[],"synchronization":"offline","archived":false,"exposed":false,
            "cloud":self.configuration.borrow().endpoints,"current":current,
            "canFound":self.account.borrow().as_ref().is_some_and(AccountSession::is_live) && catalog.is_some() && self.configuration.borrow().roots.iter().all(|root|root.pending.is_none()),
            "captures":[],"mutations":[],"rejected":[],"authority":null,"error":null});
        view["needsSignIn"] = json!(self.needs_sign_in.get());
        view["signingIn"] = json!(self.logging_in.get());
        view["lifecycle"] = self.lifecycle_view()?;
        let mut pages = json!({});
        let cards = self.cards.views();
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for card in &cards {
            let conversation = card
                .get("conversation")
                .and_then(Value::as_str)
                .unwrap_or_default();
            *counts.entry(conversation.to_owned()).or_default() += 1;
        }
        view["cardCounts"] = json!(counts);
        view["cards"] = self.page("cards", cards, &mut pages)?;
        if let Some(active) = active.filter(|active| active.attachment.check().is_ok()) {
            view["phase"] = "ready".into();
            view["synchronization"] = if active.connected.get() {
                "confirmed"
            } else {
                "offline"
            }
            .into();
            view["archived"] = json!(current.is_some_and(|id| id != active.node.identity().vault));
            view["exposed"] = json!(catalog.as_ref().is_some_and(|catalog| {
                catalog
                    .roots
                    .iter()
                    .any(|root| root.identity == active.node.identity() && root.exposed)
            }));
            view["entries"]=self.page("entries",active.node.list(&active.session,&self.grant).map_err(failure)?.into_iter().map(|record|json!({
                "reference":SecretReference { account:record.identity.account,vault:record.identity.vault,secret:record.secret }.to_string(),
                "revision":record.revision,"metadata":record.metadata })).collect(),&mut pages)?;
            view["captures"] = self.page(
                "captures",
                active
                    .node
                    .capture_summaries(&active.session, &self.grant)
                    .map_err(failure)?
                    .into_iter()
                    .map(|value| serde_json::to_value(value).map_err(failure))
                    .collect::<tembra::Result<_>>()?,
                &mut pages,
            )?;
            view["authority"]=active.node.authority_summary(&active.session,&self.grant).map_err(failure)?.map_or(Value::Null,|authority|json!({"identity":authority.identity,"sequence":authority.sequence,"devices":authority.devices}));
            view["rejected"] = self.page(
                "rejected",
                active
                    .node
                    .rejected_mutations(&active.session, &self.grant)
                    .map_err(failure)?
                    .into_iter()
                    .map(|(intent, outcome)| {
                        self.mutation_view(&active, intent)
                            .map(|intent| json!([intent, outcome]))
                    })
                    .collect::<tembra::Result<_>>()?,
                &mut pages,
            )?;
            view["mutations"] = self.page(
                "mutations",
                active
                    .node
                    .pending_mutations(&active.session, &self.grant)
                    .map_err(failure)?
                    .into_iter()
                    .map(|intent| self.mutation_view(&active, intent))
                    .collect::<tembra::Result<_>>()?,
                &mut pages,
            )?;
        }
        view["pages"] = pages;
        if serde_json::to_vec(&view).map_err(failure)?.len() > 360_000 {
            return Err(unavailable());
        }
        Ok(view)
    }
}
