use std::rc::Rc;

use dioxus::prelude::*;
use simple_table_components::icons::{ImagePlus, Move, Trash2};
use simple_table_components::{Button, ButtonSize, ButtonVariant, Input, Label};
#[cfg(not(feature = "mobile"))]
use simple_table_components::{ContentSide, Tooltip, TooltipContent, TooltipTrigger};

use crate::actions;
use crate::model::{AppPorts, EditorStore};
use crate::protocol::{ImageAnchorDto, ImageMarkerDto, SheetImageDto};
#[cfg(feature = "mobile")]
use crate::ui::ToolbarIconButton;

#[derive(Props, Clone, PartialEq)]
pub(super) struct InsertImageToolProps {
    sheet_index: usize,
    selected: (usize, usize),
    enabled: bool,
    blocked_reason: Option<String>,
}

#[component]
pub(super) fn InsertImageTool(props: InsertImageToolProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();

    #[cfg(feature = "mobile")]
    return rsx! {
        ToolbarIconButton {
            index: 10usize,
            label: "Insert image",
            tooltip: props.blocked_reason.clone(),
            disabled: store.busy() || !props.enabled,
            on_click: {
                let ports = Rc::clone(&ports);
                move |_| {
                    let ports = Rc::clone(&ports);
                    spawn(async move {
                        match ports
                            .files
                            .pick_file(crate::ports::file::MobileFileKind::Image)
                            .await
                        {
                            Ok(Some(file)) => {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    actions::MutationIntent::InsertImage {
                                        sheet_index: props.sheet_index,
                                        row: props.selected.0 as u32,
                                        col: props.selected.1 as u32,
                                        file_name: file.name,
                                        bytes: file.bytes,
                                    },
                                )
                                .await;
                            }
                            Ok(None) => {}
                            Err(error) => store.set_error(error),
                        }
                    });
                }
            },
            ImagePlus { size: 18 }
        }
    };

    #[cfg(not(feature = "mobile"))]
    rsx! {
        Tooltip { disabled: cfg!(feature = "mobile"),
            TooltipTrigger {
                Label {
                    class: "tool-button file-tool",
                    html_for: "insert-workbook-image",
                    aria_label: "Insert image",
                    title: props.blocked_reason.as_deref().unwrap_or("Insert image"),
                    ImagePlus { size: 18 }
                    input {
                        id: "insert-workbook-image",
                        class: "visually-hidden",
                        r#type: "file",
                        disabled: store.busy() || !props.enabled,
                        accept: "image/png,image/jpeg",
                        onchange: {
                            let ports = Rc::clone(&ports);
                            move |event: Event<FormData>| {
                                let Some(file) = event.files().into_iter().next() else { return; };
                                let ports = Rc::clone(&ports);
                                spawn(async move {
                                    match file.read_bytes().await {
                                        Ok(bytes) => {
                                            actions::run_mutation(
                                                store,
                                                ports,
                                                actions::MutationIntent::InsertImage {
                                                    sheet_index: props.sheet_index,
                                                    row: props.selected.0 as u32,
                                                    col: props.selected.1 as u32,
                                                    file_name: file.name(),
                                                    bytes: bytes.to_vec(),
                                                },
                                            )
                                            .await;
                                        }
                                        Err(error) => store.set_error(crate::protocol::AppErrorDto {
                                            code: "read_error".to_string(),
                                            message: error.to_string(),
                                        }),
                                    }
                                });
                            }
                        }
                    }
                }
            }
            TooltipContent {
                side: ContentSide::Bottom,
                {props.blocked_reason.as_deref().unwrap_or("Insert image")}
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub(super) struct ImageToolsProps {
    image: SheetImageDto,
    sheet_index: usize,
    selected: (usize, usize),
}

#[component]
pub(super) fn ImageTools(props: ImageToolsProps) -> Element {
    const EMU_PER_PIXEL: u32 = 9_525;

    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let image_capabilities = store
        .document
        .read()
        .as_ref()
        .map(|document| document.editor_session.capabilities.rich.images.clone())
        .unwrap_or_default();
    let blocked_reason = image_capabilities
        .blocked_reasons
        .first()
        .cloned()
        .unwrap_or_else(|| "Image changes are unavailable for this workbook".to_string());
    let (from, width_emu, height_emu) = match &props.image.anchor {
        ImageAnchorDto::OneCell {
            from,
            width_emu,
            height_emu,
        } => (from.clone(), *width_emu, *height_emu),
        ImageAnchorDto::TwoCell { from, .. } => (
            from.clone(),
            props.image.intrinsic_width.saturating_mul(EMU_PER_PIXEL),
            props.image.intrinsic_height.saturating_mul(EMU_PER_PIXEL),
        ),
    };
    let width_px = (width_emu / EMU_PER_PIXEL).max(1);
    let height_px = (height_emu / EMU_PER_PIXEL).max(1);

    rsx! {
        div { class: "image-tools",
            Button {
                class: "tool-button",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Move image to selected cell",
                title: (!image_capabilities.can_move_resize).then_some(blocked_reason.as_str()),
                disabled: store.busy() || !image_capabilities.can_move_resize,
                onclick: {
                    let ports = Rc::clone(&ports);
                    let image_id = props.image.id.clone();
                    move |_| {
                        let ports = Rc::clone(&ports);
                        let image_id = image_id.clone();
                        spawn(async move {
                            actions::run_mutation(
                                store,
                                ports,
                                actions::MutationIntent::UpdateImage {
                                    sheet_index: props.sheet_index,
                                    image_id,
                                    anchor: ImageAnchorDto::OneCell {
                                        from: ImageMarkerDto {
                                            row: props.selected.0 as u32,
                                            col: props.selected.1 as u32,
                                            row_offset_emu: 0,
                                            col_offset_emu: 0,
                                        },
                                        width_emu,
                                        height_emu,
                                    },
                                },
                            )
                            .await;
                        });
                    }
                },
                Move { size: 17 }
            }
            Button {
                class: "tool-button",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Delete image",
                title: (!image_capabilities.can_delete).then_some(blocked_reason.as_str()),
                disabled: store.busy() || !image_capabilities.can_delete,
                onclick: {
                    let ports = Rc::clone(&ports);
                    let image_id = props.image.id.clone();
                    move |_| {
                        let ports = Rc::clone(&ports);
                        let image_id = image_id.clone();
                        spawn(async move {
                            actions::run_mutation(
                                store,
                                ports,
                                actions::MutationIntent::DeleteImage {
                                    sheet_index: props.sheet_index,
                                    image_id,
                                },
                            )
                            .await;
                        });
                    }
                },
                Trash2 { size: 17 }
            }
            Label { html_for: "selected-image-width", title: "Image width",
                span { "W" }
                Input {
                    id: "selected-image-width",
                    r#type: "number",
                    min: 24,
                    max: 2000,
                    value: width_px,
                    aria_label: "Image width",
                    title: (!image_capabilities.can_move_resize).then_some(blocked_reason.as_str()),
                    disabled: store.busy() || !image_capabilities.can_move_resize,
                    onchange: {
                        let ports = Rc::clone(&ports);
                        let image_id = props.image.id.clone();
                        let from = from.clone();
                        move |event: Event<FormData>| {
                            let Ok(width) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            let image_id = image_id.clone();
                            let from = from.clone();
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    actions::MutationIntent::UpdateImage {
                                        sheet_index: props.sheet_index,
                                        image_id,
                                        anchor: ImageAnchorDto::OneCell {
                                            from,
                                            width_emu: width.clamp(24, 2000).saturating_mul(EMU_PER_PIXEL),
                                            height_emu,
                                        },
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
            Label { html_for: "selected-image-height", title: "Image height",
                span { "H" }
                Input {
                    id: "selected-image-height",
                    r#type: "number",
                    min: 24,
                    max: 2000,
                    value: height_px,
                    aria_label: "Image height",
                    title: (!image_capabilities.can_move_resize).then_some(blocked_reason.as_str()),
                    disabled: store.busy() || !image_capabilities.can_move_resize,
                    onchange: {
                        let ports = Rc::clone(&ports);
                        let image_id = props.image.id.clone();
                        move |event: Event<FormData>| {
                            let Ok(height) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            let image_id = image_id.clone();
                            let from = from.clone();
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    actions::MutationIntent::UpdateImage {
                                        sheet_index: props.sheet_index,
                                        image_id,
                                        anchor: ImageAnchorDto::OneCell {
                                            from,
                                            width_emu,
                                            height_emu: height.clamp(24, 2000).saturating_mul(EMU_PER_PIXEL),
                                        },
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
        }
    }
}
