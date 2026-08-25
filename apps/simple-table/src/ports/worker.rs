use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::channel::oneshot;
use js_sys::{Array, ArrayBuffer, Object, Reflect, Uint8Array};
use simple_table_web_protocol::{
    AttachmentMetadata, WEB_WORKER_PROTOCOL_VERSION, WorkerReply, WorkerRequest,
    WorkerRequestEnvelope, WorkerResponseEnvelope,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{ErrorEvent, Event, MessageEvent, Worker, WorkerOptions, WorkerType};

use crate::protocol::AppErrorDto;

use super::editor::PortFuture;

pub(crate) struct WorkerMessage {
    pub reply: WorkerReply,
    pub attachment: Option<Vec<u8>>,
}

type PendingSender = oneshot::Sender<Result<WorkerMessage, AppErrorDto>>;

pub(crate) struct WorkerClient {
    worker: Worker,
    pending: Rc<RefCell<HashMap<String, PendingSender>>>,
    failure: Rc<RefCell<Option<AppErrorDto>>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_message_error: Closure<dyn FnMut(Event)>,
}

impl WorkerClient {
    pub(crate) fn new() -> Result<Self, AppErrorDto> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        options.set_name("simple-table-editor");
        let worker = Worker::new_with_options("/workers/editor.js", &options).map_err(js_error)?;
        let pending = Rc::new(RefCell::new(HashMap::<String, PendingSender>::new()));
        let failure = Rc::new(RefCell::new(None));

        let on_message_pending = Rc::clone(&pending);
        let on_message_failure = Rc::clone(&failure);
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            let decoded = decode_message(event.data());
            match decoded {
                Ok((message_id, response)) => {
                    if let Some(sender) = on_message_pending.borrow_mut().remove(&message_id) {
                        let _ = sender.send(response);
                    } else {
                        fail_worker(
                            &on_message_pending,
                            &on_message_failure,
                            "worker_protocol_error",
                            "the editor worker returned an unknown message id",
                        );
                    }
                }
                Err(error) => fail_worker(
                    &on_message_pending,
                    &on_message_failure,
                    &error.code,
                    &error.message,
                ),
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

        Ok(Self {
            worker,
            pending,
            failure,
            _on_message: on_message,
            _on_error: on_error,
            _on_message_error: on_message_error,
        })
    }

    pub(crate) fn execute(
        self: &Rc<Self>,
        request: WorkerRequest,
        attachment: Option<Vec<u8>>,
    ) -> PortFuture<Result<WorkerMessage, AppErrorDto>> {
        let client = Rc::clone(self);
        Box::pin(async move {
            if let Some(error) = client.failure.borrow().clone() {
                return Err(error);
            }
            let message_id = uuid::Uuid::new_v4().to_string();
            let envelope = WorkerRequestEnvelope {
                protocol_version: WEB_WORKER_PROTOCOL_VERSION,
                message_id: message_id.clone(),
                request,
                attachment: attachment.as_ref().map(|bytes| AttachmentMetadata {
                    byte_length: bytes.len(),
                }),
            };
            let metadata = serde_json::to_string(&envelope).map_err(protocol_error)?;
            let message = Object::new();
            Reflect::set(&message, &"metadata".into(), &metadata.into()).map_err(js_error)?;
            let transfer = Array::new();
            if let Some(bytes) = attachment {
                let buffer = Uint8Array::from(bytes.as_slice()).buffer();
                Reflect::set(&message, &"attachment".into(), &buffer).map_err(js_error)?;
                transfer.push(&buffer);
            }

            let (sender, receiver) = oneshot::channel();
            client
                .pending
                .borrow_mut()
                .insert(message_id.clone(), sender);
            if let Err(error) = client
                .worker
                .post_message_with_transfer(&message, &transfer)
            {
                client.pending.borrow_mut().remove(&message_id);
                return Err(js_error(error));
            }
            receiver.await.map_err(|_| AppErrorDto {
                code: "worker_disconnected".to_string(),
                message: "editor worker disconnected".to_string(),
            })?
        })
    }
}

fn decode_message(
    value: wasm_bindgen::JsValue,
) -> Result<(String, Result<WorkerMessage, AppErrorDto>), AppErrorDto> {
    let metadata = Reflect::get(&value, &"metadata".into())
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| worker_protocol_error("worker response metadata is not text"))?;
    let envelope: WorkerResponseEnvelope =
        serde_json::from_str(&metadata).map_err(protocol_error)?;
    if envelope.protocol_version != WEB_WORKER_PROTOCOL_VERSION {
        return Err(worker_protocol_error(format!(
            "unsupported worker protocol version {}",
            envelope.protocol_version
        )));
    }
    let attachment = Reflect::get(&value, &"attachment".into())
        .map_err(js_error)?
        .dyn_into::<ArrayBuffer>()
        .ok()
        .map(|buffer| Uint8Array::new(&buffer).to_vec());
    let described_length = envelope
        .attachment
        .as_ref()
        .map(|metadata| metadata.byte_length);
    let actual_length = attachment.as_ref().map(Vec::len);
    if described_length != actual_length {
        return Err(worker_protocol_error(
            "worker response attachment metadata does not match its buffer",
        ));
    }
    let response = envelope
        .response
        .map(|reply| WorkerMessage { reply, attachment });
    Ok((envelope.message_id, response))
}

fn protocol_error(error: impl std::fmt::Display) -> AppErrorDto {
    worker_protocol_error(error.to_string())
}

fn worker_protocol_error(message: impl Into<String>) -> AppErrorDto {
    AppErrorDto {
        code: "worker_protocol_error".to_string(),
        message: message.into(),
    }
}

fn js_error(value: wasm_bindgen::JsValue) -> AppErrorDto {
    AppErrorDto {
        code: "worker_start_failed".to_string(),
        message: value.as_string().unwrap_or_else(|| format!("{value:?}")),
    }
}

fn fail_worker(
    pending: &RefCell<HashMap<String, PendingSender>>,
    failure: &RefCell<Option<AppErrorDto>>,
    code: &str,
    message: &str,
) {
    let error = AppErrorDto {
        code: code.to_string(),
        message: message.to_string(),
    };
    failure.replace(Some(error.clone()));
    for (_, sender) in pending.borrow_mut().drain() {
        let _ = sender.send(Err(error.clone()));
    }
}
