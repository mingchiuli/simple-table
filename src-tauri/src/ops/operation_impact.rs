use crate::domain::{AppliedOperation, MutationImpact};

impl MutationImpact<'_> {
    pub fn is_noop(&self) -> bool {
        match self.operation {
            AppliedOperation::SetCell {
                old_value,
                new_value,
                ..
            } => old_value == new_value,
            AppliedOperation::SetCells { changes } => changes
                .iter()
                .all(|change| change.old_value == change.new_value),
            AppliedOperation::SetColumnWidth {
                old_width,
                new_width,
                ..
            } => old_width == new_width,
            AppliedOperation::SetRowHeight {
                old_height,
                new_height,
                ..
            } => old_height == new_height,
            AppliedOperation::UpdateImage {
                old_image,
                new_image,
                ..
            } => old_image == new_image,
            _ => false,
        }
    }

    pub fn requires_search_rebuild(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddRow { .. }
                | AppliedOperation::DeleteRow { .. }
                | AppliedOperation::AddColumn { .. }
                | AppliedOperation::DeleteColumn { .. }
                | AppliedOperation::AddSheet { .. }
                | AppliedOperation::DeleteSheet { .. }
        )
    }

    pub fn is_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddRow { .. }
                | AppliedOperation::DeleteRow { .. }
                | AppliedOperation::AddColumn { .. }
                | AppliedOperation::DeleteColumn { .. }
                | AppliedOperation::AddSheet { .. }
                | AppliedOperation::DeleteSheet { .. }
        )
    }

    pub fn is_row_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddRow { .. } | AppliedOperation::DeleteRow { .. }
        )
    }

    pub fn is_column_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddColumn { .. } | AppliedOperation::DeleteColumn { .. }
        )
    }

    pub fn is_sheet_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. }
        )
    }

    pub fn is_layout_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. }
        )
    }

    pub fn is_cell_edit(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::SetCell { .. } | AppliedOperation::SetCells { .. }
        )
    }

    pub fn is_image_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::InsertImage { .. }
                | AppliedOperation::UpdateImage { .. }
                | AppliedOperation::DeleteImage { .. }
        )
    }
}
