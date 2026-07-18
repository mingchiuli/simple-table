use crate::types::display::DisplayProjection;
use serde::Serialize;
use serde::ser::SerializeMap;
use ts_rs::{Config, TS, TypeVisitor};

use super::types::{CellFormatProjection, CellValue};

pub struct ScalarCellValue;

#[allow(dead_code)]
#[derive(TS)]
#[ts(rename_all = "camelCase")]
pub struct CellFormulaProjection {
    pub formula: String,
    pub cached_value: CellValue,
    #[ts(optional)]
    pub error: Option<String>,
}

#[allow(dead_code)]
#[derive(TS)]
#[ts(rename_all = "camelCase")]
pub enum CellKind {
    Blank,
    Text,
    Number,
    Boolean,
    Formula,
    Error,
}

#[allow(dead_code)]
#[derive(TS)]
#[ts(rename_all = "camelCase")]
pub struct CellData {
    #[ts(rename = "type", type = "\"cell\"")]
    pub cell_type: String,
    pub kind: CellKind,
    pub raw: ScalarCellValue,
    pub display: String,
    #[ts(optional)]
    pub formula: Option<CellFormulaProjection>,
    #[ts(optional)]
    pub format: Option<CellFormatProjection>,
}

impl TS for ScalarCellValue {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &Config) -> String {
        "ScalarCellValue".to_string()
    }

    fn decl(_: &Config) -> String {
        "type ScalarCellValue = string | number | boolean | null;".to_string()
    }

    fn inline(_: &Config) -> String {
        "ScalarCellValue".to_string()
    }
}

impl TS for CellValue {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &Config) -> String {
        "CellValue".to_string()
    }

    fn decl(_: &Config) -> String {
        "type CellValue = CellData;".to_string()
    }

    fn inline(_: &Config) -> String {
        "CellData".to_string()
    }

    fn visit_dependencies(visitor: &mut impl TypeVisitor)
    where
        Self: 'static,
    {
        visitor.visit::<CellData>();
    }
}

impl Serialize for CellValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CellValueProjection::new(self, None).serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FormulaCellSerializeProjection<'a> {
    formula: &'a str,
    cached_value: &'a CellValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

pub(crate) struct CellValueProjection<'a> {
    cell: &'a CellValue,
    format: Option<CellFormatProjection>,
}

impl<'a> CellValueProjection<'a> {
    pub(crate) fn new(cell: &'a CellValue, format: Option<CellFormatProjection>) -> Self {
        Self { cell, format }
    }
}

impl Serialize for CellValueProjection<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut len = 4;
        if matches!(self.cell, CellValue::Formula { .. }) {
            len += 1;
        }
        if self.format.is_some() {
            len += 1;
        }

        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("type", "cell")?;
        map.serialize_entry("kind", self.cell.kind())?;
        map.serialize_entry("raw", &self.cell.raw_json_value())?;
        map.serialize_entry(
            "display",
            &DisplayProjection::display_text(self.cell, self.format.as_ref()),
        )?;
        if let Some(format) = &self.format {
            map.serialize_entry("format", format)?;
        }
        if let CellValue::Formula {
            formula,
            cached_value,
            error,
        } = self.cell
        {
            let formula_projection = FormulaCellSerializeProjection {
                formula,
                cached_value,
                error: error.as_deref(),
            };
            map.serialize_entry("formula", &formula_projection)?;
        }
        map.end()
    }
}
