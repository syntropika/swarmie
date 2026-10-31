use super::{Accounts, CredentialRequirement, failure};
use crate::settings::{
    Account, ConfigureProvider, ModelDefaults, ProviderCatalog, ProviderSettings, SetModelDefaults,
};
use futures::future::LocalBoxFuture;
use sha2::{Digest, Sha256};

impl ProviderSettings for Accounts {
    fn set_defaults(
        &self,
        principal: String,
        input: SetModelDefaults,
    ) -> LocalBoxFuture<'_, tembra::Result<ModelDefaults>> {
        Box::pin(async move { self.save_defaults(&principal, input).await })
    }

    fn configure(
        &self,
        principal: String,
        input: ConfigureProvider,
    ) -> LocalBoxFuture<'_, tembra::Result<Account>> {
        Box::pin(async move {
            self.authorize(&principal)?;
            let _mutation = self.mutation.lock().await;
            let state = self.ready_state().await?;
            let provider = input.provider_id.clone();
            let record = super::recovery::RecoveryRecord::new(
                uuid::Uuid::new_v4(),
                state.revision(),
                super::recovery::RecoveryKind::Configure,
                Some(self.driver.protocol_binding()?),
                super::recovery::RecoveryAccount::Unassociated {
                    provider: provider.clone(),
                    returning: None,
                },
                super::recovery::RecoveryReferences::default(),
            )?;
            let state = self.checkpoint(&state, record).await?;
            let candidate = self.driver.configure(input).await?;
            if candidate.record.account.provider_id != provider {
                return Err(failure(
                    "the provider returned another account configuration",
                ));
            }
            let account = self.candidate(state, candidate).await?;
            Ok(first_chunk(account))
        })
    }

    fn list(
        &self,
        principal: String,
        cursor: Option<String>,
    ) -> LocalBoxFuture<'_, tembra::Result<ProviderCatalog>> {
        Box::pin(async move {
            self.authorize(&principal)?;
            let state = self.state().await?;
            let active = state.workflow().is_some();
            let records = state.accounts().to_vec();
            let revision = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&records)
                        .map_err(|_| failure("provider settings could not be encoded"))?
                )
            );
            let offset = match cursor {
                None => 0,
                Some(cursor) => {
                    let (version, offset) = cursor
                        .split_once(':')
                        .ok_or_else(|| failure("invalid provider catalog cursor"))?;
                    if version != revision {
                        return Err(tembra::Error::new(
                            tembra::Code::Conflict,
                            "providers.catalog_changed",
                            "the provider catalog changed; load it again",
                        ));
                    }
                    offset
                        .parse::<usize>()
                        .map_err(|_| failure("invalid provider catalog cursor"))?
                }
            };
            let mut chunks = Vec::new();
            for record in records {
                let available = !active
                    && match self.driver.credential_requirement(&record)? {
                        CredentialRequirement::None => true,
                        CredentialRequirement::Account => {
                            if let Some(reference) = state.credentials().get(&record.account.id) {
                                matches!(
                                    self.credentials.read(reference.clone()).await?,
                                    super::recovery::CredentialRead::Complete(_)
                                )
                            } else {
                                false
                            }
                        }
                    };
                let mut account = record.account;
                account.available = available;
                chunks.extend(account_chunks(account));
            }
            if offset > chunks.len() {
                return Err(failure("invalid provider catalog cursor"));
            }
            let mut page = ProviderCatalog {
                choices: crate::settings::provider_catalog(),
                accounts: vec![],
                next_cursor: None,
            };
            for account in chunks.iter().skip(offset).cloned() {
                page.accounts.push(account);
                if serde_json::to_vec(&page)
                    .map_err(|_| failure("provider catalog could not be encoded"))?
                    .len()
                    > 40 * 1024
                {
                    page.accounts.pop();
                    break;
                }
            }
            let next = offset + page.accounts.len();
            if next < chunks.len() {
                page.next_cursor = Some(format!("{revision}:{next}"));
            }
            Ok(page)
        })
    }
}

fn account_chunks(mut account: Account) -> Vec<Account> {
    let models = std::mem::take(&mut account.models);
    let mut chunks = vec![];
    let mut chunk = account;
    let base = serde_json::to_vec(&chunk).map_or(4096, |bytes| bytes.len()) + 64;
    let mut size = base;
    for model in models {
        let added = serde_json::to_vec(&model).map_or(2048, |bytes| bytes.len()) + 1;
        if !chunk.models.is_empty() && (chunk.models.len() >= 256 || size + added > 20 * 1024) {
            let models = std::mem::take(&mut chunk.models);
            chunks.push(Account {
                models,
                models_truncated: true,
                ..chunk.clone()
            });
            size = base;
        }
        size += added;
        chunk.models.push(model);
    }
    chunk.models_truncated = !chunks.is_empty();
    chunks.push(chunk);
    chunks
}

pub(super) fn first_chunk(account: Account) -> Account {
    account_chunks(account)
        .into_iter()
        .next()
        .expect("each account has a chunk")
}
