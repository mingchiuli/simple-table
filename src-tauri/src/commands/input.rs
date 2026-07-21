use crate::editor_protocol::{MAX_CELL_TEXT_BYTES, MAX_MUTATION_TEXT_BYTES, MAX_SET_CELL_CHANGES};
use crate::types::SetCellRequest;

#[derive(Debug)]
pub(crate) struct BoundedCellText(String);

impl BoundedCellText {
    pub(crate) fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for BoundedCellText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct CellTextVisitor;

        impl serde::de::Visitor<'_> for CellTextVisitor {
            type Value = BoundedCellText;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    formatter,
                    "cell text containing at most {MAX_CELL_TEXT_BYTES} bytes"
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                validate_cell_text_bytes(value.len()).map(|()| BoundedCellText(value.to_string()))
            }

            fn visit_borrowed_str<E>(self, value: &'_ str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                validate_cell_text_bytes(value.len()).map(|()| BoundedCellText(value))
            }
        }

        fn validate_cell_text_bytes<E>(byte_count: usize) -> Result<(), E>
        where
            E: serde::de::Error,
        {
            if byte_count <= MAX_CELL_TEXT_BYTES {
                return Ok(());
            }
            Err(E::custom(format!(
                "cell text requires {byte_count} bytes; the maximum is {MAX_CELL_TEXT_BYTES} bytes"
            )))
        }

        deserializer.deserialize_string(CellTextVisitor)
    }
}

#[derive(Debug)]
pub(crate) struct SetCellBatch(Vec<SetCellRequest>);

impl SetCellBatch {
    pub(crate) fn into_inner(self) -> Vec<SetCellRequest> {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for SetCellBatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BatchVisitor;

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct BoundedSetCellRequest {
            sheet_index: usize,
            row: usize,
            col: usize,
            text: BoundedCellText,
        }

        impl From<BoundedSetCellRequest> for SetCellRequest {
            fn from(request: BoundedSetCellRequest) -> Self {
                Self {
                    sheet_index: request.sheet_index,
                    row: request.row,
                    col: request.col,
                    text: request.text.into_inner(),
                }
            }
        }

        impl<'de> serde::de::Visitor<'de> for BatchVisitor {
            type Value = SetCellBatch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    formatter,
                    "an array containing at most {MAX_SET_CELL_CHANGES} cell changes"
                )
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut changes = Vec::with_capacity(
                    sequence
                        .size_hint()
                        .unwrap_or_default()
                        .min(MAX_SET_CELL_CHANGES),
                );
                let mut text_bytes = 0usize;
                while changes.len() < MAX_SET_CELL_CHANGES {
                    let Some(change) = sequence.next_element::<BoundedSetCellRequest>()? else {
                        return Ok(SetCellBatch(changes));
                    };
                    text_bytes = text_bytes.checked_add(change.text.0.len()).ok_or_else(|| {
                        serde::de::Error::custom("set_cells text bytes overflowed")
                    })?;
                    if text_bytes > MAX_MUTATION_TEXT_BYTES {
                        return Err(serde::de::Error::custom(format!(
                            "set_cells text requires {text_bytes} bytes; the maximum is {MAX_MUTATION_TEXT_BYTES} bytes"
                        )));
                    }
                    changes.push(change.into());
                }
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "set_cells accepts at most {MAX_SET_CELL_CHANGES} changes"
                    )));
                }
                Ok(SetCellBatch(changes))
            }
        }

        deserializer.deserialize_seq(BatchVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BoundedCellText, MAX_CELL_TEXT_BYTES, MAX_MUTATION_TEXT_BYTES, MAX_SET_CELL_CHANGES,
        SetCellBatch,
    };
    use serde_json::{Value, json};

    fn cell_change(index: usize) -> Value {
        json!({
            "sheetIndex": 0,
            "row": index,
            "col": 0,
            "text": ""
        })
    }

    #[test]
    fn set_cell_batch_accepts_the_maximum_number_of_changes() {
        let changes = (0..MAX_SET_CELL_CHANGES).map(cell_change).collect();
        let batch: SetCellBatch =
            serde_json::from_value(Value::Array(changes)).expect("bounded cell batch");

        assert_eq!(batch.0.len(), MAX_SET_CELL_CHANGES);
    }

    #[test]
    fn set_cell_batch_rejects_an_oversized_sequence_during_deserialization() {
        let changes = (0..=MAX_SET_CELL_CHANGES).map(cell_change).collect();
        let error = serde_json::from_value::<SetCellBatch>(Value::Array(changes))
            .expect_err("oversized batch must be rejected");

        assert!(error.to_string().contains("at most 4096 changes"));
    }

    #[test]
    fn set_cell_batch_rejects_a_single_oversized_cell_during_deserialization() {
        let changes = vec![json!({
            "sheetIndex": 0,
            "row": 0,
            "col": 0,
            "text": "x".repeat(MAX_CELL_TEXT_BYTES + 1)
        })];
        let error = serde_json::from_value::<SetCellBatch>(Value::Array(changes))
            .expect_err("oversized cell text must be rejected");

        assert!(error.to_string().contains("cell text requires"));
    }

    #[test]
    fn single_cell_text_uses_the_same_deserialization_limit() {
        let accepted = serde_json::from_value::<BoundedCellText>(Value::String(
            "x".repeat(MAX_CELL_TEXT_BYTES),
        ));
        assert!(accepted.is_ok());

        let error = serde_json::from_value::<BoundedCellText>(Value::String(
            "x".repeat(MAX_CELL_TEXT_BYTES + 1),
        ))
        .expect_err("oversized single cell text");
        assert!(error.to_string().contains("cell text requires"));
    }

    #[test]
    fn set_cell_batch_rejects_aggregate_text_during_deserialization() {
        let changes = vec![
            json!({ "sheetIndex": 0, "row": 0, "col": 0, "text": "x".repeat(MAX_CELL_TEXT_BYTES) }),
            json!({ "sheetIndex": 0, "row": 1, "col": 0, "text": "x".repeat(MAX_CELL_TEXT_BYTES) }),
            json!({ "sheetIndex": 0, "row": 2, "col": 0, "text": "x" }),
        ];
        let error = serde_json::from_value::<SetCellBatch>(Value::Array(changes))
            .expect_err("oversized aggregate text must be rejected");

        assert!(
            error
                .to_string()
                .contains(&format!("maximum is {MAX_MUTATION_TEXT_BYTES} bytes"))
        );
    }
}
