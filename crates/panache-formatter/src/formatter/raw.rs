use crate::directives::extract_directive_from_node;
use crate::formatter::Formatter;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

impl Formatter {
    pub(super) fn format_html_block(&mut self, node: &SyntaxNode) {
        self.process_format_directive(node);

        let mut text = String::new();
        let mut after_marker = false;
        for element in node.descendants_with_tokens() {
            let NodeOrToken::Token(token) = element else {
                continue;
            };
            if token.kind() == SyntaxKind::LINE_PREFIX {
                if token.text().contains('>') {
                    after_marker = true;
                } else if after_marker {
                    after_marker = false;
                } else {
                    text.push_str(token.text());
                }
                continue;
            }
            after_marker = false;
            text.push_str(token.text());
        }
        self.output.push_str(&text);
        if !text.ends_with('\n') {
            self.output.push('\n');
        }
    }

    pub(super) fn format_comment(&mut self, node: &SyntaxNode) {
        self.process_format_directive(node);
        let text = node.text().to_string();
        self.output.push_str(&text);
        if !text.ends_with('\n') {
            self.output.push('\n');
        }
    }

    pub(super) fn format_latex_command(&mut self, node: &SyntaxNode) {
        self.output.push_str(&node.text().to_string());
    }

    pub(super) fn format_tex_block(&mut self, node: &SyntaxNode) {
        log::trace!("Formatting TeX block");
        for child in node.children_with_tokens() {
            if let NodeOrToken::Token(token) = child {
                self.output.push_str(token.text());
            }
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn process_format_directive(&mut self, node: &SyntaxNode) {
        let Some(directive) = extract_directive_from_node(node) else {
            return;
        };
        self.directive_tracker.process_directive(&directive);
        if matches!(directive, crate::directives::Directive::Start(_))
            && self.directive_tracker.is_formatting_ignored()
            && self.ignore_region_start.is_none()
        {
            self.ignore_region_start = Some(self.output.len());
        }
    }
}
