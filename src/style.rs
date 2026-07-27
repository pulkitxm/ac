use owo_colors::{AnsiColors, OwoColorize, Stream};

macro_rules! paint {
    ($s:expr, $stream:expr, $method:ident) => {
        $s.if_supports_color($stream, |t| t.$method()).to_string()
    };
}

pub fn blue(s: &str) -> String {
    paint!(s, Stream::Stdout, blue)
}

pub fn green(s: &str) -> String {
    paint!(s, Stream::Stdout, green)
}

pub fn bold(s: &str) -> String {
    paint!(s, Stream::Stdout, bold)
}

pub fn dim(s: &str) -> String {
    paint!(s, Stream::Stdout, dimmed)
}

pub fn yellow(s: &str) -> String {
    paint!(s, Stream::Stderr, yellow)
}

pub fn red(s: &str) -> String {
    paint!(s, Stream::Stderr, red)
}

pub fn dim_err(s: &str) -> String {
    paint!(s, Stream::Stderr, dimmed)
}

pub fn colored(s: &str, c: AnsiColors) -> String {
    s.if_supports_color(Stream::Stdout, |t| t.color(c))
        .to_string()
}

pub const LOG_PALETTE: &[AnsiColors] = &[
    AnsiColors::Blue,
    AnsiColors::Green,
    AnsiColors::Yellow,
    AnsiColors::Red,
    AnsiColors::Magenta,
    AnsiColors::Cyan,
];
