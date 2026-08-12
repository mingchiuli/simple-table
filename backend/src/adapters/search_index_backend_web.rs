use crate::domain::SearchCellText;
use crate::error::AppError;

pub(crate) trait SearchIndexReader: Send + Sync {
    fn search(
        &self,
        literal: &str,
        terms: &[String],
        limit: usize,
    ) -> Result<Vec<SearchCellText>, AppError>;
}

pub(crate) fn tokenize_search_text(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}
