//! Deterministic test provider. Receipts model an external provider's independent state.
//! This fixture performs no network I/O and is not a production model implementation.

use crate::{
    Model, ModelMethod, ModelProgress, ModelRequest, ModelResponse, OperationId, Progress, Recovery,
};
use futures::{FutureExt, future::LocalBoxFuture};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};
use tembra::{Code, Context, Error, Orb, Service};

/// Each new operation consumes one step. Recovery consumes no steps.
#[derive(Clone, Debug)]
pub enum Step {
    Reply(ModelResponse),
    Stream {
        chunks: Vec<String>,
        response: ModelResponse,
    },
    LoseReply(ModelResponse),
    CancelReply(ModelResponse),
    Fail(Error),
}

#[derive(Clone, Debug)]
pub struct Call {
    pub operation: OperationId,
    pub request: ModelRequest,
    pub principal: String,
}

struct Receipt {
    call: Call,
    response: ModelResponse,
}

/// A strict response script with operation receipts and observable request history.
pub struct MockModel {
    steps: RefCell<VecDeque<Step>>,
    receipts: RefCell<BTreeMap<OperationId, Receipt>>,
    calls: RefCell<Vec<Call>>,
    recoveries: RefCell<Vec<OperationId>>,
}

impl MockModel {
    pub fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            steps: RefCell::new(steps.into_iter().collect()),
            receipts: RefCell::new(BTreeMap::new()),
            calls: RefCell::new(vec![]),
            recoveries: RefCell::new(vec![]),
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    pub fn recoveries(&self) -> Vec<OperationId> {
        self.recoveries.borrow().clone()
    }

    pub fn remaining(&self) -> usize {
        self.steps.borrow().len()
    }

    fn receipt(
        &self,
        context: &Context,
        operation: OperationId,
        request: &ModelRequest,
    ) -> tembra::Result<Option<ModelResponse>> {
        context.check()?;
        let receipts = self.receipts.borrow();
        let Some(receipt) = receipts.get(&operation) else {
            return Ok(None);
        };
        if receipt.call.principal != context.principal()? {
            return Err(Error::new(
                Code::Forbidden,
                "mock.receipt_owner",
                "receipt belongs to another principal",
            ));
        }
        if receipt.call.request != *request {
            return Err(Error::new(
                Code::Conflict,
                "mock.operation_input",
                "operation request changed",
            ));
        }
        Ok(Some(receipt.response.clone()))
    }
}

impl Model for MockModel {
    fn stream<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
        progress: &'a Progress,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            if let Some(response) = self.receipt(context, operation, request)? {
                return Ok(response);
            }
            progress.publish(
                context,
                ModelProgress {
                    operation,
                    content: String::new(),
                },
            );
            let chunks = match self.steps.borrow().front() {
                Some(Step::Stream { chunks, .. }) => chunks.clone(),
                _ => vec![],
            };
            let mut content = String::new();
            for chunk in chunks {
                context.check()?;
                content.push_str(&chunk);
                progress.publish(
                    context,
                    ModelProgress {
                        operation,
                        content: content.clone(),
                    },
                );
            }
            let response = self.complete(context, operation, request).await?;
            if response.message.content != content {
                progress.publish(
                    context,
                    ModelProgress {
                        operation,
                        content: response.message.content.clone(),
                    },
                );
            }
            Ok(response)
        }
        .boxed_local()
    }

    fn complete<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            if let Some(response) = self.receipt(context, operation, request)? {
                return Ok(response);
            }
            let call = Call {
                operation,
                request: request.clone(),
                principal: context.principal()?,
            };
            self.calls.borrow_mut().push(call.clone());
            let step = self.steps.borrow_mut().pop_front().ok_or_else(|| {
                Error::invalid("mock.script_exhausted", "unexpected model request")
            })?;
            let response = match &step {
                Step::Reply(response) | Step::LoseReply(response) | Step::CancelReply(response) => {
                    response.clone()
                }
                Step::Stream { response, .. } => response.clone(),
                Step::Fail(error) => return Err(error.clone()),
            };
            self.receipts.borrow_mut().insert(
                operation,
                Receipt {
                    call,
                    response: response.clone(),
                },
            );
            match step {
                Step::LoseReply(_) => Err(Error::new(
                    Code::Unavailable,
                    "mock.reply_lost",
                    "completion recorded but reply lost",
                )),
                Step::CancelReply(_) => {
                    context.cancel()?;
                    Err(Error::new(
                        Code::Cancelled,
                        "mock.cancelled",
                        "completion recorded before cancellation",
                    ))
                }
                _ => Ok(response),
            }
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        async move {
            self.recoveries.borrow_mut().push(operation);
            Ok(self
                .receipt(context, operation, request)?
                .map(Recovery::Completed)
                .unwrap_or(Recovery::SafeToRetry))
        }
        .boxed_local()
    }
}

/// Registers the mock behind the same native orb contract used by other providers.
pub fn orb(name: impl Into<String>, model: Rc<MockModel>) -> Orb {
    let name = name.into();
    Orb::new(name.clone(), 1).service(
        Service::new(name, move |d| Ok((model.clone(), Progress::bind(&d)?)))
            .method(
                ModelMethod::Complete.as_str(),
                |model, ctx, (operation, request): (OperationId, ModelRequest)| async move {
                    model.0.complete(&ctx, operation, &request).await
                },
            )
            .method(
                ModelMethod::Recover.as_str(),
                |model, ctx, (operation, request): (OperationId, ModelRequest)| async move {
                    model.0.recover(&ctx, operation, &request).await
                },
            )
            .method(
                ModelMethod::Stream.as_str(),
                |model, ctx, (operation, request): (OperationId, ModelRequest)| async move {
                    model.0.stream(&ctx, operation, &request, &model.1).await
                },
            ),
    )
}
