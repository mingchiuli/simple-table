#[cfg(all(feature = "mobile", target_os = "android"))]
pub(crate) mod android;
pub mod editor;
pub mod file;
#[cfg(feature = "mobile")]
pub mod recovery;
pub mod update;
pub mod window;
#[cfg(feature = "web")]
pub(crate) mod worker;
#[cfg(not(feature = "mobile"))]
pub mod workspace;

#[cfg(not(feature = "mobile"))]
pub fn platform_editor_and_workspace_ports() -> (
    std::rc::Rc<dyn editor::EditorPort>,
    std::rc::Rc<dyn workspace::LocalWorkspacePort>,
) {
    #[cfg(feature = "web")]
    {
        match worker::WorkerClient::new() {
            Ok(client) => {
                let client = std::rc::Rc::new(client);
                (
                    editor::worker_editor_port(std::rc::Rc::clone(&client)),
                    workspace::worker_workspace_port(client),
                )
            }
            Err(error) => (
                editor::failed_editor_port(error.clone()),
                workspace::failed_workspace_port(error),
            ),
        }
    }

    #[cfg(not(feature = "web"))]
    (
        editor::platform_editor_port(),
        workspace::platform_workspace_port(),
    )
}
