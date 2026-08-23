use dioxus::prelude::*;
use simple_table_components::{
    ContentSide, ToolbarButton, Tooltip, TooltipContent, TooltipTrigger,
};

#[derive(Props, Clone, PartialEq)]
pub struct ToolbarIconButtonProps {
    pub index: usize,
    pub label: String,
    #[props(default)]
    pub disabled: bool,
    #[props(default)]
    pub active: bool,
    pub on_click: EventHandler<()>,
    pub children: Element,
}

#[component]
pub fn ToolbarIconButton(props: ToolbarIconButtonProps) -> Element {
    let class = if props.active {
        "tool-button active"
    } else {
        "tool-button"
    };
    let trigger_label = props.label.clone();
    let content_label = props.label.clone();
    let children = props.children.clone();
    let on_click = props.on_click;

    rsx! {
        Tooltip { disabled: props.disabled,
            TooltipTrigger {
                r#as: move |attributes: Vec<Attribute>| {
                    let children = children.clone();
                    let label = trigger_label.clone();
                    rsx! {
                        ToolbarButton {
                            attributes,
                            class,
                            index: props.index,
                            disabled: props.disabled,
                            on_click: move || on_click.call(()),
                            aria_label: label,
                            {children}
                        }
                    }
                }
            }
            TooltipContent { side: ContentSide::Bottom, "{content_label}" }
        }
    }
}
