use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageMarker {
    pub row: u32,
    pub col: u32,
    pub row_offset_emu: i32,
    pub col_offset_emu: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum ImageAnchor {
    OneCell {
        from: ImageMarker,
        #[serde(rename = "widthEmu")]
        width_emu: u32,
        #[serde(rename = "heightEmu")]
        height_emu: u32,
    },
    TwoCell {
        from: ImageMarker,
        to: ImageMarker,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetImage {
    pub id: String,
    pub media_id: String,
    pub mime_type: String,
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
    pub anchor: ImageAnchor,
    pub z_index: usize,
    pub renderable: bool,
}
