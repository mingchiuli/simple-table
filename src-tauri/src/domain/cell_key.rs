pub(crate) fn parse_cell_key(key: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut saw_digit = false;
    for byte in key.bytes() {
        if byte.is_ascii_alphabetic() && !saw_digit {
            col = col
                .checked_mul(26)?
                .checked_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1))?;
        } else if byte.is_ascii_digit() {
            saw_digit = true;
            row = row.checked_mul(10)?.checked_add(usize::from(byte - b'0'))?;
        } else {
            return None;
        }
    }
    (col > 0 && row > 0).then_some((row - 1, col - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_excel_cell_keys() {
        assert_eq!(parse_cell_key("A1"), Some((0, 0)));
        assert_eq!(parse_cell_key("AA12"), Some((11, 26)));
        assert_eq!(parse_cell_key("1A"), None);
    }
}
