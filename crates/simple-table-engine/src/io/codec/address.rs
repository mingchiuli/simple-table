pub(crate) fn coordinate(col: u32, row: u32) -> String {
    let mut col_num = col;
    let mut letters = String::new();
    while col_num > 0 {
        let rem = ((col_num - 1) % 26) as u8;
        letters.insert(0, (b'A' + rem) as char);
        col_num = (col_num - 1) / 26;
    }
    format!("{letters}{row}")
}
