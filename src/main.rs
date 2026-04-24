mod ast;
mod codegen;
mod error;
mod lexer;
mod parser;

use std::{env, fs, path::Path, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: brust <file.br>");
        process::exit(1);
    }

    let input_path = &args[1];
    let src = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read '{}': {}", input_path, e);
        process::exit(1);
    });

    // Lex
    let tokens = lexer::Lexer::new(&src).tokenize().unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    // Parse
    let file = parser::Parser::new(tokens).parse().unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    // Codegen
    let c_src = codegen::generate(&file);

    // Write output: same name, .c extension
    let out_path = Path::new(input_path).with_extension("c");
    fs::write(&out_path, &c_src).unwrap_or_else(|e| {
        eprintln!("error: cannot write '{}': {}", out_path.display(), e);
        process::exit(1);
    });

    println!("wrote {}", out_path.display());
}
