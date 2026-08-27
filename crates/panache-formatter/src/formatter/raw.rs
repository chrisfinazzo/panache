use crate::directives::extract_directive_from_node;
use crate::formatter::Formatter;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

use super::code_blocks;
use super::math;

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
        let formatted = math::format_latex_math_environment(node, &self.config)
            .unwrap_or_else(|| node.text().to_string());
        self.output.push_str(&formatted);
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

/// A single space after the closing colon makes valued MyST options reparse to
/// the same CST, while valueless options remain compact.
fn format_directive_option(node: &SyntaxNode) -> String {
    let mut name = String::new();
    let mut value = String::new();
    for element in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::MYST_DIRECTIVE_OPTION_NAME => name = token.text().to_string(),
                SyntaxKind::MYST_DIRECTIVE_OPTION_VALUE => value = token.text().to_string(),
                _ => {}
            }
        }
    }
    if value.is_empty() {
        format!(":{name}:")
    } else {
        format!(":{name}: {value}")
    }
}

impl Formatter {
    pub(super) fn format_myst_directive(&mut self, node: &SyntaxNode, indent: usize) {
        let mut open_text: Option<String> = None;
        let mut close_text: Option<String> = None;
        let mut options = Vec::new();
        let mut body = Vec::new();
        let mut verbatim_body: Option<SyntaxNode> = None;
        for element in node.children_with_tokens() {
            if let NodeOrToken::Node(child) = element {
                match child.kind() {
                    SyntaxKind::MYST_DIRECTIVE_OPEN => {
                        open_text = Some(child.text().to_string());
                    }
                    SyntaxKind::MYST_DIRECTIVE_CLOSE => {
                        close_text = Some(child.text().to_string());
                    }
                    SyntaxKind::MYST_DIRECTIVE_OPTION => options.push(child),
                    SyntaxKind::MYST_DIRECTIVE_BODY => verbatim_body = Some(child),
                    _ => body.push(child),
                }
            }
        }

        if let Some(open) = &open_text {
            self.output.push_str(open.trim_end_matches('\n'));
            self.output.push('\n');
        }

        for option in &options {
            self.output.push_str(&format_directive_option(option));
            self.output.push('\n');
        }

        if let Some(body_node) = verbatim_body {
            let body_text = code_blocks::extract_myst_directive_parts(node)
                .and_then(|(language, body)| self.formatted_code.get(&(language, body)).cloned())
                .unwrap_or_else(|| body_node.text().to_string());
            self.output.push_str(body_text.trim_end_matches('\n'));
            self.output.push('\n');
            if let Some(close) = &close_text {
                self.output.push_str(close.trim_end_matches('\n'));
                self.output.push('\n');
            }
            self.consecutive_blank_lines = 0;
            return;
        }

        let leading = body
            .iter()
            .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
            .count();
        let trailing = body
            .iter()
            .rev()
            .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
            .count();
        let end = body.len().saturating_sub(trailing).max(leading);

        if !options.is_empty() && leading < end {
            self.output.push('\n');
        }

        let mut prev_blank = false;
        for child in &body[leading..end] {
            if child.kind() == SyntaxKind::BLANK_LINE {
                if !prev_blank {
                    self.output.push('\n');
                    prev_blank = true;
                }
                continue;
            }
            self.format_node_sync(child, indent);
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            prev_blank = false;
        }

        if let Some(close) = &close_text {
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            self.output.push_str(close.trim_end_matches('\n'));
            self.output.push('\n');
        }
        self.consecutive_blank_lines = 0;
    }

    pub(super) fn format_raw_block(&mut self, node: &SyntaxNode) {
        self.output
            .push_str(node.text().to_string().trim_end_matches('\n'));
        self.output.push('\n');
        self.consecutive_blank_lines = 0;
    }
}
