// This binary selects the publication subset of the shared HTTP acceptance fixtures.
#[allow(dead_code)]
mod support;

use std::{cell::Cell, rc::Rc, sync::atomic::Ordering};
use support::*;
use swarmie_model_contracts::*;
use tembra::{Readable, RuntimeError};

#[tembra::contract(local, name = "fixture.model-publication-projection", version = 1)]
trait Projection {
    #[state]
    fn accepted(&self) -> bool;
}
struct ProjectionService(Readable<bool>);
impl ProjectionProvider for ProjectionService {
    fn accepted(&self) -> Readable<bool> {
        self.0.clone()
    }
}

fn stream() -> Reply {
    Reply::Frames(vec![
        (
            0,
            frame(serde_json::json!({"choices":[{"delta":{"content":"first"}}]})),
        ),
        (
            30,
            frame(serde_json::json!({"choices":[{"delta":{"content":" second"}}]})),
        ),
        (
            0,
            frame(serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]})),
        ),
        (0, done()),
    ])
}

#[test]
fn subscriber_and_external_projection_errors_do_not_reject_source_or_rerun_http() {
    let server = Server::new(vec![stream()]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let generation = generation(&host, request("observer-errors")).await;
        let source = generation.progress();
        host.install(
            tembra::NativeFacet::new("projection-provider", vec![], move |_, setup| {
                let source = source.clone();
                let accepted_definition = setup.state("accepted")?;
                setup.serve(Projection, move |_, _, declaration| {
                    let accepted = declaration.derive(
                        &accepted_definition,
                        (source.clone(),),
                        |(state,)| {
                            if !state.events.is_empty() || state.physically_settled {
                                return Err(RuntimeError::invalid(
                                    "fixture_projection",
                                    "projection refused",
                                ));
                            }
                            Ok(true)
                        },
                    )?;
                    Ok(ProjectionService(accepted))
                })
            })
            .unwrap(),
            Default::default(),
        )
        .unwrap()
        .await
        .unwrap();
        let projection = host.get(Projection, ProjectionInput {}).await.unwrap();
        let rejected = generation
            .progress()
            .subscribe(|snapshot| {
                if snapshot.value()?.phase != GenerationPhase::Prepared {
                    return Err(RuntimeError::invalid(
                        "fixture_subscriber",
                        "subscriber refused",
                    ));
                }
                Ok(())
            })
            .unwrap();
        let delivered = Rc::new(Cell::new(0));
        let observations = delivered.clone();
        let healthy = generation
            .progress()
            .subscribe(move |_| {
                observations.set(observations.get() + 1);
                Ok(())
            })
            .unwrap();
        generation.start().await.unwrap();
        let terminal = settled(&generation).await;
        assert_eq!(generation.recover().await.unwrap(), terminal);
        assert_eq!(
            terminal.response.unwrap().message,
            Message::text(Role::Assistant, "first second")
        );
        assert!(projection.accepted().value().unwrap());
        assert!(delivered.get() >= 4);
        let errors = host.core().errors();
        assert!(errors.iter().any(|e| e.reason == "fixture_subscriber"));
        assert!(errors.iter().any(|e| e.reason == "fixture_projection"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        rejected.unsubscribe();
        healthy.unsubscribe();
        host.close().await.unwrap();
    });
}

#[test]
fn native_observer_panic_after_adoption_preserves_sequences_and_terminal_recovery() {
    let server = Server::new(vec![stream()]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let generation = generation(&host, request("observer-panic")).await;
        let panics = Rc::new(Cell::new(0));
        let observed = panics.clone();
        let subscription = generation
            .progress()
            .subscribe(move |snapshot| {
                let state = snapshot.value()?;
                if state.phase != GenerationPhase::Prepared {
                    observed.set(observed.get() + 1);
                    panic!("fixture observer panic after adoption");
                }
                Ok(())
            })
            .unwrap();
        generation.start().await.unwrap();
        wait_for(|| {
            generation
                .progress()
                .value()
                .is_ok_and(|s| s.physically_settled)
        })
        .await;
        let terminal = generation.recover().await.unwrap();
        assert!(terminal.error.is_none());
        assert_eq!(terminal.last_sequence, 2);
        assert_eq!(
            terminal
                .events
                .iter()
                .map(|e| e.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(panics.get() >= 3);
        generation.acknowledge(2).await.unwrap();
        assert!(generation.recover().await.unwrap().events.is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        subscription.unsubscribe();
        host.close().await.unwrap();
        assert!(generation.recover().await.is_err());
    });
}

#[test]
fn concurrent_consumer_cancel_and_host_shutdown_physically_close_stream() {
    let server = Server::new(vec![Reply::Hold]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let generation = generation(&host, request("close-race")).await;
        generation.start().await.unwrap();
        wait_for(|| !server.requests.lock().unwrap().is_empty()).await;
        let (cancelled, closed) = futures::join!(generation.cancel(), host.close());
        // Shutdown can revoke the invocation before cancellation dispatch; cleanup still joins.
        let _ = cancelled;
        closed.unwrap();
        wait_for(|| server.disconnected.load(Ordering::SeqCst)).await;
        assert!(generation.recover().await.is_err());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    });
}
