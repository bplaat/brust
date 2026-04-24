use crate::ast::{File, Item, Stmt};

pub fn generate(file: &File) -> String {
    let mut out = String::new();
    out.push_str("#include <stdio.h>\n\n");
    for item in &file.items {
        match item {
            Item::Fn(f) => {
                // Only main gets special C signature for now
                if f.name == "main" {
                    out.push_str("int main(void) {\n");
                } else {
                    out.push_str(&format!("void {}(void) {{\n", f.name));
                }
                for stmt in &f.body.stmts {
                    match stmt {
                        Stmt::Println(text) => {
                            // Escape the string for C
                            let escaped = c_escape(text);
                            out.push_str(&format!("    printf(\"{escaped}\\n\");\n"));
                        }
                    }
                }
                if f.name == "main" {
                    out.push_str("    return 0;\n");
                }
                out.push_str("}\n");
            }
        }
    }
    out
}

/// Escape a string so it is safe inside a C double-quoted string.
fn c_escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}
