// out: Ñoño
// out: A☃
// out: foobar
// out: tab	here

fn main() {
    // Non-ASCII UTF-8 characters in string literals
    println!("{}", "Ñoño");

    // \u{...} unicode escapes in string literals
    println!("{}", "\u{41}\u{2603}");

    // Backslash-newline continuation: strips '\', the newline, and all
    // leading whitespace on the next line.
    let s = "foo\
             bar";
    println!("{}", s);

    // Mix of ASCII escapes and literal text
    println!("{}", "tab\there");
}
