use std::rc::Rc;

use simple_table_web_protocol::{WebWorkspaceReply, WebWorkspaceRequest};

use crate::protocol::AppErrorDto;

use super::editor::PortFuture;

pub trait LocalWorkspacePort {
    fn execute(
        &self,
        request: WebWorkspaceRequest,
    ) -> PortFuture<Result<WebWorkspaceReply, AppErrorDto>>;
}

#[cfg(feature = "web")]
pub(crate) fn failed_workspace_port(error: AppErrorDto) -> Rc<dyn LocalWorkspacePort> {
    Rc::new(FailedWorkspacePort(error))
}

#[cfg(feature = "web")]
struct FailedWorkspacePort(AppErrorDto);

#[cfg(feature = "web")]
impl LocalWorkspacePort for FailedWorkspacePort {
    fn execute(
        &self,
        _request: WebWorkspaceRequest,
    ) -> PortFuture<Result<WebWorkspaceReply, AppErrorDto>> {
        let error = self.0.clone();
        Box::pin(async move { Err(error) })
    }
}

#[cfg(feature = "web")]
pub(crate) fn worker_workspace_port(
    client: Rc<super::worker::WorkerClient>,
) -> Rc<dyn LocalWorkspacePort> {
    Rc::new(WebWorkspacePort(client))
}

#[cfg(not(feature = "web"))]
pub fn platform_workspace_port() -> Rc<dyn LocalWorkspacePort> {
    Rc::new(UnavailableWorkspacePort)
}

#[cfg(not(feature = "web"))]
struct UnavailableWorkspacePort;

#[cfg(not(feature = "web"))]
impl LocalWorkspacePort for UnavailableWorkspacePort {
    fn execute(
        &self,
        _request: WebWorkspaceRequest,
    ) -> PortFuture<Result<WebWorkspaceReply, AppErrorDto>> {
        Box::pin(async {
            Err(AppErrorDto {
                code: "workspace_unavailable".to_string(),
                message: "Local workspace storage is unavailable on this platform".to_string(),
            })
        })
    }
}

#[cfg(feature = "web")]
struct WebWorkspacePort(Rc<super::worker::WorkerClient>);

#[cfg(feature = "web")]
impl LocalWorkspacePort for WebWorkspacePort {
    fn execute(
        &self,
        request: WebWorkspaceRequest,
    ) -> PortFuture<Result<WebWorkspaceReply, AppErrorDto>> {
        let client = Rc::clone(&self.0);
        Box::pin(async move {
            let message = client
                .execute(
                    simple_table_web_protocol::WorkerRequest::Workspace(request),
                    None,
                )
                .await?;
            if message.attachment.is_some() {
                return Err(AppErrorDto {
                    code: "worker_protocol_error".to_string(),
                    message: "workspace response contained an unexpected attachment".to_string(),
                });
            }
            match message.reply {
                simple_table_web_protocol::WorkerReply::Workspace(reply) => Ok(reply),
                simple_table_web_protocol::WorkerReply::Editor(_) => Err(AppErrorDto {
                    code: "worker_protocol_error".to_string(),
                    message: "worker returned an editor reply for a workspace request".to_string(),
                }),
            }
        })
    }
}
