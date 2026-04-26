mod ast;
mod codegen;
mod error;
mod lexer;
mod loc;
mod parser;
mod type_checker;

use std::{env, fs, path::Path, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: brust <file.rs> [-o <output>]");
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
    let source_dir = Path::new(input_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let file = parser::Parser::new(tokens, source_dir)
        .parse_file()
        .unwrap_or_else(|e| {
            eprintln!("{}", e);
            process::exit(1);
        });

    // Type and borrow check
    let tc_errors = type_checker::check(&file);
    if !tc_errors.is_empty() {
        for e in &tc_errors {
            eprintln!("{}", e);
        }
        process::exit(1);
    }

    // Codegen -> C source
    let c_src = codegen::generate(&file);

    // Determine final binary output path.
    let bin_path = if args.len() >= 4 && args[2] == "-o" {
        Path::new(&args[3]).to_path_buf()
    } else {
        Path::new(input_path).with_extension("")
    };

    // Write C to a temporary file next to the binary output.
    let c_path = bin_path.with_extension("c");
    fs::write(&c_path, &c_src).unwrap_or_else(|e| {
        eprintln!("error: cannot write '{}': {}", c_path.display(), e);
        process::exit(1);
    });

    // Compile C -> binary using $CC or cc.
    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = process::Command::new(&cc)
        .args([
            c_path.to_str().unwrap(),
            "-o",
            bin_path.to_str().unwrap(),
            "-w", // suppress warnings from generated C
        ])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: cannot run '{}': {}", cc, e);
            process::exit(1);
        });
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}
