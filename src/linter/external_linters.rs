//! External linter integration for code blocks.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use crate::external_tools_common::{
    find_missing_commands, log_warning_once, missing_commands_warning_message,
};
use crate::linter::code_block_collector::BlockMapping;
use crate::linter::diagnostics::Diagnostic;
use crate::linter::offsets::line_col_to_byte_offset_1based;

mod clippy;
mod eslint;
mod jarl;
mod jolars;
mod ruff;
mod shellcheck;
mod staticcheck;

pub(crate) trait ExternalLinterParser {
    const NAME: &'static str;
    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError>;
}

/// Errors that can occur when invoking external linters.
#[derive(Debug)]
pub enum LinterError {
    SpawnFailed(String),
    NonZeroExit { code: i32, stderr: String },
    Timeout,
    IoError(std::io::Error),
    ParseError(String),
}

impl std::fmt::Display for LinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(cmd) => write!(f, "failed to spawn linter: {}", cmd),
            Self::NonZeroExit { code, stderr } => {
                write!(f, "linter exited with code {}: {}", code, stderr)
            }
            Self::Timeout => write!(f, "linter timed out"),
            Self::IoError(e) => write!(f, "linter I/O error: {}", e),
            Self::ParseError(msg) => write!(f, "failed to parse linter output: {}", msg),
        }
    }
}

impl std::error::Error for LinterError {}

impl From<std::io::Error> for LinterError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Shared parse context for linter-specific parsers.
pub(crate) struct ParseContext<'a> {
    pub output: &'a str,
    pub linted_input: &'a str,
    pub original_input: &'a str,
    pub mappings: Option<&'a [BlockMapping]>,
}

/// Information about a supported linter.
pub struct LinterInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub url: &'static str,
    pub command: &'static str,
    pub args: Vec<&'static str>,
    pub supported_languages: Vec<&'static str>,
}

fn shellcheck_shell_for_language(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "bash" => "bash",
        "ksh" => "ksh",
        // ShellCheck doesn't support zsh as a dialect flag; use sh as the closest baseline.
        "zsh" | "sh" | "shell" => "sh",
        _ => "sh",
    }
}

pub(crate) fn append_language_specific_args(
    cmd: &mut std::process::Command,
    linter_name: &str,
    language: &str,
) {
    if linter_name.eq_ignore_ascii_case("shellcheck") {
        let shell = shellcheck_shell_for_language(language);
        cmd.arg("--shell").arg(shell);
    }
}

pub(crate) fn file_suffix_for_language(language: &str) -> Option<&'static str> {
    match language.to_ascii_lowercase().as_str() {
        "js" | "javascript" => Some(".js"),
        "jsx" => Some(".jsx"),
        "mjs" => Some(".mjs"),
        "cjs" => Some(".cjs"),
        "ts" | "typescript" => Some(".ts"),
        "tsx" => Some(".tsx"),
        "python" => Some(".py"),
        "go" | "golang" => Some(".go"),
        "rust" | "rs" => Some(".rs"),
        "r" => Some(".R"),
        "julia" | "jl" => Some(".jl"),
        "latex" | "tex" => Some(".tex"),
        "sh" | "bash" | "zsh" | "ksh" | "shell" => Some(".sh"),
        _ => None,
    }
}

pub(crate) fn create_linter_temp_input(
    language: &str,
    code: &str,
) -> Result<(tempfile::TempDir, PathBuf), LinterError> {
    let mut dir_builder = tempfile::Builder::new();
    dir_builder.prefix("panache-external-");
    let temp_dir = dir_builder.tempdir()?;

    let suffix = file_suffix_for_language(language).unwrap_or("");
    let temp_path = temp_dir.path().join(format!("input{}", suffix));
    std::fs::write(&temp_path, code.as_bytes())?;

    Ok((temp_dir, temp_path))
}

/// Registry of supported external linters.
pub struct ExternalLinterRegistry {
    linters: HashMap<String, LinterInfo>,
}

impl ExternalLinterRegistry {
    pub fn new() -> Self {
        let mut linters = HashMap::new();
        linters.insert(
            "jarl".to_string(),
            LinterInfo {
                name: "jarl",
                description: "Jarl is a fast linter for R: it does static code analysis to search for programming errors, bugs, and suspicious patterns of code.",
                url: "https://github.com/etiennebacher/jarl",
                command: "jarl",
                args: vec!["check", "--output-format=json"],
                supported_languages: vec!["r"],
            },
        );
        linters.insert(
            "arity".to_string(),
            LinterInfo {
                name: "arity",
                description: "A language server, formatter, and linter for R with correctness, readability, and performance lints and safe autofixes.",
                url: "https://github.com/jolars/arity",
                command: "arity",
                args: vec!["lint", "--no-config", "--output", "json"],
                supported_languages: vec!["r"],
            },
        );
        linters.insert(
            "badness".to_string(),
            LinterInfo {
                name: "badness",
                description: "A language server, formatter, and linter for LaTeX built on a lossless concrete syntax tree.",
                url: "https://github.com/jolars/badness",
                command: "badness",
                args: vec!["lint", "--no-config", "--output", "json"],
                supported_languages: vec!["latex", "tex"],
            },
        );
        linters.insert(
            "fatou".to_string(),
            LinterInfo {
                name: "fatou",
                description: "A language server, formatter, and linter for Julia that never requires running Julia itself.",
                url: "https://github.com/jolars/fatou",
                command: "fatou",
                args: vec!["lint", "--no-config", "--output", "json"],
                supported_languages: vec!["julia", "jl"],
            },
        );
        linters.insert(
            "ruff".to_string(),
            LinterInfo {
                name: "ruff",
                description: "An extremely fast Python linter and code formatter, written in Rust. ",
                url: "https://docs.astral.sh/ruff/",
                command: "ruff",
                args: vec!["check", "--output-format", "json"],
                supported_languages: vec!["python"],
            },
        );
        linters.insert(
            "eslint".to_string(),
            LinterInfo {
                name: "eslint",
                description: "JavaScript and TypeScript linter.",
                url: "https://eslint.org/",
                command: "eslint",
                args: vec![
                    "--no-config-lookup",
                    "--rule",
                    "no-unused-vars:error",
                    "--format",
                    "json",
                ],
                supported_languages: vec![
                    "js",
                    "javascript",
                    "jsx",
                    "mjs",
                    "cjs",
                    "ts",
                    "typescript",
                    "tsx",
                ],
            },
        );
        linters.insert(
            "shellcheck".to_string(),
            LinterInfo {
                name: "shellcheck",
                description: "Static analysis for shell scripts.",
                url: "https://www.shellcheck.net/",
                command: "shellcheck",
                args: vec!["-f", "json"],
                supported_languages: vec!["sh", "bash", "zsh", "ksh", "shell"],
            },
        );
        linters.insert(
            "staticcheck".to_string(),
            LinterInfo {
                name: "staticcheck",
                description: "Advanced static analysis for Go code.",
                url: "https://staticcheck.dev/",
                command: "staticcheck",
                args: vec!["-f", "json"],
                supported_languages: vec!["go", "golang"],
            },
        );
        linters.insert(
            "clippy".to_string(),
            LinterInfo {
                name: "clippy",
                description: "Rust lints to catch mistakes and improve style.",
                url: "https://doc.rust-lang.org/clippy/",
                command: "clippy-driver",
                args: vec!["--error-format=json", "-W", "clippy::all"],
                supported_languages: vec!["rust", "rs"],
            },
        );
        Self { linters }
    }

    pub fn get(&self, name: &str) -> Option<&LinterInfo> {
        self.linters.get(name)
    }

    pub fn supports_language(&self, linter_name: &str, language: &str) -> Option<bool> {
        self.get(linter_name).map(|info| {
            info.supported_languages
                .iter()
                .any(|supported| supported.eq_ignore_ascii_case(language))
        })
    }
}

impl Default for ExternalLinterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn find_missing_linter_commands<'a, I>(
    configured_linter_names: I,
    registry: &ExternalLinterRegistry,
) -> HashSet<String>
where
    I: IntoIterator<Item = &'a str>,
{
    find_missing_commands(
        configured_linter_names
            .into_iter()
            .filter_map(|name| registry.get(name).map(|info| info.command)),
    )
}

pub fn log_missing_linter_commands(missing: &HashSet<String>) {
    let Some(message) = missing_linter_warning_message(missing) else {
        return;
    };
    log_warning_once(&message);
}

fn missing_linter_warning_message(missing: &HashSet<String>) -> Option<String> {
    missing_commands_warning_message(missing, "linter", "linting")
}

pub fn parse_linter_output(
    linter_name: &str,
    output: &str,
    linted_input: &str,
    original_input: &str,
    mappings: Option<&[BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    let ctx = ParseContext {
        output,
        linted_input,
        original_input,
        mappings,
    };
    if linter_name == jarl::JarlParser::NAME {
        return jarl::JarlParser::parse(&ctx);
    }
    if linter_name == jolars::ArityParser::NAME {
        return jolars::ArityParser::parse(&ctx);
    }
    if linter_name == jolars::FatouParser::NAME {
        return jolars::FatouParser::parse(&ctx);
    }
    if linter_name == jolars::BadnessParser::NAME {
        return jolars::BadnessParser::parse(&ctx);
    }
    if linter_name == ruff::RuffParser::NAME {
        return ruff::RuffParser::parse(&ctx);
    }
    if linter_name == eslint::EslintParser::NAME {
        return eslint::EslintParser::parse(&ctx);
    }
    if linter_name == staticcheck::StaticcheckParser::NAME {
        return staticcheck::StaticcheckParser::parse(&ctx);
    }
    if linter_name == clippy::ClippyParser::NAME {
        return clippy::ClippyParser::parse(&ctx);
    }
    if linter_name == shellcheck::ShellcheckParser::NAME {
        return shellcheck::ShellcheckParser::parse(&ctx);
    }

    Err(LinterError::ParseError(format!(
        "no parser for linter: {}",
        linter_name
    )))
}

pub(crate) fn line_col_to_offset(input: &str, line: usize, column: usize) -> Option<usize> {
    line_col_to_byte_offset_1based(input, line, column)
}

/// Byte offset in the original document for a tool-reported 1-based
/// (line, column). Tool positions are relative to the concatenated,
/// *dedented* lint input, so with mappings present the position must go
/// through the per-line offset table — a column on a container-prefixed
/// line is short of the document column by the stripped prefix. Reading
/// the original input directly is only correct when the whole input was
/// linted as-is (no mappings).
pub(crate) fn map_tool_line_col_to_original(
    ctx: &ParseContext<'_>,
    line: usize,
    column: usize,
) -> Option<usize> {
    match ctx.mappings {
        Some(mappings) => line_col_to_offset(ctx.linted_input, line, column)
            .and_then(|offset| {
                map_concatenated_offset_to_original_with_end_boundary(offset, mappings)
            })
            .or_else(|| line_col_to_offset(ctx.original_input, line, column)),
        None => line_col_to_offset(ctx.original_input, line, column),
    }
}

pub(crate) fn map_concatenated_offset_to_original(
    offset: usize,
    mappings: &[BlockMapping],
) -> Option<usize> {
    for mapping in mappings {
        if mapping.concatenated_range.contains(&offset) {
            // Block content is dedented (container prefixes stripped), so
            // map through the line table: the offset's distance into its
            // line is the same in both views, but each line's start shifts
            // by that line's stripped prefix.
            let original_offset = if let Some(&(line_start, original_line_start)) = mapping
                .line_offsets
                .iter()
                .rev()
                .find(|(line_start, _)| *line_start <= offset)
            {
                original_line_start + (offset - line_start)
            } else {
                // No line table (hand-built mapping): the content is
                // byte-identical to the original, offset arithmetic holds.
                mapping.original_range.start + (offset - mapping.concatenated_range.start)
            };
            if original_offset <= mapping.original_range.end {
                return Some(original_offset);
            }
        }
    }
    None
}

pub(crate) fn map_concatenated_offset_to_original_with_end_boundary(
    offset: usize,
    mappings: &[BlockMapping],
) -> Option<usize> {
    map_concatenated_offset_to_original(offset, mappings).or_else(|| {
        mappings.iter().find_map(|mapping| {
            if mapping.concatenated_range.end == offset {
                Some(mapping.original_range.end)
            } else {
                None
            }
        })
    })
}

/// Map a fix edit's `[start, end)` (offsets in the concatenated lint input)
/// plus its replacement text onto an original-document range.
///
/// Block content is dedented, so a document range covering more than one
/// content line also covers the intervening container-prefix bytes — a
/// tool edit spanning lines of a prefixed block is not expressible as one
/// document edit (applying it verbatim deletes or displaces `> `/indent
/// bytes and merges lines). Such fixes return `None`: the caller keeps
/// the diagnostic and drops the fix.
///
/// Blocks whose content is byte-identical to the document (every line
/// shifted by one constant delta — the common top-level case) stay fully
/// mappable, multi-line edits included.
pub(crate) fn map_concatenated_edit_to_original(
    linted_input: &str,
    start: usize,
    end: usize,
    replacement: &str,
    mappings: &[BlockMapping],
) -> Option<(usize, usize)> {
    if end < start {
        return None;
    }
    let mapping = mappings.iter().find(|mapping| {
        mapping.concatenated_range.contains(&start) || mapping.concatenated_range.end == start
    })?;
    if end > mapping.concatenated_range.end {
        return None;
    }

    let block_delta = mapping
        .original_range
        .start
        .wrapping_sub(mapping.concatenated_range.start);
    let constant_delta = mapping
        .line_offsets
        .iter()
        .all(|&(line_start, original_line_start)| {
            original_line_start.wrapping_sub(line_start) == block_delta
        });
    if constant_delta {
        // Content is byte-identical to the document: plain offset
        // arithmetic expresses any edit, including multi-line ones.
        return Some((start + block_delta, end + block_delta));
    }

    // Prefixed block. A position past the final newline sits before the
    // *next document line's* prefix, so an insertion there would land
    // outside the container's line structure.
    if start == mapping.concatenated_range.end {
        return None;
    }

    // The replaced document range is contiguous, so an edit reaching into
    // another content line would also cover that line's prefix bytes.
    // Line structure must survive too: an edit absorbing the line's
    // trailing newline has to put exactly one back at its end (else the
    // next line's prefix is orphaned onto this line), and an edit inside
    // the line must not introduce newlines (the inserted line would be
    // prefix-less).
    let covered = linted_input.get(start..end)?;
    let covered_newlines = covered.matches('\n').count();
    let replacement_newlines = replacement.matches('\n').count();
    let structure_preserved = match covered_newlines {
        0 => replacement_newlines == 0,
        1 => covered.ends_with('\n') && replacement_newlines == 1 && replacement.ends_with('\n'),
        _ => false,
    };
    if !structure_preserved {
        return None;
    }

    let (line_start, original_line_start) = *mapping
        .line_offsets
        .iter()
        .rev()
        .find(|(line_start, _)| *line_start <= start)?;
    let mapped_start = original_line_start + (start - line_start);
    let mapped_end = original_line_start + (end - line_start);
    if mapped_end > mapping.original_range.end {
        return None;
    }
    Some((mapped_start, mapped_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixed_block_mapping(input: &str) -> (String, Vec<BlockMapping>) {
        let tree = crate::parse(input, None);
        let blocks = crate::utils::collect_code_blocks(&tree, input);
        let result = crate::linter::code_block_collector::concatenate_with_blanks_and_mapping(
            &blocks["python"],
        );
        (result.content, result.mappings)
    }

    #[test]
    fn edit_mapper_maps_line_local_edits_past_prefix() {
        let input = "> ```python\n> import os\n> x = 1\n> ```\n";
        let (linted, mappings) = prefixed_block_mapping(input);

        // Mid-line edit maps past the line's `> ` prefix.
        let start = linted.find("os").unwrap();
        assert_eq!(
            map_concatenated_edit_to_original(&linted, start, start + 2, "sys", &mappings),
            Some((input.find("os").unwrap(), input.find("os").unwrap() + 2))
        );

        // A whole-line replacement that restores the newline stays
        // line-local and is representable.
        let line = linted.find("import os\n").unwrap();
        let doc_line = input.find("import os\n").unwrap();
        assert_eq!(
            map_concatenated_edit_to_original(
                &linted,
                line,
                line + "import os\n".len(),
                "import sys\n",
                &mappings
            ),
            Some((doc_line, doc_line + "import os\n".len()))
        );
    }

    #[test]
    fn edit_mapper_drops_structure_breaking_edits_in_prefixed_blocks() {
        let input = "> ```python\n> import os\n> x = 1\n> ```\n";
        let (linted, mappings) = prefixed_block_mapping(input);
        let line = linted.find("import os\n").unwrap();

        // Whole-line deletion absorbs the newline without restoring it:
        // applying it would orphan the next line's `> ` prefix.
        assert_eq!(
            map_concatenated_edit_to_original(
                &linted,
                line,
                line + "import os\n".len(),
                "",
                &mappings
            ),
            None
        );

        // An edit spanning two content lines would also cover the second
        // line's prefix bytes in the document.
        assert_eq!(
            map_concatenated_edit_to_original(
                &linted,
                line,
                linted.find("x = 1").unwrap() + 1,
                "y",
                &mappings
            ),
            None
        );

        // Inserted newlines would create prefix-less lines inside the
        // container.
        let start = linted.find("os").unwrap();
        assert_eq!(
            map_concatenated_edit_to_original(
                &linted,
                start,
                start + 2,
                "os\nimport sys",
                &mappings
            ),
            None
        );
    }

    #[test]
    fn edit_mapper_keeps_multiline_edits_for_unprefixed_blocks() {
        let input = "```python\nimport os\nx = 1\n```\n";
        let (linted, mappings) = prefixed_block_mapping(input);

        // Content is byte-identical to the document, so a multi-line
        // deletion maps through unchanged.
        let line = linted.find("import os\n").unwrap();
        let doc_line = input.find("import os\n").unwrap();
        let len = "import os\nx = 1\n".len();
        assert_eq!(
            map_concatenated_edit_to_original(&linted, line, line + len, "", &mappings),
            Some((doc_line, doc_line + len))
        );
    }

    #[test]
    fn test_registry_contains_linters() {
        let registry = ExternalLinterRegistry::new();
        assert!(registry.get("jarl").is_some());
        assert!(registry.get("arity").is_some());
        assert!(registry.get("fatou").is_some());
        assert!(registry.get("badness").is_some());
        assert!(registry.get("ruff").is_some());
        assert!(registry.get("eslint").is_some());
        assert!(registry.get("staticcheck").is_some());
        assert!(registry.get("clippy").is_some());
        assert!(registry.get("shellcheck").is_some());
    }

    #[test]
    fn test_registry_linter_language_support() {
        let registry = ExternalLinterRegistry::new();
        assert_eq!(registry.supports_language("jarl", "r"), Some(true));
        assert_eq!(registry.supports_language("jarl", "bash"), Some(false));
        assert_eq!(registry.supports_language("arity", "r"), Some(true));
        assert_eq!(registry.supports_language("arity", "julia"), Some(false));
        assert_eq!(registry.supports_language("fatou", "julia"), Some(true));
        assert_eq!(registry.supports_language("fatou", "jl"), Some(true));
        assert_eq!(registry.supports_language("fatou", "r"), Some(false));
        assert_eq!(registry.supports_language("badness", "latex"), Some(true));
        assert_eq!(registry.supports_language("badness", "tex"), Some(true));
        assert_eq!(registry.supports_language("badness", "r"), Some(false));
        assert_eq!(registry.supports_language("ruff", "python"), Some(true));
        assert_eq!(registry.supports_language("eslint", "js"), Some(true));
        assert_eq!(
            registry.supports_language("eslint", "typescript"),
            Some(true)
        );
        assert_eq!(registry.supports_language("eslint", "python"), Some(false));
        assert_eq!(registry.supports_language("staticcheck", "go"), Some(true));
        assert_eq!(
            registry.supports_language("staticcheck", "golang"),
            Some(true)
        );
        assert_eq!(
            registry.supports_language("staticcheck", "python"),
            Some(false)
        );
        assert_eq!(registry.supports_language("clippy", "rust"), Some(true));
        assert_eq!(registry.supports_language("clippy", "rs"), Some(true));
        assert_eq!(registry.supports_language("clippy", "go"), Some(false));
        assert_eq!(registry.supports_language("shellcheck", "bash"), Some(true));
        assert_eq!(registry.supports_language("shellcheck", "sh"), Some(true));
        assert_eq!(
            registry.supports_language("shellcheck", "python"),
            Some(false)
        );
        assert_eq!(registry.supports_language("unknown", "r"), None);
    }

    #[test]
    fn test_file_suffix_for_language_covers_julia() {
        assert_eq!(file_suffix_for_language("julia"), Some(".jl"));
        assert_eq!(file_suffix_for_language("jl"), Some(".jl"));
        assert_eq!(file_suffix_for_language("Julia"), Some(".jl"));
    }

    #[test]
    fn test_file_suffix_for_language_covers_latex() {
        assert_eq!(file_suffix_for_language("latex"), Some(".tex"));
        assert_eq!(file_suffix_for_language("tex"), Some(".tex"));
        assert_eq!(file_suffix_for_language("LaTeX"), Some(".tex"));
    }

    #[test]
    fn test_create_linter_temp_input_cleanup_removes_sibling_artifacts() {
        let temp_dir_path;
        {
            let (temp_dir, temp_path) =
                create_linter_temp_input("rust", "fn main() { let _x = 1; }\n").unwrap();
            temp_dir_path = temp_dir.path().to_path_buf();

            assert!(temp_path.exists());

            let sibling_artifact = temp_dir.path().join("input");
            std::fs::write(&sibling_artifact, b"compiled artifact").unwrap();
            assert!(sibling_artifact.exists());
        }

        assert!(!temp_dir_path.exists());
    }

    #[test]
    fn test_shellcheck_language_maps_to_explicit_shell() {
        assert_eq!(shellcheck_shell_for_language("sh"), "sh");
        assert_eq!(shellcheck_shell_for_language("shell"), "sh");
        assert_eq!(shellcheck_shell_for_language("bash"), "bash");
        assert_eq!(shellcheck_shell_for_language("ksh"), "ksh");
        assert_eq!(shellcheck_shell_for_language("zsh"), "sh");
    }

    #[test]
    fn test_append_language_specific_args_adds_shellcheck_shell_flag() {
        let mut cmd = std::process::Command::new("shellcheck");
        append_language_specific_args(&mut cmd, "shellcheck", "bash");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--shell".to_string(), "bash".to_string()]);
    }

    #[test]
    fn missing_linter_warning_message_sorts_and_deduplicates_commands() {
        let missing = HashSet::from([
            "ruff".to_string(),
            "shellcheck".to_string(),
            "ruff".to_string(),
        ]);
        let message = missing_linter_warning_message(&missing).expect("message expected");
        assert_eq!(
            message,
            "External linter command(s) not found: ruff, shellcheck. Configured external linting for these tools will be skipped."
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn find_missing_linter_commands_uses_registry_commands() {
        let registry = ExternalLinterRegistry::new();
        let missing =
            find_missing_linter_commands(["ruff", "definitely_unknown_linter"], &registry);
        // Unknown linter names are ignored here; they are handled by runner warnings.
        assert!(!missing.contains("definitely_unknown_linter"));
    }
}
