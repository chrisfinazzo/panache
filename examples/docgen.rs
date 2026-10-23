//! Regenerate the rule catalog with `cargo run --example docgen`.

use std::{fs, io};

fn main() -> io::Result<()> {
    let path = "docs/reference/_linter-rules.qmd";
    let catalog = panache::linter::docs::render_catalog()?;
    if !fs::read_to_string(path).is_ok_and(|current| current == catalog) {
        fs::write(path, catalog)?;
        println!("wrote {path}");
    }
    Ok(())
}
