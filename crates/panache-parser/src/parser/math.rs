//! In-tree TeX math content parser.
//!
//! Produces a lossless structural CST for the *content* between math
//! delimiters (the delimiters themselves are owned by the host `INLINE_MATH` /
//! `DISPLAY_MATH` nodes, see `parser/inlines/math.rs`). The returned subtree is
//! rooted at [`SyntaxKind::MATH_CONTENT`] and is spliced directly into the host
//! document tree, replacing the opaque content `TEXT` token.
//!
//! This is a *syntactic* parse, not a semantic one: TeX is a Turing-complete
//! macro language, so we only capture structure that a formatter can safely act
//! on — brace groups, `\begin`/`\end` environments, control sequences,
//! alignment tabs (`&`), line breaks (`\\`), sub/superscript markers, comments,
//! and whitespace. Everything else is an ordinary-atom run ([`MATH_TEXT`]).
//!
//! The **CST is lossless and never fails** (`node.text() == content` for every
//! input; worst case is a single `MATH_TEXT` atom). Structural problems
//! (unbalanced braces, unclosed or mismatched environments) are *not* reported
//! here: they are derived from the realized tree shape by
//! [`crate::syntax::math_diagnostics`], the single source of truth shared by the
//! linter, formatter, and LSP. Keeping the parser diagnostic-free means the
//! host-aligned ranges come for free from the spliced subtree.
//!
//! [`MATH_TEXT`]: SyntaxKind::MATH_TEXT

use crate::parser::inlines::bookdown::try_parse_bookdown_equation_definition;
use crate::syntax::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder};

/// Flavor-/extension-dependent parsing options for math content. Default is
/// all-off (pure TeX). The math grammar itself is flavor-agnostic; only
/// constructs layered on top of TeX by a Markdown flavor live here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MathParseOptions {
    /// Recognize bookdown equation labels `(\#eq:label)` as a single
    /// [`SyntaxKind::MATH_EQUATION_LABEL`] token (gated on the
    /// `bookdown_equation_references` extension).
    pub bookdown_equation_labels: bool,
}

/// Parse math content into a lossless `MATH_CONTENT` green node. `content` is
/// the raw text between (but excluding) the math delimiters. Never fails:
/// `SyntaxNode::new_root(result).text() == content` for every input.
pub fn parse_math_content(content: &str, opts: MathParseOptions) -> GreenNode {
    let mut parser = MathParser {
        input: content,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        opts,
    };
    parser.builder.start_node(SyntaxKind::MATH_CONTENT.into());
    parser.parse_elements(Ctx::Top);
    parser.builder.finish_node();
    parser.builder.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// Top level of the math content.
    Top,
    /// Inside a `{ ... }` brace group; stops at the matching `}`.
    Group,
    /// Inside a `\begin{env} ... \end{env}` body; stops at `\end`.
    Env,
    /// Inside a `\left<d> ... \right<d>` body; stops at `\right`.
    LeftRight,
}

struct MathParser<'a> {
    input: &'a str,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    opts: MathParseOptions,
}

impl MathParser<'_> {
    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn peek_char(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump_bytes(&mut self, len: usize, kind: SyntaxKind) {
        let text = &self.input[self.pos..self.pos + len];
        self.builder.token(kind.into(), text);
        self.pos += len;
    }

    fn peek_control_word(&self) -> Option<&str> {
        let after = self.rest().strip_prefix('\\')?;
        let len: usize = after
            .bytes()
            .take_while(|b| b.is_ascii_alphabetic() || *b == b'@')
            .count();
        if len == 0 { None } else { Some(&after[..len]) }
    }

    fn parse_elements(&mut self, ctx: Ctx) {
        while let Some(c) = self.peek_char() {
            match c {
                '}' if ctx == Ctx::Group => break,
                '}' => self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE),
                '\\' => {
                    if self.rest().starts_with("\\\\") {
                        self.bump_bytes(2, SyntaxKind::MATH_LINE_BREAK);
                    } else if let Some(word) = self.peek_control_word() {
                        match word {
                            "begin" => self.parse_environment(),
                            "end" if ctx == Ctx::Env => break,
                            "end" => {
                                self.parse_control_word();
                            }
                            "left" => self.parse_delimited(),
                            "right" if ctx == Ctx::LeftRight => break,
                            "right" => {
                                self.parse_control_word();
                            }
                            _ => self.parse_control_word(),
                        }
                    } else {
                        self.parse_control_symbol();
                    }
                }
                '{' => self.parse_group(),
                '(' if self.opts.bookdown_equation_labels => match self.equation_label_len() {
                    Some(len) => self.bump_bytes(len, SyntaxKind::MATH_EQUATION_LABEL),
                    None => self.bump_bytes(1, SyntaxKind::MATH_OPEN),
                },
                '(' | '[' => self.bump_bytes(1, SyntaxKind::MATH_OPEN),
                ')' | ']' => self.bump_bytes(1, SyntaxKind::MATH_CLOSE),
                ',' | ';' => self.bump_bytes(1, SyntaxKind::MATH_PUNCT),
                ':' => self.bump_bytes(1, SyntaxKind::MATH_TEXT),
                '&' => self.bump_bytes(1, SyntaxKind::MATH_ALIGN),
                '^' | '_' => self.bump_bytes(1, SyntaxKind::MATH_SCRIPT),
                c if is_operator(c) => self.bump_bytes(1, SyntaxKind::MATH_OPERATOR),
                '%' => self.parse_comment(),
                ' ' | '\t' => self.parse_spaces(),
                '\n' => self.bump_bytes(1, SyntaxKind::MATH_NEWLINE),
                '\r' => {
                    let len = if self.rest().starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
                    self.bump_bytes(len, SyntaxKind::MATH_NEWLINE);
                }
                _ => self.parse_text(),
            }
        }
    }

    fn parse_environment(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_ENVIRONMENT.into());
        self.parse_control_word(); // \begin
        self.parse_environment_name(); // {name} group, if present
        self.parse_elements(Ctx::Env);
        if self.peek_control_word() == Some("end") {
            self.parse_control_word(); // \end
            self.parse_environment_name(); // {name} group, if present
        }
        self.builder.finish_node();
    }

    fn parse_environment_name(&mut self) {
        if self.peek_char() == Some('{') {
            self.parse_group();
        }
    }

    fn parse_group(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_GROUP.into());
        self.bump_bytes(1, SyntaxKind::MATH_GROUP_OPEN); // {
        self.parse_elements(Ctx::Group);
        if self.peek_char() == Some('}') {
            self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE); // }
        }
        self.builder.finish_node();
    }

    fn parse_delimited(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_DELIMITED.into());
        self.parse_control_word(); // \left
        self.consume_delimiter(); // opening delimiter argument
        self.parse_elements(Ctx::LeftRight);
        if self.peek_control_word() == Some("right") {
            self.parse_control_word(); // \right
            self.consume_delimiter(); // closing delimiter argument
        }
        self.builder.finish_node();
    }

    /// Consume the single delimiter that follows `\left` / `\right`, when it sits
    /// immediately at the cursor. Brackets keep their fixed `MATH_OPEN`/
    /// `MATH_CLOSE` kind; the ambiguous `. | /` stay `MATH_TEXT` (as elsewhere);
    /// a control-sequence delimiter (`\{`, `\langle`, `\|`, …) is a `MATH_COMMAND`.
    /// If a space or anything else intervenes, nothing is consumed here and the
    /// surrounding element loop tokenizes it normally — losslessness holds either
    /// way; only the token's node membership shifts.
    fn consume_delimiter(&mut self) {
        match self.peek_char() {
            Some('(' | '[') => self.bump_bytes(1, SyntaxKind::MATH_OPEN),
            Some(')' | ']') => self.bump_bytes(1, SyntaxKind::MATH_CLOSE),
            Some('.' | '|' | '/') => self.bump_bytes(1, SyntaxKind::MATH_TEXT),
            Some('\\') => {
                if self.peek_control_word().is_some() {
                    self.parse_control_word();
                } else {
                    self.parse_control_symbol();
                }
            }
            _ => {}
        }
    }

    fn parse_control_word(&mut self) {
        let word_len = self.peek_control_word().map(str::len).unwrap_or(0);
        self.bump_bytes(1 + word_len, SyntaxKind::MATH_COMMAND);
    }

    fn parse_control_symbol(&mut self) {
        let after = &self.input[self.pos + 1..];
        let len = 1 + after.chars().next().map(char::len_utf8).unwrap_or(0);
        self.bump_bytes(len, SyntaxKind::MATH_COMMAND);
    }

    fn parse_comment(&mut self) {
        let len = self
            .rest()
            .find(['\n', '\r'])
            .unwrap_or_else(|| self.rest().len());
        self.bump_bytes(len, SyntaxKind::MATH_COMMENT);
    }

    fn parse_spaces(&mut self) {
        let len = self
            .rest()
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        self.bump_bytes(len, SyntaxKind::MATH_SPACE);
    }

    fn parse_text(&mut self) {
        let len = self
            .rest()
            .find(|c: char| is_special(c))
            .unwrap_or_else(|| self.rest().len());
        debug_assert!(len > 0, "parse_text on a special char");
        self.bump_bytes(len, SyntaxKind::MATH_TEXT);
    }

    fn equation_label_len(&self) -> Option<usize> {
        try_parse_bookdown_equation_definition(self.rest()).map(|(len, _)| len)
    }
}

fn is_special(c: char) -> bool {
    is_operator(c)
        || is_delimiter(c)
        || matches!(
            c,
            '\\' | '{' | '}' | '&' | '^' | '_' | '%' | ':' | ' ' | '\t' | '\n' | '\r'
        )
}

fn is_delimiter(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | ',' | ';')
}

/// Operator atoms split out of ordinary text into their own
/// [`SyntaxKind::MATH_OPERATOR`] token. The TeX mathbin (`+ - *`) and mathrel
/// (`= < >`) core; the formatter assigns class/precedence/spacing downstream.
fn is_operator(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '=' | '<' | '>')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxNode;

    fn node(content: &str) -> SyntaxNode {
        SyntaxNode::new_root(parse_math_content(content, MathParseOptions::default()))
    }

    fn node_with(content: &str, opts: MathParseOptions) -> SyntaxNode {
        SyntaxNode::new_root(parse_math_content(content, opts))
    }

    fn token_kinds(content: &str) -> Vec<SyntaxKind> {
        node(content)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect()
    }

    /// Losslessness is the hard invariant for every input.
    fn assert_lossless(content: &str) {
        assert_eq!(
            node(content).text().to_string(),
            content,
            "roundtrip: {content:?}"
        );
    }

    #[test]
    fn root_is_math_content() {
        assert_eq!(node("x").kind(), SyntaxKind::MATH_CONTENT);
    }

    #[test]
    fn plain_text_is_one_atom_run() {
        assert_eq!(token_kinds("abc"), vec![SyntaxKind::MATH_TEXT]);
        assert_lossless("abc");
        assert_eq!(
            token_kinds("f(x)/2.5"),
            vec![
                SyntaxKind::MATH_TEXT,  // f
                SyntaxKind::MATH_OPEN,  // (
                SyntaxKind::MATH_TEXT,  // x
                SyntaxKind::MATH_CLOSE, // )
                SyntaxKind::MATH_TEXT,  // /2.5
            ]
        );
        assert_lossless("f(x)/2.5");
    }

    #[test]
    fn delimiters_and_punctuation_split_atom_runs() {
        assert_eq!(
            token_kinds("[a,b);"),
            vec![
                SyntaxKind::MATH_OPEN,  // [
                SyntaxKind::MATH_TEXT,  // a
                SyntaxKind::MATH_PUNCT, // ,
                SyntaxKind::MATH_TEXT,  // b
                SyntaxKind::MATH_CLOSE, // )
                SyntaxKind::MATH_PUNCT, // ;
            ]
        );
        assert_lossless("[a,b);");
        assert_eq!(token_kinds("a|b.c/d"), vec![SyntaxKind::MATH_TEXT]);
        assert_lossless("a|b.c/d");
        assert_eq!(token_kinds(r"\(\)\[\]"), vec![SyntaxKind::MATH_COMMAND; 4]);
        assert_lossless(r"\(\)\[\]");
    }

    #[test]
    fn operators_split_atom_runs() {
        assert_eq!(
            token_kinds("a+b=c"),
            vec![
                SyntaxKind::MATH_TEXT,     // a
                SyntaxKind::MATH_OPERATOR, // +
                SyntaxKind::MATH_TEXT,     // b
                SyntaxKind::MATH_OPERATOR, // =
                SyntaxKind::MATH_TEXT,     // c
            ]
        );
        assert_lossless("a+b=c");
    }

    #[test]
    fn each_operator_char_is_its_own_token() {
        for op in ["+", "-", "*", "=", "<", ">"] {
            assert_eq!(
                token_kinds(op),
                vec![SyntaxKind::MATH_OPERATOR],
                "operator {op:?}"
            );
            assert_lossless(op);
        }
        assert_eq!(
            token_kinds("a<=b"),
            vec![
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_OPERATOR, // <
                SyntaxKind::MATH_OPERATOR, // =
                SyntaxKind::MATH_TEXT,
            ]
        );
        assert_eq!(
            token_kinds("-x"),
            vec![SyntaxKind::MATH_OPERATOR, SyntaxKind::MATH_TEXT]
        );
        assert_lossless("-x");
        assert_eq!(token_kinds(r"\<"), vec![SyntaxKind::MATH_COMMAND]);
        assert_lossless(r"\<");
    }

    #[test]
    fn operators_inside_groups_and_scripts_are_lossless() {
        for content in [r"e^{-x}", r"10^{-3}", r"\frac{a+b}{c-d}", r"x_{i+1}"] {
            assert_lossless(content);
        }
    }

    #[test]
    fn control_word_and_symbol() {
        assert_eq!(
            token_kinds(r"\alpha\,"),
            vec![SyntaxKind::MATH_COMMAND, SyntaxKind::MATH_COMMAND]
        );
        assert_lossless(r"\alpha\,");
        assert_eq!(token_kinds(r"\&\%\{\}"), vec![SyntaxKind::MATH_COMMAND; 4]);
        assert_lossless(r"\&\%\{\}");
    }

    #[test]
    fn brace_group_nests() {
        let tree = node(r"x^{2}");
        let group = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MATH_GROUP)
            .expect("group");
        let kinds: Vec<_> = group.children_with_tokens().map(|el| el.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::MATH_GROUP_OPEN,
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_GROUP_CLOSE
            ]
        );
        assert_lossless(r"x^{2}");
    }

    #[test]
    fn line_break_alignment_and_scripts() {
        assert_eq!(
            token_kinds(r"x &= 1 \\"),
            vec![
                SyntaxKind::MATH_TEXT,       // x
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_ALIGN,      // &
                SyntaxKind::MATH_OPERATOR,   // =
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_TEXT,       // 1
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_LINE_BREAK, // \\
            ]
        );
        assert_lossless(r"x &= 1 \\");
        assert_eq!(
            token_kinds("x^2_i"),
            vec![
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_SCRIPT,
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_SCRIPT,
                SyntaxKind::MATH_TEXT,
            ]
        );
    }

    #[test]
    fn environment_wraps_body() {
        let content = "\\begin{aligned}\nx &= 1\n\\end{aligned}";
        let tree = node(content);
        let env = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MATH_ENVIRONMENT)
            .expect("environment");
        assert_eq!(env.text().to_string(), content);
        let commands = env
            .children_with_tokens()
            .filter(|el| el.kind() == SyntaxKind::MATH_COMMAND)
            .count();
        assert_eq!(commands, 2);
        assert_lossless(content);
    }

    #[test]
    fn nested_environments() {
        let content = r"\begin{a}\begin{b}x\end{b}\end{a}";
        let envs = node(content)
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::MATH_ENVIRONMENT)
            .count();
        assert_eq!(envs, 2);
        assert_lossless(content);
    }

    #[test]
    fn comment_runs_to_end_of_line() {
        assert_eq!(
            token_kinds("a % tail\nb"),
            vec![
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_SPACE,
                SyntaxKind::MATH_COMMENT,
                SyntaxKind::MATH_NEWLINE,
                SyntaxKind::MATH_TEXT,
            ]
        );
        assert_lossless("a % tail\nb");
    }

    #[test]
    fn crlf_and_unicode_are_lossless() {
        assert_lossless("x &= 1\r\ny &= 2\r\n");
        assert_lossless(r"\alpha + \beta \neq \gamma_{\text{αβγ}}");
    }

    #[test]
    fn empty_content() {
        assert_eq!(node("").text().to_string(), "");
        assert!(token_kinds("").is_empty());
    }

    #[test]
    fn trailing_backslash() {
        assert_eq!(
            token_kinds("a\\"),
            vec![SyntaxKind::MATH_TEXT, SyntaxKind::MATH_COMMAND]
        );
        assert_lossless("a\\");
    }

    #[test]
    fn malformed_math_is_still_lossless() {
        for content in [
            "{a",
            "a}b",
            r"\begin{aligned} x &= 1",
            r"\begin{aligned}x\end{matrix}",
            r"x \end{aligned}",
        ] {
            assert_lossless(content);
        }
    }

    fn delimited_count(content: &str) -> usize {
        node(content)
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::MATH_DELIMITED)
            .count()
    }

    #[test]
    fn left_right_wraps_a_delimited_node() {
        let content = r"\left( x + y \right)";
        let tree = node(content);
        let delim = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MATH_DELIMITED)
            .expect("delimited node");
        assert_eq!(delim.text().to_string(), content);
        let commands: Vec<String> = delim
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|t| t.kind() == SyntaxKind::MATH_COMMAND)
            .map(|t| t.text().to_string())
            .collect();
        assert_eq!(commands, vec![r"\left", r"\right"]);
        assert_lossless(content);
    }

    #[test]
    fn left_right_delimiters_keep_their_token_kinds() {
        assert_eq!(
            token_kinds(r"\left(x\right)"),
            vec![
                SyntaxKind::MATH_COMMAND, // \left
                SyntaxKind::MATH_OPEN,    // (
                SyntaxKind::MATH_TEXT,    // x
                SyntaxKind::MATH_COMMAND, // \right
                SyntaxKind::MATH_CLOSE,   // )
            ]
        );
    }

    #[test]
    fn null_delimiter_and_asymmetric_pairs_are_lossless() {
        for content in [
            r"\left. x \right|",
            r"\left( x \right]",
            r"\left\{ x \right\}",
        ] {
            assert_eq!(delimited_count(content), 1, "one node: {content:?}");
            assert_lossless(content);
        }
    }

    #[test]
    fn nested_delimited_is_lossless() {
        let content = r"\left[ \left( a \right) \right]";
        assert_eq!(delimited_count(content), 2);
        assert_lossless(content);
    }

    #[test]
    fn unclosed_and_stray_delimiters_stay_lossless() {
        assert_eq!(delimited_count(r"\left( x"), 1);
        assert_lossless(r"\left( x");
        assert_eq!(delimited_count(r"x \right)"), 0);
        assert_lossless(r"x \right)");
    }

    #[test]
    fn leftarrow_and_rightarrow_are_not_delimiters() {
        let content = r"a \leftarrow b \rightarrow c";
        assert_eq!(delimited_count(content), 0);
        assert_lossless(content);
    }

    const BOOKDOWN: MathParseOptions = MathParseOptions {
        bookdown_equation_labels: true,
    };

    fn label_kinds(content: &str, opts: MathParseOptions) -> Vec<SyntaxKind> {
        node_with(content, opts)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect()
    }

    #[test]
    fn equation_label_recognized_when_enabled() {
        let kinds = label_kinds(r"a (\#eq:foo)", BOOKDOWN);
        assert!(kinds.contains(&SyntaxKind::MATH_EQUATION_LABEL));
        let label = node_with(r"a (\#eq:foo)", BOOKDOWN)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|t| t.kind() == SyntaxKind::MATH_EQUATION_LABEL)
            .expect("label token");
        assert_eq!(label.text(), r"(\#eq:foo)");
    }

    #[test]
    fn equation_label_ignored_when_disabled() {
        let kinds = label_kinds(r"a (\#eq:foo)", MathParseOptions::default());
        assert!(!kinds.contains(&SyntaxKind::MATH_EQUATION_LABEL));
    }

    #[test]
    fn plain_parens_tokenize_the_same_with_or_without_bookdown() {
        let expected = vec![
            SyntaxKind::MATH_TEXT,  // f
            SyntaxKind::MATH_OPEN,  // (
            SyntaxKind::MATH_TEXT,  // x
            SyntaxKind::MATH_CLOSE, // )
        ];
        assert_eq!(token_kinds("f(x)"), expected);
        assert_eq!(label_kinds("f(x)", BOOKDOWN), expected);
    }

    #[test]
    fn label_parsing_is_lossless() {
        let content = "\\begin{align}\n  a (\\#eq:solveG)\n\\end{align}";
        assert_eq!(node_with(content, BOOKDOWN).text().to_string(), content);
    }
}
