use std::collections::HashMap;

use formualizer_parse::parser::{ASTNode, ASTNodeType, BatchParser, CollectPolicy, ReferenceType};

const MAX_AST_CACHE_ENTRIES: usize = 4096;

#[derive(Clone)]
enum FormulaParseEntry {
    Parsed(ASTNode),
    Error(String),
}

pub(crate) struct FormulaAstService {
    parser: BatchParser,
    parsed_cache: HashMap<String, FormulaParseEntry>,
}

pub(crate) struct ParsedFormula {
    source: FormulaSource,
    ast: ASTNode,
}

#[derive(Clone)]
pub(crate) struct FormulaTextEdit {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) replacement: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FormulaSource {
    original: String,
    parsed: String,
    added_equals: bool,
}

impl Default for FormulaAstService {
    fn default() -> Self {
        Self::new()
    }
}

impl FormulaAstService {
    pub(crate) fn new() -> Self {
        Self {
            parser: BatchParser::builder()
                .with_volatility_classifier(is_volatile_function)
                .build(),
            parsed_cache: HashMap::new(),
        }
    }

    pub(crate) fn parse(&mut self, formula: &str) -> Result<ParsedFormula, String> {
        let source = FormulaSource::new(formula);
        let ast = self.parse_ast(source.parsed())?;
        Ok(ParsedFormula { source, ast })
    }

    fn parse_ast(&mut self, formula: &str) -> Result<ASTNode, String> {
        if let Some(entry) = self.parsed_cache.get(formula) {
            return match entry {
                FormulaParseEntry::Parsed(ast) => Ok(ast.clone()),
                FormulaParseEntry::Error(error) => Err(error.clone()),
            };
        }

        if self.parsed_cache.len() >= MAX_AST_CACHE_ENTRIES {
            self.parsed_cache.clear();
        }

        match self.parser.parse(formula) {
            Ok(ast) => {
                self.parsed_cache
                    .insert(formula.to_string(), FormulaParseEntry::Parsed(ast.clone()));
                Ok(ast)
            }
            Err(error) => {
                let error = error.to_string();
                self.parsed_cache
                    .insert(formula.to_string(), FormulaParseEntry::Error(error.clone()));
                Err(error)
            }
        }
    }
}

impl ParsedFormula {
    pub(crate) fn source(&self) -> &FormulaSource {
        &self.source
    }

    pub(crate) fn contains_volatile(&self) -> bool {
        self.ast.contains_volatile()
    }

    pub(crate) fn references(&self) -> Vec<ReferenceType> {
        self.ast
            .collect_references(&CollectPolicy::default())
            .into_iter()
            .collect()
    }

    pub(crate) fn collect_reference_nodes<'a>(&'a self, nodes: &mut Vec<&'a ASTNode>) {
        collect_reference_nodes(&self.ast, nodes);
    }
}

impl FormulaSource {
    fn new(original: &str) -> Self {
        let added_equals = !original.starts_with('=');
        let parsed = if added_equals {
            format!("={original}")
        } else {
            original.to_string()
        };
        Self {
            original: original.to_string(),
            parsed,
            added_equals,
        }
    }

    fn parsed(&self) -> &str {
        &self.parsed
    }

    #[cfg(test)]
    pub(crate) fn original(&self) -> &str {
        &self.original
    }

    pub(crate) fn original_span(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let offset = usize::from(self.added_equals);
        let start = start.checked_sub(offset)?;
        let end = end.checked_sub(offset)?;
        if start >= end
            || end > self.original.len()
            || !self.original.is_char_boundary(start)
            || !self.original.is_char_boundary(end)
        {
            return None;
        }
        Some((start, end))
    }
}

pub(crate) fn apply_formula_text_edits(
    source: &str,
    mut edits: Vec<FormulaTextEdit>,
) -> Option<String> {
    if edits.is_empty() {
        return Some(source.to_string());
    }

    edits.sort_by_key(|edit| edit.start);
    let mut previous_end = 0;
    for edit in &edits {
        if edit.start < previous_end
            || edit.start >= edit.end
            || edit.end > source.len()
            || !source.is_char_boundary(edit.start)
            || !source.is_char_boundary(edit.end)
        {
            return None;
        }
        previous_end = edit.end;
    }

    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        output.push_str(&source[cursor..edit.start]);
        output.push_str(&edit.replacement);
        cursor = edit.end;
    }
    output.push_str(source.get(cursor..)?);
    Some(output)
}

fn collect_reference_nodes<'a>(ast: &'a ASTNode, nodes: &mut Vec<&'a ASTNode>) {
    match &ast.node_type {
        ASTNodeType::Reference { .. } => nodes.push(ast),
        ASTNodeType::UnaryOp { expr, .. } => collect_reference_nodes(expr, nodes),
        ASTNodeType::BinaryOp { left, right, .. } => {
            collect_reference_nodes(left, nodes);
            collect_reference_nodes(right, nodes);
        }
        ASTNodeType::Function { args, .. } => {
            for arg in args {
                collect_reference_nodes(arg, nodes);
            }
        }
        ASTNodeType::Call { callee, args } => {
            collect_reference_nodes(callee, nodes);
            for arg in args {
                collect_reference_nodes(arg, nodes);
            }
        }
        ASTNodeType::Array(rows) => {
            for row in rows {
                for item in row {
                    collect_reference_nodes(item, nodes);
                }
            }
        }
        ASTNodeType::Literal(_) => {}
    }
}

fn is_volatile_function(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "OFFSET" | "INDIRECT" | "INFO" | "CELL"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formulas_with_or_without_leading_equals() {
        let mut service = FormulaAstService::new();

        assert!(service.parse("=A1+1").is_ok());
        assert!(service.parse("A1+1").is_ok());
    }

    #[test]
    fn exposes_original_spans_when_equals_was_added() {
        let mut service = FormulaAstService::new();
        let parsed = service.parse("Inputs!A1+1").expect("parse formula");
        let mut references = Vec::new();
        parsed.collect_reference_nodes(&mut references);
        let token = references[0]
            .source_token
            .as_ref()
            .expect("reference token");

        assert_eq!(
            parsed.source().original_span(token.start, token.end),
            Some((0, 9))
        );
        assert_eq!(&parsed.source().original()[0..9], "Inputs!A1");
    }

    #[test]
    fn classifies_volatile_formulas() {
        let mut service = FormulaAstService::new();

        assert!(
            service
                .parse("=NOW()")
                .expect("parse formula")
                .contains_volatile()
        );
        assert!(
            !service
                .parse("=A1+1")
                .expect("parse formula")
                .contains_volatile()
        );
    }
}
