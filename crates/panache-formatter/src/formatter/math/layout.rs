//! Small Wadler-style document model for structured math layout.
//!
//! This is adapted from Badness's formatter IR and printer, reduced to the
//! primitives needed by embedded math environments. Keeping multiline fragments
//! as documents lets surrounding punctuation remain in the same concatenation.

#[derive(Clone, Debug)]
pub(super) enum Doc {
    Nil,
    Text(String),
    Concat(Vec<Doc>),
    Line,
    SoftLine,
    HardLine,
    Indent(Box<Doc>),
    Align(usize, Box<Doc>),
    Group(Box<Doc>),
}

impl Doc {
    pub(super) fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        debug_assert!(!text.contains('\n'));
        if text.is_empty() {
            Self::Nil
        } else {
            Self::Text(text)
        }
    }

    pub(super) fn concat(docs: impl IntoIterator<Item = Doc>) -> Self {
        let docs: Vec<_> = docs
            .into_iter()
            .filter(|doc| !matches!(doc, Self::Nil))
            .collect();
        match docs.len() {
            0 => Self::Nil,
            1 => docs.into_iter().next().expect("one document"),
            _ => Self::Concat(docs),
        }
    }

    pub(super) fn join(separator: Doc, docs: impl IntoIterator<Item = Doc>) -> Self {
        let docs: Vec<_> = docs
            .into_iter()
            .filter(|doc| !matches!(doc, Self::Nil))
            .collect();
        let mut out = Vec::new();
        for (index, doc) in docs.into_iter().enumerate() {
            if index > 0 {
                out.push(separator.clone());
            }
            out.push(doc);
        }
        Self::concat(out)
    }

    pub(super) fn indent(doc: Doc) -> Self {
        Self::Indent(Box::new(doc))
    }

    pub(super) fn align(width: usize, doc: Doc) -> Self {
        if width == 0 || matches!(doc, Self::Nil) {
            doc
        } else {
            Self::Align(width, Box::new(doc))
        }
    }

    pub(super) fn group(doc: Doc) -> Self {
        Self::Group(Box::new(doc))
    }

    fn flat_width(&self) -> Option<usize> {
        match self {
            Self::Nil => Some(0),
            Self::Text(text) => Some(text.chars().count()),
            Self::Concat(docs) => docs
                .iter()
                .try_fold(0usize, |width, doc| Some(width + doc.flat_width()?)),
            Self::Line => Some(1),
            Self::SoftLine => Some(0),
            Self::HardLine => None,
            Self::Indent(doc) | Self::Align(_, doc) | Self::Group(doc) => doc.flat_width(),
        }
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Flat,
    Break,
}

pub(super) struct Printer {
    line_width: usize,
    indent_width: usize,
}

impl Printer {
    pub(super) fn new(line_width: usize, indent_width: usize) -> Self {
        Self {
            line_width,
            indent_width,
        }
    }

    pub(super) fn print(&self, doc: &Doc, initial_indent: usize) -> String {
        let mut writer = Writer::new(initial_indent);
        self.render(doc, initial_indent, Mode::Break, &mut writer);
        writer.out
    }

    fn render(&self, doc: &Doc, indent: usize, mode: Mode, writer: &mut Writer) {
        match doc {
            Doc::Nil => {}
            Doc::Text(text) => writer.write(text),
            Doc::Concat(docs) => {
                for doc in docs {
                    self.render(doc, indent, mode, writer);
                }
            }
            Doc::Line => match mode {
                Mode::Flat => writer.write(" "),
                Mode::Break => writer.newline(indent),
            },
            Doc::SoftLine => {
                if matches!(mode, Mode::Break) {
                    writer.newline(indent);
                }
            }
            Doc::HardLine => writer.newline(indent),
            Doc::Indent(inner) => {
                self.render(inner, indent + self.indent_width, mode, writer);
            }
            Doc::Align(width, inner) => self.render(inner, indent + width, mode, writer),
            Doc::Group(inner) => {
                let fits = inner
                    .flat_width()
                    .is_some_and(|width| writer.current_column() + width <= self.line_width);
                self.render(
                    inner,
                    indent,
                    if fits { Mode::Flat } else { Mode::Break },
                    writer,
                );
            }
        }
    }
}

struct Writer {
    out: String,
    column: usize,
    pending_indent: usize,
    needs_indent: bool,
}

impl Writer {
    fn new(initial_indent: usize) -> Self {
        Self {
            out: String::new(),
            column: 0,
            pending_indent: initial_indent,
            needs_indent: initial_indent > 0,
        }
    }

    fn current_column(&self) -> usize {
        self.column
            + if self.needs_indent {
                self.pending_indent
            } else {
                0
            }
    }

    fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.needs_indent {
            self.out.push_str(&" ".repeat(self.pending_indent));
            self.column += self.pending_indent;
            self.needs_indent = false;
        }
        self.out.push_str(text);
        self.column += text.chars().count();
    }

    fn newline(&mut self, indent: usize) {
        self.out.push('\n');
        self.column = 0;
        self.pending_indent = indent;
        self.needs_indent = indent > 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printer(width: usize) -> Printer {
        Printer::new(width, 2)
    }

    #[test]
    fn group_stays_flat_when_it_fits() {
        let doc = Doc::group(Doc::concat([
            Doc::text("f("),
            Doc::indent(Doc::concat([Doc::SoftLine, Doc::text("x")])),
            Doc::SoftLine,
            Doc::text(")"),
        ]));
        assert_eq!(printer(80).print(&doc, 0), "f(x)");
    }

    #[test]
    fn hard_line_forces_enclosing_group_open() {
        let block = Doc::concat([
            Doc::text("\\begin{x}"),
            Doc::indent(Doc::concat([Doc::HardLine, Doc::text("a")])),
            Doc::HardLine,
            Doc::text("\\end{x}"),
        ]);
        let doc = Doc::group(Doc::concat([
            Doc::text("f("),
            Doc::indent(Doc::concat([Doc::SoftLine, block])),
            Doc::SoftLine,
            Doc::text(")"),
        ]));
        assert_eq!(
            printer(80).print(&doc, 0),
            "f(\n  \\begin{x}\n    a\n  \\end{x}\n)"
        );
    }

    #[test]
    fn align_hangs_block_and_keeps_punctuation_attached() {
        let block = Doc::align(
            4,
            Doc::concat([
                Doc::text("\\begin{x}"),
                Doc::indent(Doc::concat([Doc::HardLine, Doc::text("a")])),
                Doc::HardLine,
                Doc::text("\\end{x}"),
            ]),
        );
        let doc = Doc::concat([Doc::text("v = "), block, Doc::text(",")]);
        assert_eq!(
            printer(80).print(&doc, 2),
            "  v = \\begin{x}\n        a\n      \\end{x},"
        );
    }

    #[test]
    fn broken_group_uses_line_and_soft_line_distinctly() {
        let doc = Doc::group(Doc::concat([
            Doc::text("f("),
            Doc::indent(Doc::concat([
                Doc::SoftLine,
                Doc::text("aaaaaaaa"),
                Doc::Line,
                Doc::text("bbbbbbbb"),
            ])),
            Doc::SoftLine,
            Doc::text(")"),
        ]));
        assert_eq!(printer(10).print(&doc, 0), "f(\n  aaaaaaaa\n  bbbbbbbb\n)");
    }

    #[test]
    fn join_does_not_separate_empty_documents() {
        let doc = Doc::join(Doc::Line, [Doc::text("a,"), Doc::Nil]);
        assert_eq!(printer(80).print(&doc, 0), "a,");
    }
}
