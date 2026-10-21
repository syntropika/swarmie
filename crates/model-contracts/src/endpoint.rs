//! Independently named endpoints share exact canonical schemas, not a singleton provider.

use crate::*;
use std::rc::Rc;
use tembra::{
    Context, ContractToken, ProviderContract, Readable, Result, RuntimeError, contracts::Contract,
    core,
};

fn named(canonical: Rc<Contract>, name: &str) -> Result<Rc<Contract>> {
    let mut descriptor = canonical.descriptor().clone();
    descriptor.name = name.to_owned();
    descriptor.fingerprint.clear();
    Contract::from_descriptor(descriptor)
        .map(Rc::new)
        .map_err(|_| RuntimeError::invalid("model_contract", "invalid model endpoint identity"))
}

fn check(reference: &core::Reference, descriptor: Rc<Contract>) -> Result<()> {
    if reference.contract().fingerprint() != descriptor.fingerprint() {
        return Err(RuntimeError::invalid(
            "model_contract",
            "model endpoint schema mismatch",
        ));
    }
    Ok(())
}

/// Named provider token. Obtain its identity from the lifecycle-owned Registry metadata.
#[derive(Clone)]
pub struct ModelEndpoint(Rc<Contract>);

impl ModelEndpoint {
    pub fn new(name: &str) -> Result<Self> {
        named(ModelProviderSchema.descriptor()?, name).map(Self)
    }
}

impl ContractToken for ModelEndpoint {
    type Input = ModelProviderSchemaInput;
    type Reference = ModelReference;
    fn descriptor(&self) -> Result<Rc<Contract>> {
        Ok(self.0.clone())
    }
    fn bind(&self, reference: core::Reference) -> Result<Self::Reference> {
        check(&reference, self.descriptor()?)?;
        Ok(ModelReference {
            raw: reference,
            context: None,
        })
    }
}

impl<P: ModelProviderSchemaProvider + 'static> ProviderContract<P> for ModelEndpoint {
    fn implementation(&self, provider: P) -> Result<core::Implementation> {
        ModelProviderSchema.implementation(provider)
    }
}

#[derive(Clone)]
pub struct ModelReference {
    raw: core::Reference,
    context: Option<core::Context>,
}

impl ModelReference {
    pub fn core(&self) -> &core::Reference {
        &self.raw
    }
    pub fn with_context(&self, context: &Context) -> Self {
        Self {
            raw: self.raw.clone(),
            context: Some(context.core().clone()),
        }
    }
    pub async fn capabilities(&self, model: ModelIdentity) -> Result<Capabilities> {
        invoke(
            &self.raw,
            "capabilities",
            Some(tembra::serde_json::json!(model)),
            self.context.as_ref(),
        )
        .await
    }
    pub async fn models(&self, connection: String) -> Result<ModelList> {
        invoke(
            &self.raw,
            "models",
            Some(tembra::serde_json::json!(connection)),
            self.context.as_ref(),
        )
        .await
    }
    pub async fn generate(&self, request: ModelRequest) -> Result<GenerateResult> {
        invoke(
            &self.raw,
            "generate",
            Some(tembra::serde_json::json!(request)),
            self.context.as_ref(),
        )
        .await
    }
}

/// Token for the provider's referenced generation objects. No provider library enters this API.
#[derive(Clone)]
pub struct GenerationEndpoint(Rc<Contract>);

impl GenerationEndpoint {
    pub fn new(name: &str) -> Result<Self> {
        named(ModelGenerationSchema.descriptor()?, name).map(Self)
    }
}

impl ContractToken for GenerationEndpoint {
    type Input = GenerationInput;
    type Reference = GenerationReference;
    fn descriptor(&self) -> Result<Rc<Contract>> {
        Ok(self.0.clone())
    }
    fn bind(&self, reference: core::Reference) -> Result<Self::Reference> {
        check(&reference, self.descriptor()?)?;
        Ok(GenerationReference {
            raw: reference,
            context: None,
        })
    }
}

impl<P: ModelGenerationSchemaProvider + 'static> ProviderContract<P> for GenerationEndpoint {
    fn implementation(&self, provider: P) -> Result<core::Implementation> {
        ModelGenerationSchema.implementation(provider)
    }
}

#[derive(Clone)]
pub struct GenerationReference {
    raw: core::Reference,
    context: Option<core::Context>,
}

impl GenerationReference {
    pub fn core(&self) -> &core::Reference {
        &self.raw
    }
    pub fn with_context(&self, context: &Context) -> Self {
        Self {
            raw: self.raw.clone(),
            context: Some(context.core().clone()),
        }
    }
    pub fn progress(&self) -> Readable<GenerationState> {
        Readable::from_reference(self.raw.clone(), "progress")
    }
    pub async fn start(&self) -> Result<()> {
        self.raw
            .call_json("start", None, self.context.as_ref())
            .await
            .map(|_| ())
    }
    pub async fn acknowledge(&self, through: u64) -> Result<()> {
        self.raw
            .call_json(
                "acknowledge",
                Some(tembra::serde_json::json!(through)),
                self.context.as_ref(),
            )
            .await
            .map(|_| ())
    }
    pub async fn cancel(&self) -> Result<()> {
        self.raw
            .call_json("cancel", None, self.context.as_ref())
            .await
            .map(|_| ())
    }
}

async fn invoke<T: tembra::serde::de::DeserializeOwned>(
    raw: &core::Reference,
    method: &str,
    args: Option<tembra::serde_json::Value>,
    context: Option<&core::Context>,
) -> Result<T> {
    let value = raw
        .call_json(method, args, context)
        .await?
        .ok_or_else(|| RuntimeError::invalid("model_result", "missing model result"))?;
    tembra::contracts::from_portable_value(&value)
        .map_err(|_| RuntimeError::invalid("model_result", "invalid model result"))
}
