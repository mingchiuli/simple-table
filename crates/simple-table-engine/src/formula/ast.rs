use std::collections::HashMap;

use formualizer_parse::parser::{ASTNode, ASTNodeType, BatchParser, CollectPolicy, ReferenceType};

pub(crate) const MAX_FORMULA_BYTES: usize = 64 * 1024;
pub(crate) const MAX_FORMULA_NESTING_DEPTH: usize = 128;
pub(crate) const MAX_FORMULA_AST_NODES: usize = 4_096;
pub(crate) const MAX_FORMULA_REFERENCES: usize = 1_024;
const MAX_AST_CACHE_ENTRIES: usize = 4_096;
const MAX_AST_CACHE_BYTES: usize = 8 * 1024 * 1024;
const AST_NODE_ESTIMATED_BYTES: usize = 128;
const MAX_FORMULA_ERROR_BYTES: usize = 1_024;

#[derive(Clone)]
enum FormulaParseEntry {
    Parsed(ASTNode),
    Error(String),
}

struct CachedFormulaParse {
    value: FormulaParseEntry,
    estimated_bytes: usize,
    last_used: u64,
}

pub(crate) struct FormulaAstService {
    parser: BatchParser,
    parsed_cache: HashMap<String, CachedFormulaParse>,
    cache_estimated_bytes: usize,
    cache_clock: u64,
}

pub(crate) struct ParsedFormula {
    source: FormulaSource,
    ast: ASTNode,
    reference_count: usize,
}

#[derive(Clone, Copy)]
struct AstMetrics {
    node_count: usize,
    reference_count: usize,
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
            cache_estimated_bytes: 0,
            cache_clock: 0,
        }
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(self.cache_estimated_bytes)
    }

    pub(crate) fn parse(&mut self, formula: &str) -> Result<ParsedFormula, String> {
        validate_formula_source(formula)?;
        let source = FormulaSource::new(formula);
        let ast = self.parse_ast(source.parsed())?;
        let metrics = ast_metrics(&ast)?;
        Ok(ParsedFormula {
            source,
            ast,
            reference_count: metrics.reference_count,
        })
    }

    fn parse_ast(&mut self, formula: &str) -> Result<ASTNode, String> {
        let clock = self.next_cache_clock();
        if let Some(entry) = self.parsed_cache.get_mut(formula) {
            entry.last_used = clock;
            return match &entry.value {
                FormulaParseEntry::Parsed(ast) => Ok(ast.clone()),
                FormulaParseEntry::Error(error) => Err(error.clone()),
            };
        }

        match self.parser.parse(formula) {
            Ok(ast) => {
                let metrics = ast_metrics(&ast)?;
                self.cache_parse_result(
                    formula,
                    FormulaParseEntry::Parsed(ast.clone()),
                    formula.len().saturating_mul(2).saturating_add(
                        metrics.node_count.saturating_mul(AST_NODE_ESTIMATED_BYTES),
                    ),
                    clock,
                );
                Ok(ast)
            }
            Err(error) => {
                let error = truncate_utf8(&error.to_string(), MAX_FORMULA_ERROR_BYTES);
                self.cache_parse_result(
                    formula,
                    FormulaParseEntry::Error(error.clone()),
                    formula.len().saturating_mul(2).saturating_add(error.len()),
                    clock,
                );
                Err(error)
            }
        }
    }

    fn cache_parse_result(
        &mut self,
        formula: &str,
        value: FormulaParseEntry,
        estimated_bytes: usize,
        last_used: u64,
    ) {
        let estimated_bytes = estimated_bytes.max(1);
        if estimated_bytes > MAX_AST_CACHE_BYTES {
            return;
        }
        while self.parsed_cache.len() >= MAX_AST_CACHE_ENTRIES
            || self.cache_estimated_bytes.saturating_add(estimated_bytes) > MAX_AST_CACHE_BYTES
        {
            let Some(oldest) = self
                .parsed_cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(formula, _)| formula.clone())
            else {
                break;
            };
            if let Some(removed) = self.parsed_cache.remove(&oldest) {
                self.cache_estimated_bytes = self
                    .cache_estimated_bytes
                    .saturating_sub(removed.estimated_bytes);
            }
        }
        self.parsed_cache.insert(
            formula.to_string(),
            CachedFormulaParse {
                value,
                estimated_bytes,
                last_used,
            },
        );
        self.cache_estimated_bytes = self.cache_estimated_bytes.saturating_add(estimated_bytes);
    }

    fn next_cache_clock(&mut self) -> u64 {
        let Some(next) = self.cache_clock.checked_add(1) else {
            self.parsed_cache.clear();
            self.cache_estimated_bytes = 0;
            self.cache_clock = 1;
            return self.cache_clock;
        };
        self.cache_clock = next;
        next
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

    pub(crate) fn reference_count(&self) -> usize {
        self.reference_count
    }

    pub(crate) fn collect_reference_nodes<'a>(&'a self, nodes: &mut Vec<&'a ASTNode>) {
        collect_reference_nodes(&self.ast, nodes);
    }
}

fn validate_formula_source(formula: &str) -> Result<(), String> {
    if formula.len() > MAX_FORMULA_BYTES {
        return Err(format!(
            "formula requires {} bytes; the maximum is {MAX_FORMULA_BYTES} bytes",
            formula.len()
        ));
    }

    let mut nesting_depth = 0usize;
    let mut in_double_quote = false;
    let mut in_single_quote = false;
    let mut chars = formula.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if !in_single_quote => {
                if in_double_quote && chars.peek() == Some(&'"') {
                    chars.next();
                } else {
                    in_double_quote = !in_double_quote;
                }
            }
            '\'' if !in_double_quote => {
                if in_single_quote && chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_single_quote = !in_single_quote;
                }
            }
            '(' | '{' if !in_double_quote && !in_single_quote => {
                nesting_depth = nesting_depth.saturating_add(1);
                if nesting_depth > MAX_FORMULA_NESTING_DEPTH {
                    return Err(format!(
                        "formula nesting exceeds the maximum depth of {MAX_FORMULA_NESTING_DEPTH}"
                    ));
                }
            }
            ')' | '}' if !in_double_quote && !in_single_quote => {
                nesting_depth = nesting_depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    Ok(())
}

fn ast_metrics(ast: &ASTNode) -> Result<AstMetrics, String> {
    let mut node_count = 0usize;
    let mut reference_count = 0usize;
    let mut pending = vec![ast];
    while let Some(node) = pending.pop() {
        node_count = node_count.saturating_add(1);
        if node_count > MAX_FORMULA_AST_NODES {
            return Err(format!(
                "formula contains more than {MAX_FORMULA_AST_NODES} syntax nodes"
            ));
        }
        match &node.node_type {
            ASTNodeType::Reference { .. } => {
                reference_count = reference_count.saturating_add(1);
            }
            ASTNodeType::UnaryOp { expr, .. } => pending.push(expr),
            ASTNodeType::BinaryOp { left, right, .. } => {
                pending.push(left);
                pending.push(right);
            }
            ASTNodeType::Function { args, .. } => pending.extend(args.iter()),
            ASTNodeType::Call { callee, args } => {
                pending.push(callee);
                pending.extend(args.iter());
            }
            ASTNodeType::Array(rows) => pending.extend(rows.iter().flat_map(|row| row.iter())),
            ASTNodeType::Literal(_) | ASTNodeType::Omitted => {}
        }
    }
    Ok(AstMetrics {
        node_count,
        reference_count,
    })
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_string();
    }
    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
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
        ASTNodeType::Literal(_) | ASTNodeType::Omitted => {}
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

    #[test]
    fn rejects_formula_bytes_and_nesting_before_parsing() {
        let mut service = FormulaAstService::new();

        let byte_error = match service.parse(&format!("={}", "x".repeat(MAX_FORMULA_BYTES))) {
            Ok(_) => panic!("oversized formula must fail"),
            Err(error) => error,
        };
        assert!(byte_error.contains("maximum"));

        let nested = format!(
            "={}1{}",
            "(".repeat(MAX_FORMULA_NESTING_DEPTH + 1),
            ")".repeat(MAX_FORMULA_NESTING_DEPTH + 1)
        );
        let depth_error = match service.parse(&nested) {
            Ok(_) => panic!("deeply nested formula must fail"),
            Err(error) => error,
        };
        assert!(depth_error.contains("nesting"));
        assert!(service.parsed_cache.is_empty());
    }

    #[test]
    fn ast_cache_remains_within_its_byte_budget() {
        let mut service = FormulaAstService::new();
        let payload = "x".repeat(MAX_FORMULA_BYTES / 2);
        for index in 0..(MAX_AST_CACHE_BYTES / payload.len() + 16) {
            let _ = service.parse(&format!("=\"{index}-{payload}\""));
        }

        assert!(service.cache_estimated_bytes <= MAX_AST_CACHE_BYTES);
        assert!(service.parsed_cache.len() < MAX_AST_CACHE_ENTRIES);
    }
}
