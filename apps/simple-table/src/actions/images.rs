use std::rc::Rc;

use base64::Engine;
use dioxus::prelude::{ReadableExt, WritableExt};

use super::shared::{document_identity, unexpected_reply};
use crate::model::{AppPorts, EditorStore};
use crate::protocol::{EditorCommand, EditorReply, EditorRequest};

pub async fn refresh_images(mut store: EditorStore, ports: Rc<AppPorts>) {
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let sheet_index = store.active_sheet();
    let items = match load_image_catalog(
        ports.editor.as_ref(),
        document_id,
        base_revision,
        sheet_index,
    )
    .await
    {
        Ok(items) => items,
        Err(error) => {
            store.set_error(error);
            return;
        }
    };

    let previous_items = store.images.read().clone();
    let previous_assets = store.image_assets.read().clone();
    let mut assets = std::collections::HashMap::new();
    for image in &items {
        if !image.renderable {
            continue;
        }
        let unchanged_asset = previous_items.iter().any(|previous| {
            previous.id == image.id
                && previous.media_id == image.media_id
                && previous.mime_type == image.mime_type
        });
        if unchanged_asset && let Some(asset) = previous_assets.get(&image.id) {
            assets.insert(image.id.clone(), Rc::clone(asset));
            continue;
        }
        match ports
            .editor
            .execute_command(EditorCommand::new(EditorRequest::ImageBytes {
                document_id,
                base_revision,
                sheet_index,
                image_id: image.id.clone(),
            }))
            .await
        {
            Ok(output) if matches!(output.reply, EditorReply::Bytes) => {
                let Some(bytes) = output.attachment else {
                    store.set_error(unexpected_reply("image bytes"));
                    return;
                };
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                assets.insert(
                    image.id.clone(),
                    Rc::<str>::from(format!("data:{};base64,{encoded}", image.mime_type)),
                );
            }
            Ok(_) => {}
            Err(error) => {
                store.set_error(error);
                return;
            }
        }
    }
    if store
        .selected_image
        .read()
        .as_ref()
        .is_some_and(|id| !items.iter().any(|image| &image.id == id))
    {
        store.selected_image.set(None);
    }
    store.images.set(Rc::new(items));
    store.image_assets.set(Rc::new(assets));
}

async fn load_image_catalog(
    editor: &dyn crate::ports::editor::EditorPort,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
) -> Result<Vec<crate::protocol::SheetImageDto>, crate::protocol::AppErrorDto> {
    let mut items = Vec::new();
    let mut offset = 0;
    loop {
        let next_offset = match editor
            .execute(EditorRequest::SheetImages {
                document_id,
                base_revision,
                sheet_index,
                offset,
                limit: 256,
            })
            .await
        {
            Ok(EditorReply::Images {
                items: page,
                next_offset,
            }) => {
                items.extend(page);
                next_offset
            }
            Ok(_) => return Err(unexpected_reply("image catalog")),
            Err(error) => return Err(error),
        };
        let Some(next_offset) = next_offset else {
            break;
        };
        if next_offset <= offset {
            return Err(crate::protocol::AppErrorDto {
                code: "protocol_error".to_string(),
                message: "image catalog returned a non-advancing page cursor".to_string(),
            });
        }
        offset = next_offset;
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::ports::editor::{EditorPort, PortFuture};
    use crate::protocol::{ImageAnchorDto, ImageMarkerDto, SheetImageDto};

    fn image(id: &str) -> SheetImageDto {
        SheetImageDto {
            id: id.to_string(),
            media_id: format!("media-{id}"),
            mime_type: "image/png".to_string(),
            intrinsic_width: 1,
            intrinsic_height: 1,
            anchor: ImageAnchorDto::OneCell {
                from: ImageMarkerDto {
                    row: 0,
                    col: 0,
                    row_offset_emu: 0,
                    col_offset_emu: 0,
                },
                width_emu: 9_525,
                height_emu: 9_525,
            },
            z_index: 0,
            renderable: false,
        }
    }

    struct PagedImageEditor {
        offsets: Rc<RefCell<Vec<usize>>>,
    }

    impl EditorPort for PagedImageEditor {
        fn execute(
            &self,
            request: EditorRequest,
        ) -> PortFuture<Result<EditorReply, crate::protocol::AppErrorDto>> {
            let EditorRequest::SheetImages { offset, .. } = request else {
                panic!("unexpected request");
            };
            self.offsets.borrow_mut().push(offset);
            let response = match offset {
                0 => Ok(EditorReply::Images {
                    items: vec![image("first")],
                    next_offset: Some(256),
                }),
                256 => Ok(EditorReply::Images {
                    items: vec![image("second")],
                    next_offset: None,
                }),
                _ => panic!("unexpected image page offset"),
            };
            Box::pin(async move { response })
        }
    }

    #[test]
    fn image_catalog_follows_all_page_cursors() {
        let offsets = Rc::new(RefCell::new(Vec::new()));
        let editor = PagedImageEditor {
            offsets: Rc::clone(&offsets),
        };

        let images = futures::executor::block_on(load_image_catalog(&editor, 7, 1, 0)).unwrap();

        assert_eq!(offsets.borrow().as_slice(), &[0, 256]);
        assert_eq!(
            images
                .iter()
                .map(|image| image.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
