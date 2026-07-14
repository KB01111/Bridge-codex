use serde::Serialize;
use tree_sitter::Language;
use tree_sitter::Node;
use tree_sitter::Parser;

use codex_code_memory::Language as CodeMemoryLanguage;

use crate::code_policy_checks::inspect_syntax_tree;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_CODE_BLOCK_BYTES: usize = 32 * 1024;
const MAX_CODE_BLOCKS: usize = 8;

pub const SANDBOX_EXECUTION_SYSTEM_PROMPT: &str = r#"When a response contains executable code, obey this sandbox contract:
- Put every complete program in a clean fenced Markdown code block with an explicit language tag: c, cpp, rust, go, swift, javascript, typescript, tsx, or python.
- C, C++, Rust, Go, and Swift code must use portable standard-language facilities and be compatible with the wasm32-wasip2 target. Do not use OS-specific APIs, FFI, native extensions, subprocesses, or external system libraries.
- JavaScript, TypeScript, and Python code must use only standard, pure-language facilities. Do not require packages with native C extensions.
- The runtime is offline. Do not make network requests or use fetch, sockets, HTTP clients, WebSockets, or remote imports.
- The only filesystem root is /sandbox/. Use relative paths within the sandbox or absolute paths beginning with /sandbox/. Never access parent directories or host paths.
- The program must write its verifiable result to stdout with the language's normal print facility.
- After the code fence, include a short line beginning with `Expected stdout:` that describes the expected console output.
Do not place executable code outside a fenced block."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SandboxLanguage {
    C,
    Cpp,
    Rust,
    Go,
    Swift,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
}

impl SandboxLanguage {
    fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().as_str() {
            "c" => Some(Self::C),
            "cpp" | "c++" => Some(Self::Cpp),
            "rust" | "rs" => Some(Self::Rust),
            "go" | "golang" => Some(Self::Go),
            "swift" => Some(Self::Swift),
            "javascript" | "js" => Some(Self::JavaScript),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "python" | "py" => Some(Self::Python),
            _ => None,
        }
    }

    pub(crate) fn tag(self) -> &'static str {
        self.code_memory_language().tag()
    }

    fn parser_language(self) -> Language {
        self.code_memory_language().tree_sitter_language()
    }

    fn code_memory_language(self) -> CodeMemoryLanguage {
        match self {
            Self::C => CodeMemoryLanguage::C,
            Self::Cpp => CodeMemoryLanguage::Cpp,
            Self::Rust => CodeMemoryLanguage::Rust,
            Self::Go => CodeMemoryLanguage::Go,
            Self::Swift => CodeMemoryLanguage::Swift,
            Self::JavaScript => CodeMemoryLanguage::JavaScript,
            Self::TypeScript => CodeMemoryLanguage::TypeScript,
            Self::Tsx => CodeMemoryLanguage::Tsx,
            Self::Python => CodeMemoryLanguage::Python,
        }
    }

    fn target(self) -> Option<&'static str> {
        match self {
            Self::C | Self::Cpp | Self::Rust | Self::Go | Self::Swift => Some("wasm32-wasip2"),
            Self::JavaScript | Self::TypeScript | Self::Tsx | Self::Python => None,
        }
    }

    fn writes_stdout(self, source: &str) -> bool {
        let source = source.to_ascii_lowercase();
        match self {
            Self::C => ["printf(", "puts(", "putchar("]
                .iter()
                .any(|call| source.contains(call)),
            Self::Cpp => ["std::cout", "std::println(", "printf(", "puts("]
                .iter()
                .any(|call| source.contains(call)),
            Self::Rust => ["println!(", "print!(", "std::io::stdout("]
                .iter()
                .any(|call| source.contains(call)),
            Self::Go => ["fmt.print(", "fmt.println(", "fmt.printf("]
                .iter()
                .any(|call| source.contains(call)),
            Self::Swift => source.contains("print("),
            Self::JavaScript | Self::TypeScript | Self::Tsx => {
                ["console.log(", "console.info(", "console.error("]
                    .iter()
                    .any(|call| source.contains(call))
            }
            Self::Python => source.contains("print("),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub code: String,
    pub message: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedCodeBlock {
    pub language: String,
    pub target: Option<String>,
    pub root_kind: String,
    pub source_bytes: usize,
    pub stdout_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeValidation {
    pub valid: bool,
    pub contains_code: bool,
    pub expected_stdout_description: bool,
    pub blocks: Vec<ValidatedCodeBlock>,
    pub issues: Vec<ValidationIssue>,
}

impl CodeValidation {
    pub fn failure_summary(&self) -> String {
        self.issues
            .iter()
            .filter(|issue| issue.severity == ValidationSeverity::Error)
            .take(3)
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodePolicyStatus {
    parser: String,
    language_abi_version: usize,
    supported_languages: Vec<String>,
    compiled_target: String,
    network_access: String,
    sandbox_root: String,
    response_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxValidationEvent {
    pub request_id: String,
    pub validation: CodeValidation,
}

struct FencedBlock {
    tag: String,
    source: String,
    start_line: usize,
}

struct FenceExtraction {
    blocks: Vec<FencedBlock>,
    prose: String,
    issues: Vec<ValidationIssue>,
}

pub fn validate_markdown_response(response: &str) -> CodeValidation {
    if response.len() > MAX_RESPONSE_BYTES {
        return CodeValidation {
            valid: false,
            contains_code: response.contains("```"),
            expected_stdout_description: false,
            blocks: Vec::new(),
            issues: vec![error_issue(
                "response_too_large",
                format!("response exceeds the {MAX_RESPONSE_BYTES}-byte validation limit"),
                /*line*/ None,
            )],
        };
    }

    let FenceExtraction {
        blocks: fenced_blocks,
        prose,
        mut issues,
    } = extract_fenced_blocks(response);
    let contains_code = !fenced_blocks.is_empty();
    let expected_stdout_description = !contains_code
        || prose.lines().any(|line| {
            line.trim_start()
                .to_ascii_lowercase()
                .starts_with("expected stdout:")
        });
    if contains_code && !expected_stdout_description {
        issues.push(error_issue(
            "missing_stdout_description",
            "add a short `Expected stdout:` description outside the code fence",
            /*line*/ None,
        ));
    }
    if !contains_code {
        let executable_markers = [
            "#include ",
            "fn main(",
            "int main(",
            "func main(",
            "package main",
            "def main(",
            "console.log(",
            "print(",
        ];
        if response.lines().map(str::trim).any(|line| {
            let line = line.to_ascii_lowercase();
            executable_markers
                .iter()
                .any(|marker| line.starts_with(marker))
        }) {
            issues.push(error_issue(
                "unfenced_code",
                "executable code must be contained in a fenced Markdown code block",
                /*line*/ None,
            ));
        }
    }

    let mut blocks = Vec::new();
    for block in fenced_blocks {
        let Some(language) = SandboxLanguage::from_tag(&block.tag) else {
            issues.push(error_issue(
                "unsupported_language",
                format!(
                    "unsupported or missing fenced-code language `{}`",
                    block.tag
                ),
                Some(block.start_line.saturating_sub(1)),
            ));
            blocks.push(ValidatedCodeBlock {
                language: block.tag,
                target: None,
                root_kind: "unsupported".to_string(),
                source_bytes: block.source.len(),
                stdout_detected: false,
            });
            continue;
        };
        if block.source.len() > MAX_CODE_BLOCK_BYTES {
            issues.push(error_issue(
                "code_block_too_large",
                format!(
                    "{} code block exceeds the {MAX_CODE_BLOCK_BYTES}-byte validation limit",
                    language.tag()
                ),
                Some(block.start_line),
            ));
            continue;
        }

        let mut parser = Parser::new();
        if let Err(error) = parser.set_language(&language.parser_language()) {
            issues.push(error_issue(
                "grammar_incompatible",
                format!("failed to load the {} grammar: {error}", language.tag()),
                Some(block.start_line),
            ));
            continue;
        }
        let Some(tree) = parser.parse(&block.source, None) else {
            issues.push(error_issue(
                "parse_cancelled",
                format!("Tree-sitter did not parse the {} block", language.tag()),
                Some(block.start_line),
            ));
            continue;
        };
        let root = tree.root_node();
        if root.has_error() {
            let line = first_parse_error(root)
                .map(|node| block.start_line + node.start_position().row)
                .unwrap_or(block.start_line);
            issues.push(error_issue(
                "syntax_error",
                format!("{} code contains a syntax error", language.tag()),
                Some(line),
            ));
        }
        issues.extend(inspect_syntax_tree(
            language,
            &block.source,
            root,
            block.start_line,
        ));

        let stdout_detected = language.writes_stdout(&block.source);
        if !stdout_detected {
            issues.push(error_issue(
                "missing_stdout_write",
                format!(
                    "{} program does not write a result to stdout",
                    language.tag()
                ),
                Some(block.start_line),
            ));
        }
        blocks.push(ValidatedCodeBlock {
            language: language.tag().to_string(),
            target: language.target().map(str::to_string),
            root_kind: root.kind().to_string(),
            source_bytes: block.source.len(),
            stdout_detected,
        });
    }

    let valid = !issues
        .iter()
        .any(|issue| issue.severity == ValidationSeverity::Error);
    CodeValidation {
        valid,
        contains_code,
        expected_stdout_description,
        blocks,
        issues,
    }
}

fn extract_fenced_blocks(response: &str) -> FenceExtraction {
    let mut blocks = Vec::new();
    let mut prose = Vec::new();
    let mut issues = Vec::new();
    let mut open: Option<(String, usize, Vec<&str>)> = None;

    for (line_index, line) in response.lines().enumerate() {
        let trimmed = line.trim();
        if let Some((tag, start_line, source_lines)) = open.as_mut() {
            if trimmed == "```" {
                blocks.push(FencedBlock {
                    tag: std::mem::take(tag),
                    source: source_lines.join("\n"),
                    start_line: *start_line,
                });
                open = None;
            } else {
                source_lines.push(line);
            }
            continue;
        }

        if let Some(info) = trimmed.strip_prefix("```") {
            if blocks.len() >= MAX_CODE_BLOCKS {
                issues.push(error_issue(
                    "too_many_code_blocks",
                    format!("response contains more than {MAX_CODE_BLOCKS} code blocks"),
                    Some(line_index + 1),
                ));
                continue;
            }
            let mut info_parts = info.split_whitespace();
            let tag = info_parts.next().unwrap_or_default().to_string();
            if info_parts.next().is_some() {
                issues.push(error_issue(
                    "invalid_fence_info",
                    "code fences may contain only a language tag",
                    Some(line_index + 1),
                ));
            }
            open = Some((tag, line_index + 2, Vec::new()));
        } else {
            prose.push(line);
        }
    }

    if let Some((tag, start_line, source_lines)) = open {
        blocks.push(FencedBlock {
            tag,
            source: source_lines.join("\n"),
            start_line,
        });
        issues.push(error_issue(
            "unclosed_code_fence",
            "close the executable code block with three backticks",
            Some(start_line.saturating_sub(1)),
        ));
    }

    FenceExtraction {
        blocks,
        prose: prose.join("\n"),
        issues,
    }
}

fn first_parse_error(root: Node<'_>) -> Option<Node<'_>> {
    let mut stack = vec![root];
    let mut first = None;
    while let Some(node) = stack.pop() {
        if (node.is_error() || node.is_missing())
            && first.is_none_or(|current: Node<'_>| node.start_byte() < current.start_byte())
        {
            first = Some(node);
        }
        for index in 0..node.child_count() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
    first
}

pub(crate) fn error_issue(
    code: impl Into<String>,
    message: impl Into<String>,
    line: Option<usize>,
) -> ValidationIssue {
    ValidationIssue {
        severity: ValidationSeverity::Error,
        code: code.into(),
        message: message.into(),
        line,
    }
}

#[tauri::command]
pub fn get_code_policy_status() -> CodePolicyStatus {
    CodePolicyStatus {
        parser: "tree-sitter".to_string(),
        language_abi_version: tree_sitter::LANGUAGE_VERSION,
        supported_languages: [
            "C",
            "C++",
            "Rust",
            "Go",
            "Swift",
            "JavaScript",
            "TypeScript / TSX",
            "Python",
        ]
        .map(str::to_string)
        .to_vec(),
        compiled_target: "wasm32-wasip2".to_string(),
        network_access: "offline".to_string(),
        sandbox_root: "/sandbox/".to_string(),
        response_contract: "fenced code plus Expected stdout".to_string(),
    }
}

#[tauri::command]
pub fn validate_sandbox_response(response: String) -> CodeValidation {
    validate_markdown_response(&response)
}

#[cfg(test)]
#[path = "code_policy_tests.rs"]
mod tests;
