use std::rc::Rc;

#[cfg(any(feature = "web", feature = "mobile", test))]
use std::future::Future;
#[cfg(any(feature = "web", feature = "mobile", test))]
use std::pin::Pin;
#[cfg(any(feature = "web", feature = "mobile", test))]
use std::time::Duration;

#[cfg(any(feature = "web", feature = "mobile", test))]
use dioxus::prelude::WritableExt;
#[cfg(any(feature = "web", feature = "mobile"))]
use dioxus::prelude::{ReadableExt, spawn};
#[cfg(any(feature = "web", feature = "mobile", test))]
use dioxus_sdk_time::sleep;

#[cfg(any(feature = "web", feature = "mobile", test))]
use crate::model::UiNotice;
use crate::model::{AppPorts, EditorStore};
#[cfg(any(feature = "web", feature = "mobile", test))]
use crate::protocol::AppErrorDto;

#[cfg(any(feature = "web", feature = "mobile", test))]
use super::document_identity;
#[cfg(any(feature = "web", feature = "mobile"))]
use super::document_name;

#[cfg(any(feature = "web", feature = "mobile"))]
const RECOVERY_DEBOUNCE: Duration = Duration::from_secs(2);
#[cfg(any(feature = "web", feature = "mobile"))]
const RECOVERY_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

#[cfg(any(feature = "web", feature = "mobile", test))]
type RecoveryFuture = Pin<Box<dyn Future<Output = RecoveryAttempt> + 'static>>;

#[cfg(any(feature = "web", feature = "mobile", test))]
#[derive(Clone, Copy)]
struct RecoveryTarget {
    generation: u64,
    identity: Option<(u64, u64)>,
}

#[cfg(any(feature = "web", feature = "mobile", test))]
#[derive(Clone, Copy)]
enum RecoveryFailureKind {
    Checkpoint,
    Cleanup,
}

#[cfg(any(feature = "web", feature = "mobile", test))]
enum RecoveryAttempt {
    Succeeded,
    Stale,
    Failed {
        kind: RecoveryFailureKind,
        error: AppErrorDto,
    },
}

#[cfg(any(feature = "web", feature = "mobile", test))]
trait RecoveryBackend {
    fn attempt(&self, store: EditorStore, target: RecoveryTarget) -> RecoveryFuture;
}

pub(super) fn schedule(store: EditorStore, ports: Rc<AppPorts>) {
    #[cfg(feature = "web")]
    schedule_backend(store, Rc::new(WebRecoveryBackend { ports }));

    #[cfg(feature = "mobile")]
    schedule_backend(store, Rc::new(MobileRecoveryBackend { ports }));

    #[cfg(not(any(feature = "web", feature = "mobile")))]
    let _ = (store, ports);
}

pub(super) fn mark_healthy(store: EditorStore) {
    store.mark_recovery_healthy();
}

#[cfg(feature = "mobile")]
pub(super) fn schedule_cleanup(store: EditorStore, ports: Rc<AppPorts>) {
    let target = next_target(store, false);
    spawn(run_recovery(
        store,
        target,
        Rc::new(MobileRecoveryCleanupBackend { ports }),
        &RECOVERY_RETRY_DELAYS,
    ));
}

#[cfg(any(feature = "web", feature = "mobile"))]
fn schedule_backend(store: EditorStore, backend: Rc<dyn RecoveryBackend>) {
    let target = next_target(store, true);
    spawn(async move {
        sleep(RECOVERY_DEBOUNCE).await;
        run_recovery(store, target, backend, &RECOVERY_RETRY_DELAYS).await;
    });
}

#[cfg(any(feature = "web", feature = "mobile", test))]
fn next_target(mut store: EditorStore, include_identity: bool) -> RecoveryTarget {
    let generation = store.edit_generation().wrapping_add(1);
    store.edit_generation.set(generation);
    RecoveryTarget {
        generation,
        identity: include_identity.then(|| document_identity(store)).flatten(),
    }
}

#[cfg(any(feature = "web", feature = "mobile", test))]
async fn run_recovery(
    store: EditorStore,
    target: RecoveryTarget,
    backend: Rc<dyn RecoveryBackend>,
    retry_delays: &[Duration],
) {
    let mut attempt_index = 0;
    loop {
        if stale(store, target) {
            return;
        }
        match backend.attempt(store, target).await {
            RecoveryAttempt::Succeeded => {
                store.mark_recovery_healthy();
                return;
            }
            RecoveryAttempt::Stale => return,
            RecoveryAttempt::Failed { kind, error } => {
                let Some(delay) = retry_delays.get(attempt_index) else {
                    store.report_recovery_failure(failure_notice(kind, error));
                    return;
                };
                attempt_index += 1;
                if !delay.is_zero() {
                    sleep(*delay).await;
                }
            }
        }
    }
}

#[cfg(any(feature = "web", feature = "mobile", test))]
fn failure_notice(kind: RecoveryFailureKind, error: AppErrorDto) -> UiNotice {
    match kind {
        RecoveryFailureKind::Checkpoint => UiNotice {
            title: "Automatic recovery unavailable".to_string(),
            message: format!(
                "Recent changes could not be written to recovery storage: {}. Save the workbook manually.",
                error.message
            ),
        },
        RecoveryFailureKind::Cleanup => UiNotice {
            title: "Recovery cleanup failed".to_string(),
            message: format!(
                "An older recovery copy could not be removed: {}.",
                error.message
            ),
        },
    }
}

#[cfg(any(feature = "web", feature = "mobile", test))]
fn stale(store: EditorStore, target: RecoveryTarget) -> bool {
    store.edit_generation() != target.generation
        || target
            .identity
            .is_some_and(|identity| document_identity(store) != Some(identity))
}

#[cfg(feature = "web")]
struct WebRecoveryBackend {
    ports: Rc<AppPorts>,
}

#[cfg(feature = "web")]
impl RecoveryBackend for WebRecoveryBackend {
    fn attempt(&self, store: EditorStore, target: RecoveryTarget) -> RecoveryFuture {
        use simple_table_web_protocol::{WebWorkspaceReply, WebWorkspaceRequest};

        let ports = Rc::clone(&self.ports);
        Box::pin(async move {
            let _operation = ports.operations.lock().await;
            if stale(store, target) {
                return RecoveryAttempt::Stale;
            }
            let Some((document_id, base_revision)) = target.identity else {
                return RecoveryAttempt::Stale;
            };
            let dirty = store
                .document
                .read()
                .as_ref()
                .is_some_and(|document| document.editor_session.editor_state.is_dirty);
            let (kind, result) = if dirty {
                (
                    RecoveryFailureKind::Checkpoint,
                    ports
                        .workspace
                        .execute(WebWorkspaceRequest::CheckpointRecovery {
                            request_id: crate::model::request_id("recovery"),
                            document_id,
                            base_revision,
                            target_name: document_name(store),
                        })
                        .await,
                )
            } else {
                (
                    RecoveryFailureKind::Cleanup,
                    ports
                        .workspace
                        .execute(WebWorkspaceRequest::ClearRecovery)
                        .await,
                )
            };
            if stale(store, target) {
                return RecoveryAttempt::Stale;
            }
            match result {
                Ok(WebWorkspaceReply::Empty) => RecoveryAttempt::Succeeded,
                Ok(_) => RecoveryAttempt::Failed {
                    kind,
                    error: protocol_error("unexpected recovery workspace response"),
                },
                Err(error) => RecoveryAttempt::Failed { kind, error },
            }
        })
    }
}

#[cfg(feature = "mobile")]
struct MobileRecoveryBackend {
    ports: Rc<AppPorts>,
}

#[cfg(feature = "mobile")]
impl RecoveryBackend for MobileRecoveryBackend {
    fn attempt(&self, store: EditorStore, target: RecoveryTarget) -> RecoveryFuture {
        use crate::protocol::{EditorCommand, EditorReply, EditorRequest};

        let ports = Rc::clone(&self.ports);
        Box::pin(async move {
            let _operation = ports.operations.lock().await;
            if stale(store, target) {
                return RecoveryAttempt::Stale;
            }
            let Some((document_id, base_revision)) = target.identity else {
                return RecoveryAttempt::Stale;
            };
            let dirty = store
                .document
                .read()
                .as_ref()
                .is_some_and(|document| document.editor_session.editor_state.is_dirty);
            if !dirty {
                return match ports.recovery.clear().await {
                    Ok(()) => RecoveryAttempt::Succeeded,
                    Err(error) => RecoveryAttempt::Failed {
                        kind: RecoveryFailureKind::Cleanup,
                        error,
                    },
                };
            }
            let output = match ports
                .editor
                .execute_command(EditorCommand::new(EditorRequest::PrepareExport {
                    document_id,
                    base_revision,
                    target_name: document_name(store),
                }))
                .await
            {
                Ok(output) => output,
                Err(error) => {
                    return RecoveryAttempt::Failed {
                        kind: RecoveryFailureKind::Checkpoint,
                        error,
                    };
                }
            };
            if stale(store, target) {
                return RecoveryAttempt::Stale;
            }
            let EditorReply::ExportPrepared { file_name } = output.reply else {
                return RecoveryAttempt::Failed {
                    kind: RecoveryFailureKind::Checkpoint,
                    error: protocol_error("unexpected recovery export response"),
                };
            };
            let Some(bytes) = output.attachment else {
                return RecoveryAttempt::Failed {
                    kind: RecoveryFailureKind::Checkpoint,
                    error: protocol_error("recovery export omitted workbook bytes"),
                };
            };
            match ports.recovery.checkpoint(file_name, bytes).await {
                Ok(()) => RecoveryAttempt::Succeeded,
                Err(error) => RecoveryAttempt::Failed {
                    kind: RecoveryFailureKind::Checkpoint,
                    error,
                },
            }
        })
    }
}

#[cfg(feature = "mobile")]
struct MobileRecoveryCleanupBackend {
    ports: Rc<AppPorts>,
}

#[cfg(feature = "mobile")]
impl RecoveryBackend for MobileRecoveryCleanupBackend {
    fn attempt(&self, store: EditorStore, target: RecoveryTarget) -> RecoveryFuture {
        let ports = Rc::clone(&self.ports);
        Box::pin(async move {
            let _operation = ports.operations.lock().await;
            if stale(store, target) {
                return RecoveryAttempt::Stale;
            }
            match ports.recovery.clear().await {
                Ok(()) => RecoveryAttempt::Succeeded,
                Err(error) => RecoveryAttempt::Failed {
                    kind: RecoveryFailureKind::Cleanup,
                    error,
                },
            }
        })
    }
}

#[cfg(any(feature = "web", feature = "mobile"))]
fn protocol_error(message: &str) -> AppErrorDto {
    AppErrorDto {
        code: "protocol_error".to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use dioxus::prelude::*;

    use super::*;
    use crate::model::use_editor_store;

    thread_local! {
        #[allow(clippy::missing_const_for_thread_local)]
        static TEST_STORE: RefCell<Option<EditorStore>> = const { RefCell::new(None) };
    }

    #[component]
    fn Harness() -> Element {
        let store = use_editor_store();
        TEST_STORE.with(|slot| slot.replace(Some(store)));
        rsx! {}
    }

    fn with_store(test: impl FnOnce(EditorStore)) {
        let mut dom = VirtualDom::new(Harness);
        dom.rebuild_in_place();
        let store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));
        test(store);
        TEST_STORE.with(|slot| slot.replace(None));
    }

    struct ScriptedBackend {
        attempts: Rc<RefCell<usize>>,
        results: Rc<RefCell<VecDeque<RecoveryAttempt>>>,
    }

    impl RecoveryBackend for ScriptedBackend {
        fn attempt(&self, _store: EditorStore, _target: RecoveryTarget) -> RecoveryFuture {
            *self.attempts.borrow_mut() += 1;
            let result = self
                .results
                .borrow_mut()
                .pop_front()
                .expect("scripted recovery result");
            Box::pin(async move { result })
        }
    }

    fn failure(kind: RecoveryFailureKind) -> RecoveryAttempt {
        RecoveryAttempt::Failed {
            kind,
            error: AppErrorDto {
                code: "storage_error".to_string(),
                message: "quota exceeded".to_string(),
            },
        }
    }

    fn run_script(store: EditorStore, results: Vec<RecoveryAttempt>) -> usize {
        let attempts = Rc::new(RefCell::new(0));
        let backend = Rc::new(ScriptedBackend {
            attempts: Rc::clone(&attempts),
            results: Rc::new(RefCell::new(results.into())),
        });
        let target = next_target(store, false);
        futures::executor::block_on(run_recovery(
            store,
            target,
            backend,
            &[Duration::ZERO, Duration::ZERO],
        ));
        attempts.take()
    }

    #[test]
    fn transient_recovery_failure_retries_without_warning() {
        with_store(|store| {
            let attempts = run_script(
                store,
                vec![
                    failure(RecoveryFailureKind::Checkpoint),
                    RecoveryAttempt::Succeeded,
                ],
            );

            assert_eq!(attempts, 2);
            assert!(store.warning.read().is_none());
        });
    }

    #[test]
    fn exhausted_recovery_failure_warns_once_until_a_success() {
        with_store(|mut store| {
            let failures = || {
                vec![
                    failure(RecoveryFailureKind::Checkpoint),
                    failure(RecoveryFailureKind::Checkpoint),
                    failure(RecoveryFailureKind::Checkpoint),
                ]
            };
            assert_eq!(run_script(store, failures()), 3);
            assert_eq!(
                store
                    .warning
                    .read()
                    .as_ref()
                    .map(|notice| notice.title.as_str()),
                Some("Automatic recovery unavailable")
            );

            store.warning.set(None);
            run_script(store, failures());
            assert!(store.warning.read().is_none());

            run_script(store, vec![RecoveryAttempt::Succeeded]);
            run_script(store, failures());
            assert!(store.warning.read().is_some());
        });
    }

    #[test]
    fn cleanup_failure_uses_the_cleanup_warning() {
        with_store(|store| {
            run_script(
                store,
                vec![
                    failure(RecoveryFailureKind::Cleanup),
                    failure(RecoveryFailureKind::Cleanup),
                    failure(RecoveryFailureKind::Cleanup),
                ],
            );

            assert_eq!(
                store
                    .warning
                    .read()
                    .as_ref()
                    .map(|notice| notice.title.as_str()),
                Some("Recovery cleanup failed")
            );
        });
    }

    #[test]
    fn backend_can_cancel_a_recovery_as_stale() {
        with_store(|store| {
            assert_eq!(run_script(store, vec![RecoveryAttempt::Stale]), 1);
            assert!(store.warning.read().is_none());
        });
    }

    #[test]
    fn stale_recovery_target_is_cancelled_before_calling_the_backend() {
        with_store(|mut store| {
            let attempts = Rc::new(RefCell::new(0));
            let backend = Rc::new(ScriptedBackend {
                attempts: Rc::clone(&attempts),
                results: Rc::new(RefCell::new(VecDeque::from([RecoveryAttempt::Succeeded]))),
            });
            let target = next_target(store, false);
            store.edit_generation.set(target.generation.wrapping_add(1));

            futures::executor::block_on(run_recovery(store, target, backend, &[]));

            assert_eq!(*attempts.borrow(), 0);
            assert!(store.warning.read().is_none());
        });
    }
}
