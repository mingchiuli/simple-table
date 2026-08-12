use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::protocol::{AppErrorDto, EditorRequest, EditorResponse};

pub type PortFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

pub trait EditorPort {
    fn execute(&self, request: EditorRequest) -> PortFuture<EditorResponse>;
}

#[cfg(any(feature = "desktop", feature = "mobile"))]
pub fn platform_editor_port() -> Rc<dyn EditorPort> {
    Rc::new(native::NativeEditorPort::default())
}

#[cfg(all(not(any(feature = "desktop", feature = "mobile")), feature = "web"))]
pub fn platform_editor_port() -> Rc<dyn EditorPort> {
    Rc::new(web::WorkerEditorPort::new())
}

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
pub fn platform_editor_port() -> Rc<dyn EditorPort> {
    Rc::new(ServerEditorPort)
}

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
struct ServerEditorPort;

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
impl EditorPort for ServerEditorPort {
    fn execute(&self, _request: EditorRequest) -> PortFuture<EditorResponse> {
        Box::pin(async {
            Err(AppErrorDto {
                code: "client_not_hydrated".to_string(),
                message: "The editor becomes available after client hydration".to_string(),
            })
        })
    }
}

#[cfg(any(feature = "desktop", feature = "mobile"))]
mod native {
    use std::sync::{Arc, Mutex};

    use super::*;
    use simple_table_engine::CoreFacade;

    #[derive(Clone, Default)]
    pub struct NativeEditorPort {
        core: Arc<Mutex<CoreFacade>>,
    }

    impl EditorPort for NativeEditorPort {
        fn execute(&self, request: EditorRequest) -> PortFuture<EditorResponse> {
            let core = Arc::clone(&self.core);
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    core.lock()
                        .map_err(|_| AppErrorDto {
                            code: "internal".to_string(),
                            message: "editor lock poisoned".to_string(),
                        })?
                        .execute(request)
                })
                .await
                .map_err(|error| AppErrorDto {
                    code: "editor_task_failed".to_string(),
                    message: error.to_string(),
                })?
            })
        }
    }
}

#[cfg(feature = "web")]
mod web {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use futures::channel::oneshot;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{ErrorEvent, Event, MessageEvent, Worker, WorkerOptions, WorkerType};

    use super::*;

    struct WorkerClient {
        worker: Worker,
        pending: Rc<RefCell<VecDeque<oneshot::Sender<EditorResponse>>>>,
        failure: Rc<RefCell<Option<AppErrorDto>>>,
        _on_message: Closure<dyn FnMut(MessageEvent)>,
        _on_error: Closure<dyn FnMut(ErrorEvent)>,
        _on_message_error: Closure<dyn FnMut(Event)>,
    }

    #[derive(Clone)]
    pub struct WorkerEditorPort(Rc<Result<WorkerClient, AppErrorDto>>);

    impl WorkerEditorPort {
        pub fn new() -> Self {
            let client = (|| {
                let options = WorkerOptions::new();
                options.set_type(WorkerType::Module);
                options.set_name("simple-table-editor");
                let worker =
                    Worker::new_with_options("/workers/editor.js", &options).map_err(js_error)?;
                let pending = Rc::new(RefCell::new(
                    VecDeque::<oneshot::Sender<EditorResponse>>::new(),
                ));
                let failure = Rc::new(RefCell::new(None));
                let on_message_pending = Rc::clone(&pending);
                let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
                    let response = event
                        .data()
                        .as_string()
                        .ok_or_else(|| AppErrorDto {
                            code: "worker_protocol_error".to_string(),
                            message: "worker returned a non-text response".to_string(),
                        })
                        .and_then(|json| {
                            serde_json::from_str::<EditorResponse>(&json).map_err(|error| {
                                AppErrorDto {
                                    code: "worker_protocol_error".to_string(),
                                    message: error.to_string(),
                                }
                            })
                        })
                        .and_then(|response| response);
                    if let Some(sender) = on_message_pending.borrow_mut().pop_front() {
                        let _ = sender.send(response);
                    }
                }) as Box<dyn FnMut(MessageEvent)>);
                worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
                let on_error_pending = Rc::clone(&pending);
                let on_error_failure = Rc::clone(&failure);
                let on_error = Closure::wrap(Box::new(move |event: ErrorEvent| {
                    fail_worker(
                        &on_error_pending,
                        &on_error_failure,
                        "worker_failed",
                        &format!(
                            "{}:{}: {}",
                            event.filename(),
                            event.lineno(),
                            event.message()
                        ),
                    );
                }) as Box<dyn FnMut(ErrorEvent)>);
                worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
                let on_message_error_pending = Rc::clone(&pending);
                let on_message_error_failure = Rc::clone(&failure);
                let on_message_error = Closure::wrap(Box::new(move |_event: Event| {
                    fail_worker(
                        &on_message_error_pending,
                        &on_message_error_failure,
                        "worker_protocol_error",
                        "the editor worker returned an unreadable message",
                    );
                }) as Box<dyn FnMut(Event)>);
                worker.set_onmessageerror(Some(on_message_error.as_ref().unchecked_ref()));
                Ok(WorkerClient {
                    worker,
                    pending,
                    failure,
                    _on_message: on_message,
                    _on_error: on_error,
                    _on_message_error: on_message_error,
                })
            })();
            Self(Rc::new(client))
        }
    }

    impl EditorPort for WorkerEditorPort {
        fn execute(&self, request: EditorRequest) -> PortFuture<EditorResponse> {
            let client = Rc::clone(&self.0);
            Box::pin(async move {
                let client = client.as_ref().as_ref().map_err(Clone::clone)?;
                if let Some(error) = client.failure.borrow().clone() {
                    return Err(error);
                }
                let json = serde_json::to_string(&request).map_err(|error| AppErrorDto {
                    code: "worker_protocol_error".to_string(),
                    message: error.to_string(),
                })?;
                let (sender, receiver) = oneshot::channel();
                client.pending.borrow_mut().push_back(sender);
                if let Err(error) = client.worker.post_message(&json.into()) {
                    client.pending.borrow_mut().pop_back();
                    return Err(js_error(error));
                }
                receiver.await.map_err(|_| AppErrorDto {
                    code: "worker_disconnected".to_string(),
                    message: "editor worker disconnected".to_string(),
                })?
            })
        }
    }

    fn js_error(value: wasm_bindgen::JsValue) -> AppErrorDto {
        AppErrorDto {
            code: "worker_start_failed".to_string(),
            message: value.as_string().unwrap_or_else(|| format!("{value:?}")),
        }
    }

    fn fail_worker(
        pending: &RefCell<VecDeque<oneshot::Sender<EditorResponse>>>,
        failure: &RefCell<Option<AppErrorDto>>,
        code: &str,
        message: &str,
    ) {
        let error = AppErrorDto {
            code: code.to_string(),
            message: message.to_string(),
        };
        failure.replace(Some(error.clone()));
        for sender in pending.borrow_mut().drain(..) {
            let _ = sender.send(Err(error.clone()));
        }
    }
}
