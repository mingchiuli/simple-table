use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::protocol::{AppErrorDto, EditorCommand, EditorReply, EditorRequest, EditorResponse};

pub type PortFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

pub trait EditorPort {
    fn execute(&self, request: EditorRequest) -> PortFuture<Result<EditorReply, AppErrorDto>>;

    fn execute_command(&self, command: EditorCommand) -> PortFuture<EditorResponse> {
        if command.attachment.is_some() {
            return Box::pin(async {
                Err(AppErrorDto {
                    code: "unsupported_attachment".to_string(),
                    message: "This editor port does not support binary attachments".to_string(),
                })
            });
        }
        let future = self.execute(command.request);
        Box::pin(async move { future.await.map(crate::protocol::EditorOutput::new) })
    }
}

#[cfg(feature = "web")]
pub(crate) fn failed_editor_port(error: AppErrorDto) -> Rc<dyn EditorPort> {
    Rc::new(FailedEditorPort(error))
}

#[cfg(feature = "web")]
struct FailedEditorPort(AppErrorDto);

#[cfg(feature = "web")]
impl EditorPort for FailedEditorPort {
    fn execute(&self, _request: EditorRequest) -> PortFuture<Result<EditorReply, AppErrorDto>> {
        let error = self.0.clone();
        Box::pin(async move { Err(error) })
    }
}

#[cfg(any(feature = "desktop", feature = "mobile"))]
pub fn platform_editor_port() -> Rc<dyn EditorPort> {
    Rc::new(native::NativeEditorPort::default())
}

#[cfg(all(not(any(feature = "desktop", feature = "mobile")), feature = "web"))]
pub(crate) fn worker_editor_port(client: Rc<super::worker::WorkerClient>) -> Rc<dyn EditorPort> {
    Rc::new(WorkerEditorPort(client))
}

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
pub fn platform_editor_port() -> Rc<dyn EditorPort> {
    Rc::new(ServerEditorPort)
}

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
struct ServerEditorPort;

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
impl EditorPort for ServerEditorPort {
    fn execute(&self, _request: EditorRequest) -> PortFuture<Result<EditorReply, AppErrorDto>> {
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
        fn execute(&self, request: EditorRequest) -> PortFuture<Result<EditorReply, AppErrorDto>> {
            let core = Arc::clone(&self.core);
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    core.lock()
                        .map_err(|_| AppErrorDto {
                            code: "internal".to_string(),
                            message: "editor lock poisoned".to_string(),
                        })?
                        .execute(request)
                        .map(|output| output.reply)
                })
                .await
                .map_err(|error| AppErrorDto {
                    code: "editor_task_failed".to_string(),
                    message: error.to_string(),
                })?
            })
        }

        fn execute_command(&self, command: EditorCommand) -> PortFuture<EditorResponse> {
            let core = Arc::clone(&self.core);
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    core.lock()
                        .map_err(|_| AppErrorDto {
                            code: "internal".to_string(),
                            message: "editor lock poisoned".to_string(),
                        })?
                        .execute(command)
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
struct WorkerEditorPort(Rc<super::worker::WorkerClient>);

#[cfg(feature = "web")]
impl EditorPort for WorkerEditorPort {
    fn execute(&self, request: EditorRequest) -> PortFuture<Result<EditorReply, AppErrorDto>> {
        let future = self.execute_command(EditorCommand::new(request));
        Box::pin(async move { future.await.map(|output| output.reply) })
    }

    fn execute_command(&self, command: EditorCommand) -> PortFuture<EditorResponse> {
        let client = Rc::clone(&self.0);
        Box::pin(async move {
            let message = client
                .execute(
                    simple_table_web_protocol::WorkerRequest::Editor(command.request),
                    command.attachment,
                )
                .await?;
            match message.reply {
                simple_table_web_protocol::WorkerReply::Editor(reply) => {
                    Ok(crate::protocol::EditorOutput {
                        reply,
                        attachment: message.attachment,
                    })
                }
                simple_table_web_protocol::WorkerReply::Workspace(_) => Err(AppErrorDto {
                    code: "worker_protocol_error".to_string(),
                    message: "worker returned a workspace reply for an editor request".to_string(),
                }),
            }
        })
    }
}
