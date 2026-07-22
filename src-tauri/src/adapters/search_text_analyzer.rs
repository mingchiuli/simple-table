use tantivy::tokenizer::{LowerCaser, TextAnalyzer, TokenStream};
use tantivy_jieba::JiebaTokenizer;

pub(crate) fn search_text_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(JiebaTokenizer::new())
        .filter(LowerCaser)
        .build()
}

pub(crate) fn tokenize_search_text(text: &str) -> Vec<String> {
    let mut analyzer = search_text_analyzer();
    let mut stream = analyzer.token_stream(text);
    let mut tokens = Vec::new();
    while stream.advance() {
        let token = stream.token();
        if !token.text.is_empty() {
            tokens.push(token.text.to_lowercase());
        }
    }
    tokens
}
