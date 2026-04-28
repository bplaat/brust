// out: n_tilde
// out: snowman
// out: A
// out: newline
// out: tab
// out: backslash
// out: null

fn describe(c: char) -> &str {
    match c {
        'Ñ' => "n_tilde",
        '\u{2603}' => "snowman",
        'A' => "A",
        '\n' => "newline",
        '\t' => "tab",
        '\\' => "backslash",
        '\0' => "null",
        _ => "other",
    }
}

fn main() {
    // Non-ASCII UTF-8 char literal
    println!("{}", describe('Ñ'));

    // \u{...} unicode escape in char literal
    println!("{}", describe('\u{2603}'));

    // \u{41} == 'A'
    println!("{}", describe('\u{41}'));

    // Standard escape sequences
    println!("{}", describe('\n'));
    println!("{}", describe('\t'));
    println!("{}", describe('\\'));
    println!("{}", describe('\0'));
}
