use std::convert::Infallible;

use super::Whole;
use crate::codecs::embeddings::Embedding as Vector;
use crate::codecs::error::ProviderError;
use crate::codecs::telemetry::{GenAiOperation, SpanBuilder, SpanCombinator};
use crate::codecs::wire::{Call, Capabilities, Fold, Free, Operation, Reply};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RerankRequest {
    pub query: String,
    pub documents: Vec<String>,
}

macro_rules! modality_operation {
    (
        $(#[$doc:meta])*
        $op:ident {
            request: $request:ty,
            response: $response:ty,
            telemetry: $telemetry:ident,
            fold: $fold:ty,
            seed: $seed:expr,
        }
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $op;

        impl Operation for $op {
            type Request = $request;
            type Event = Infallible;
            type End = $response;
            type Response = $response;
            type Fold = Traced<Self, $fold>;
            type Emit = Free;

            fn fold(request: &Self::Request, call: &mut Call<'_>) -> Self::Fold {
                let telemetry = call
                    .wire
                    .telemetry
                    .map_or(GenAiOperation::$telemetry, |telemetry| telemetry(call.mode));
                debug_assert!(!telemetry.is_completion());
                let span = SpanBuilder::new(
                    call.wire.name,
                    call.wire.model.unwrap_or_default(),
                    telemetry,
                )
                .build();
                call.instrument(span.clone());
                #[allow(clippy::redundant_closure_call)]
                let inner = ($seed)(request, &*call);
                Traced {
                    inner,
                    span,
                    record: |span, response| {
                        span.record_response(
                            response.response_id.as_deref(),
                            response.model.as_deref(),
                            &response.usage,
                        )
                    },
                }
            }
        }
    };
}

pub struct Traced<Op: Operation, F> {
    inner: F,
    span: tracing::Span,
    record: fn(&tracing::Span, &Op::Response),
}

impl<Op, F> Fold<Op> for Traced<Op, F>
where
    Op: Operation,
    F: Fold<Op>,
{
    fn absorb(&mut self, event: &Op::Event) -> Result<(), ProviderError> {
        self.inner.absorb(event)
    }

    fn finish(self, end: Op::End, reply: Reply) -> Result<Op::Response, ProviderError> {
        let response = self.inner.finish(end, reply)?;
        (self.record)(&self.span, &response);
        Ok(response)
    }
}

pub(crate) trait Stamp {
    fn stamp(&mut self, reply: &Reply);
}

macro_rules! stamp {
    ($($response:ty),*) => {$(
        impl Stamp for $response {
            fn stamp(&mut self, reply: &Reply) {
                use crate::codecs::provider_response::reported;
                self.provider.clone_from(&reply.provider);
                self.provider_request_id = reported(reply.provider_request_id.clone());
                self.raw.clone_from(&reply.raw);
                self.model = reported(self.model.take());
                self.response_id = reported(self.response_id.take());
            }
        }
    )*};
}

stamp!(
    crate::codecs::embeddings::EmbeddingResponse,
    crate::codecs::rerank::RerankResponse,
    crate::codecs::transcription::TranscriptionResponse
);

modality_operation!(
    Embedding {
        request: Vec<String>,
        response: crate::codecs::embeddings::EmbeddingResponse,
        telemetry: Embeddings,
        fold: Embedded,
        seed: |texts: &Vec<String>, call: &Call<'_>| Embedded::over(texts.clone(), call),
    }
);

modality_operation!(
    ImageEmbedding {
        request: Vec<Vec<u8>>,
        response: crate::codecs::embeddings::EmbeddingResponse,
        telemetry: Embeddings,
        fold: Embedded,
        seed: |images: &Vec<Vec<u8>>, call: &Call<'_>| Embedded::over(
            images.iter().map(|bytes| crate::codecs::embeddings::image_document(bytes)).collect(),
            call,
        ),
    }
);

modality_operation!(
    Rerank {
        request: RerankRequest,
        response: crate::codecs::rerank::RerankResponse,
        telemetry: Rerank,
        fold: Whole<Self>,
        seed: |_: &_, _: &Call<'_>| Whole::stamping(<crate::codecs::rerank::RerankResponse as Stamp>::stamp),
    }
);

modality_operation!(
    Transcription {
        request: crate::codecs::transcription::TranscriptionRequest,
        response: crate::codecs::transcription::TranscriptionResponse,
        telemetry: Transcription,
        fold: Whole<Self>,
        seed: |_: &_, _: &Call<'_>| Whole::stamping(<crate::codecs::transcription::TranscriptionResponse as Stamp>::stamp),
    }
);

pub struct Embedded {
    documents: Vec<String>,
    provider: String,
    capabilities: Capabilities,
}

impl Embedded {
    pub fn over(documents: Vec<String>, call: &Call<'_>) -> Self {
        Self {
            documents,
            provider: call.wire.name.to_owned(),
            capabilities: call.wire.capabilities,
        }
    }

    fn zipped(self, vectors: Vec<Vector>) -> Result<Vec<Vector>, ProviderError> {
        self.capabilities.honour_declaration(
            &self.provider,
            vectors.iter().map(|vector| vector.vec.len()),
        )?;
        if vectors.len() != self.documents.len() {
            return Err(ProviderError::Response(format!(
                "provider returned {} embeddings for {} documents",
                vectors.len(),
                self.documents.len()
            )));
        }
        Ok(self
            .documents
            .into_iter()
            .zip(vectors)
            .map(|(document, vector)| Vector {
                document,
                vec: vector.vec,
            })
            .collect())
    }
}

impl<Op> Fold<Op> for Embedded
where
    Op: Operation<
            Event = Infallible,
            End = crate::codecs::embeddings::EmbeddingResponse,
            Response = crate::codecs::embeddings::EmbeddingResponse,
        >,
{
    fn absorb(&mut self, event: &Infallible) -> Result<(), ProviderError> {
        match *event {}
    }

    fn finish(
        self,
        mut response: crate::codecs::embeddings::EmbeddingResponse,
        reply: Reply,
    ) -> Result<crate::codecs::embeddings::EmbeddingResponse, ProviderError> {
        response.embeddings = self.zipped(std::mem::take(&mut response.embeddings))?;
        response.stamp(&reply);
        Ok(response)
    }
}
