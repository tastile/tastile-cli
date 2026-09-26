//! Output formatting helpers.

/// Print a row-aligned table. The CLI uses this for the human-readable
/// output of `tiles`, `today`, `prompts list`, and `source-tiles list`.
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    if rows.is_empty() {
        println!("(no rows)");
        return;
    }
    let widths = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let col_width = rows
                .iter()
                .map(|r| r.get(i).map(String::len).unwrap_or(0))
                .max()
                .unwrap_or(0);
            h.len().max(col_width)
        })
        .collect::<Vec<_>>();

    let sep = widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{sep}");
    let header_line = headers
        .iter()
        .zip(&widths)
        .map(|(h, w)| format!("{h:<w$}"))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{header_line}");
    println!("{sep}");
    for row in rows {
        let line = row
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    }
    println!("{sep}");
}
