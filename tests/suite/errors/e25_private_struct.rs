// err: `lexer_Token` is private

mod lexer {
    struct Token {
        pub value: i32,
    }
}

fn main() {
    let t: lexer::Token = lexer::Token { value: 1 };
}
