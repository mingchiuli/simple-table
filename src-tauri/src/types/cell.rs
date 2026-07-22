use crate::domain::{
    CellNumber, CellValue as DomainCellValue, format_cell_display, format_cell_search,
    normalize_formula_text,
};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::{Config, TS, TypeVisitor};

const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const JS_MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CellFormatProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellValue(DomainCellValue);

impl CellValue {
    pub(crate) fn as_domain(&self) -> &DomainCellValue {
        &self.0
    }
}

impl From<DomainCellValue> for CellValue {
    fn from(value: DomainCellValue) -> Self {
        Self(value)
    }
}

impl<'de> Deserialize<'de> for CellValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserialize_domain_cell_value(Value::deserialize(deserializer)?)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for CellValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CellValueProjection::new(self.as_domain(), None).serialize(serializer)
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

struct FormulaCellSerializeProjection<'a> {
    formula: &'a str,
    cached_value: &'a DomainCellValue,
    error: Option<&'a str>,
}

impl Serialize for FormulaCellSerializeProjection<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(if self.error.is_some() { 3 } else { 2 }))?;
        map.serialize_entry("formula", self.formula)?;
        map.serialize_entry(
            "cachedValue",
            &CellValueProjection::new(self.cached_value, None),
        )?;
        if let Some(error) = self.error {
            map.serialize_entry("error", error)?;
        }
        map.end()
    }
}

pub(crate) struct CellValueProjection<'a> {
    cell: &'a DomainCellValue,
    format: Option<CellFormatProjection>,
}

impl<'a> CellValueProjection<'a> {
    pub(crate) fn new(cell: &'a DomainCellValue, format: Option<CellFormatProjection>) -> Self {
        Self { cell, format }
    }
}

impl Serialize for CellValueProjection<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut len = 4;
        if matches!(self.cell, DomainCellValue::Formula { .. }) {
            len += 1;
        }
        if self.format.is_some() {
            len += 1;
        }

        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("type", "cell")?;
        map.serialize_entry("kind", self.cell.kind())?;
        map.serialize_entry("raw", &raw_json_value(self.cell))?;
        map.serialize_entry(
            "display",
            &DisplayProjection::display(self.cell, self.format.as_ref()),
        )?;
        if let DomainCellValue::Formula {
            formula,
            cached_value,
            error,
        } = self.cell
        {
            map.serialize_entry(
                "formula",
                &FormulaCellSerializeProjection {
                    formula,
                    cached_value,
                    error: error.as_deref(),
                },
            )?;
        }
        if let Some(format) = &self.format {
            map.serialize_entry("format", format)?;
        }
        map.end()
    }
}

struct DisplayProjection;

impl DisplayProjection {
    fn display(cell: &DomainCellValue, format: Option<&CellFormatProjection>) -> String {
        format_cell_display(
            cell,
            format.and_then(|value| value.number_format.as_deref()),
        )
    }

    #[allow(dead_code)]
    fn search(cell: &DomainCellValue, format: Option<&CellFormatProjection>) -> String {
        format_cell_search(
            cell,
            format.and_then(|value| value.number_format.as_deref()),
        )
    }
}

fn raw_json_value(value: &DomainCellValue) -> Value {
    match value {
        DomainCellValue::Null => Value::Null,
        DomainCellValue::String(value) => Value::String(value.clone()),
        DomainCellValue::Number(value) => {
            if let Some(integer) = value.as_i64()
                && !(JS_MIN_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&integer)
            {
                return Value::String(integer.to_string());
            }
            value.as_i64().map_or_else(
                || {
                    serde_json::Number::from_f64(value.as_f64())
                        .map(Value::Number)
                        .unwrap_or(Value::Null)
                },
                |value| Value::Number(value.into()),
            )
        }
        DomainCellValue::Boolean(value) => Value::Bool(*value),
        DomainCellValue::Formula { cached_value, .. } => raw_json_value(cached_value),
    }
}

fn deserialize_domain_cell_value(value: Value) -> Result<DomainCellValue, String> {
    match value {
        Value::Null => Ok(DomainCellValue::Null),
        Value::Bool(value) => Ok(DomainCellValue::Boolean(value)),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(DomainCellValue::Number(CellNumber::from(integer)))
            } else if let Some(float) = value.as_f64() {
                CellNumber::from_f64(float)
                    .map(DomainCellValue::Number)
                    .ok_or_else(|| "cell number must be finite".to_string())
            } else {
                Ok(DomainCellValue::String(value.to_string()))
            }
        }
        Value::String(value) => Ok(DomainCellValue::String(value)),
        Value::Object(mut object) => deserialize_cell_object(&mut object),
        Value::Array(value) => Ok(DomainCellValue::String(Value::Array(value).to_string())),
    }
}

fn deserialize_cell_object(
    object: &mut serde_json::Map<String, Value>,
) -> Result<DomainCellValue, String> {
    if object.get("type").and_then(Value::as_str) == Some("cell") {
        let formula = object.remove("formula");
        if let Some(Value::Object(mut formula)) = formula {
            return deserialize_formula_object(&mut formula, object.remove("raw"));
        }
        return object
            .remove("raw")
            .map(deserialize_domain_cell_value)
            .transpose()
            .map(|value| value.unwrap_or(DomainCellValue::Null));
    }

    if object.get("type").and_then(Value::as_str) == Some("formula") {
        return deserialize_formula_object(object, None);
    }

    Ok(DomainCellValue::String(
        Value::Object(object.clone()).to_string(),
    ))
}

fn deserialize_formula_object(
    formula: &mut serde_json::Map<String, Value>,
    raw_fallback: Option<Value>,
) -> Result<DomainCellValue, String> {
    let formula_text = formula
        .remove("formula")
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default();
    let cached_value = formula
        .remove("cachedValue")
        .or_else(|| formula.remove("cached_value"))
        .or(raw_fallback)
        .map(deserialize_domain_cell_value)
        .transpose()?
        .unwrap_or(DomainCellValue::Null);
    let error = formula
        .remove("error")
        .and_then(|value| value.as_str().map(ToOwned::to_owned));

    Ok(DomainCellValue::Formula {
        formula: normalize_formula_text(formula_text),
        cached_value: Box::new(cached_value),
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn percent_format() -> CellFormatProjection {
        CellFormatProjection {
            number_format: Some("0%".to_string()),
            style_id: None,
        }
    }

    #[test]
    fn formats_percent_display_text() {
        let cell = DomainCellValue::Number(CellNumber::from_f64(0.4).unwrap());

        assert_eq!(
            DisplayProjection::display(&cell, Some(&percent_format())),
            "40%"
        );
    }

    #[test]
    fn search_text_includes_display_and_raw_number() {
        let cell = DomainCellValue::Number(CellNumber::from_f64(0.4).unwrap());

        assert_eq!(
            DisplayProjection::search(&cell, Some(&percent_format())),
            "40%\n0.4"
        );
    }

    #[test]
    fn wire_cell_value_round_trips_formula_projection() {
        let value = CellValue::from(DomainCellValue::formula(
            "=A1+1",
            DomainCellValue::Number(2.into()),
        ));

        let json = serde_json::to_value(&value).expect("serialize wire value");
        let restored: CellValue = serde_json::from_value(json).expect("deserialize wire value");

        assert_eq!(restored, value);
    }
}
