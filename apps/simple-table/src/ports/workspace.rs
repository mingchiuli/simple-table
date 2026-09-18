//! Browser-side local workspace port.
//!
//! The commands exchanged here are the Web Worker / IndexedDB workspace
//! protocol, so this module is compiled for the `web` target only. Desktop and
//! mobile store documents through `ports::file` and `ports::recovery` instead.

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

pub(crate) fn failed_workspace_port(error: AppErrorDto) -> Rc<dyn LocalWorkspacePort> {
    Rc::new(FailedWorkspacePort(error))
}

struct FailedWorkspacePort(AppErrorDto);

impl LocalWorkspacePort for FailedWorkspacePort {
    fn execute(
        &self,
        _request: WebWorkspaceRequest,
    ) -> PortFuture<Result<WebWorkspaceReply, AppErrorDto>> {
        let error = self.0.clone();
        Box::pin(async move { Err(error) })
    }
}

pub(crate) fn worker_workspace_port(
    client: Rc<super::worker::WorkerClient>,
) -> Rc<dyn LocalWorkspacePort> {
    Rc::new(WebWorkspacePort(client))
}

struct WebWorkspacePort(Rc<super::worker::WorkerClient>);

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
