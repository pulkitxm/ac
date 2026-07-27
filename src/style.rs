//! The one place that decides whether a byte of ANSI is emitted.
//!
//! Every coloured string in ac goes through a function here. That is what makes
//! the colour contract real: `Ctx::new` calls [`owo_colors::set_override`] once,
//! with the answer to "is colour allowed", and these helpers respect it. Colour
//! is therefore off automatically when stdout is not a terminal, when `NO_COLOR`
//! is set, when `--no-color` is passed, and whenever `--json` is in play.
//!
//! Reaching for `OwoColorize::blue()` directly would bypass the override and
//! paint escape codes into redirected output, so it is deliberately not done
//! anywhere else in the tree.

use owo_colors::{AnsiColors, OwoColorize, Stream};

/// Human status lines go to stdout, so that is the stream we ask about.
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

/// Warnings and errors are written to stderr, which can be a terminal even when
/// stdout is a pipe.
pub fn yellow(s: &str) -> String {
    paint!(s, Stream::Stderr, yellow)
}

pub fn red(s: &str) -> String {
    paint!(s, Stream::Stderr, red)
}

/// The dimmed `$ container ...` echo, which also goes to stderr.
pub fn dim_err(s: &str) -> String {
    paint!(s, Stream::Stderr, dimmed)
}

/// One of the rotating per-service colours used by `ac <project> logs`.
pub fn colored(s: &str, c: AnsiColors) -> String {
    s.if_supports_color(Stream::Stdout, |t| t.color(c))
        .to_string()
}

/// The palette `logs` cycles through, one colour per service.
pub const LOG_PALETTE: &[AnsiColors] = &[
    AnsiColors::Blue,
    AnsiColors::Green,
    AnsiColors::Yellow,
    AnsiColors::Red,
    AnsiColors::Magenta,
    AnsiColors::Cyan,
];
