pub(crate) mod cell_key;
mod editor_operation;
pub(crate) mod resource_limits;

pub use editor_operation::{
    AppliedOperation, EditorCommand, MutationImpact, OperationPatchProjector, ProjectionMutation,
    ResolvedCellEdit,
};
