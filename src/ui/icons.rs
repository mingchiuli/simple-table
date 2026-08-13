use dioxus::prelude::*;
use lucide_icons::Icon;

macro_rules! lucide_icon {
    ($name:ident, $variant:ident) => {
        #[component]
        pub fn $name(#[props(default = 24)] size: u16) -> Element {
            let glyph = char::from(Icon::$variant);
            rsx! {
                span {
                    class: "lucide-icon",
                    style: "--lucide-icon-size: {size}px",
                    aria_hidden: "true",
                    "{glyph}"
                }
            }
        }
    };
}

lucide_icon!(Columns3, Columns3);
lucide_icon!(Download, Download);
lucide_icon!(ExternalLink, ExternalLink);
lucide_icon!(FilePlus, FilePlus);
lucide_icon!(FolderOpen, FolderOpen);
lucide_icon!(FunctionSquare, FunctionSquare);
lucide_icon!(Grid2X2Plus, Grid2X2Plus);
lucide_icon!(HardDriveDownload, HardDriveDownload);
lucide_icon!(House, House);
lucide_icon!(ImagePlus, ImagePlus);
lucide_icon!(Move, Move);
lucide_icon!(Plus, Plus);
lucide_icon!(Redo2, Redo2);
lucide_icon!(Rows3, Rows3);
lucide_icon!(Save, Save);
lucide_icon!(Search, Search);
lucide_icon!(Sheet, Sheet);
lucide_icon!(Trash2, Trash2);
lucide_icon!(Undo2, Undo2);
lucide_icon!(X, X);
