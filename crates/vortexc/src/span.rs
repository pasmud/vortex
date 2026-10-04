//! Source positions.
//!
//! Positions are 1-based for both line and column, as required by `SPEC.md`
//! section 4. A column counts Unicode scalar values, not bytes, so a multi byte
//! character advances the column by one and a column matches what a reader
//! counts in an editor. A tab also advances the column by one.

/// A position in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number, counting Unicode scalar values.
    pub col: u32,
}

impl Pos {
    /// The position of the first character of a file.
    pub const START: Pos = Pos { line: 1, col: 1 };

    /// Returns a position one column to the right.
    pub fn next_col(self) -> Pos {
        Pos {
            line: self.line,
            col: self.col + 1,
        }
    }

    /// Returns the position of the first column of the next line.
    pub fn next_line(self) -> Pos {
        Pos {
            line: self.line + 1,
            col: 1,
        }
    }
}

impl std::fmt::Display for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Renders a diagnostic with a caret under the offending column.
///
/// This is the format every lexer and parser error uses, so that a reader sees
/// the same shape everywhere in the project.
pub fn render_snippet(src: &str, pos: Pos, message: &str) -> String {
    let mut out = String::new();
    let mut line_no = 1u32;
    for line in src.split('\n') {
        if line_no == pos.line {
            out.push_str(&format!("{}\n", line));
            // The caret is one space per character, not per byte.
            for (i, ch) in line.chars().enumerate() {
                if (i as u32) + 1 == pos.col {
                    break;
                }
                if ch == '\t' {
                    out.push('\t');
                } else {
                    out.push(' ');
                }
            }
            out.push_str("^\n");
            break;
        }
        line_no += 1;
    }
    out.push_str(message);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_one_based() {
        assert_eq!(Pos::START, Pos { line: 1, col: 1 });
        assert_eq!(Pos { line: 1, col: 1 }.next_col(), Pos { line: 1, col: 2 });
        assert_eq!(Pos { line: 1, col: 4 }.next_line(), Pos { line: 2, col: 1 });
    }

    #[test]
    fn position_displays_as_line_colon_col() {
        assert_eq!(Pos { line: 3, col: 7 }.to_string(), "3:7");
    }

    #[test]
    fn caret_sits_under_the_reported_column() {
        let snippet = render_snippet("let x = @\n", Pos { line: 1, col: 9 }, "boom");
        assert_eq!(snippet, "let x = @\n        ^\nboom");
    }

    #[test]
    fn caret_counts_characters_not_bytes() {
        // The emoji is one column wide even though it is several bytes.
        let src = "let \u{1F600} = 1\n";
        let snippet = render_snippet(src, Pos { line: 1, col: 7 }, "boom");
        assert_eq!(snippet, "let \u{1F600} = 1\n      ^\nboom");
    }

    #[test]
    fn caret_tracks_a_tab_as_one_column() {
        let snippet = render_snippet("\tx = 1\n", Pos { line: 1, col: 3 }, "boom");
        assert_eq!(snippet, "\tx = 1\n\t ^\nboom");
    }

    #[test]
    fn snippet_picks_the_requested_line() {
        let snippet = render_snippet("a\nbb\nccc\n", Pos { line: 3, col: 2 }, "boom");
        assert_eq!(snippet, "ccc\n ^\nboom");
    }
}
