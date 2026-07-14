use pretty_assertions::assert_eq;

use super::ValidationSeverity;
use super::validate_markdown_response;

fn response(language: &str, source: &str) -> String {
    format!("```{language}\n{source}\n```\n\nExpected stdout: ok")
}

#[test]
fn supported_grammars_parse_portable_programs() {
    let cases = [
        (
            "c",
            "#include <stdio.h>\nint main(void) { puts(\"ok\"); return 0; }",
        ),
        (
            "cpp",
            "#include <iostream>\nint main() { std::cout << \"ok\\n\"; }",
        ),
        ("rust", "fn main() { println!(\"ok\"); }"),
        (
            "go",
            "package main\nimport \"fmt\"\nfunc main() { fmt.Println(\"ok\") }",
        ),
        ("swift", "print(\"ok\")"),
        ("javascript", "const value = \"ok\"; console.log(value);"),
        (
            "typescript",
            "const value: string = \"ok\"; console.log(value);",
        ),
        ("tsx", "const node = <span>ok</span>; console.log(node);"),
        ("python", "print(\"ok\")"),
    ];

    for (language, source) in cases {
        let validation = validate_markdown_response(&response(language, source));
        assert!(
            validation.valid,
            "{language} validation failed: {:?}",
            validation.issues
        );
        assert_eq!(validation.blocks.len(), 1);
        assert!(validation.blocks[0].stdout_detected);
    }
}

#[test]
fn tree_sitter_syntax_errors_are_rejected() {
    let validation =
        validate_markdown_response(&response("rust", "fn main( { println!(\"ok\"); }"));

    assert!(!validation.valid);
    assert!(
        validation
            .issues
            .iter()
            .any(|issue| issue.code == "syntax_error")
    );
}

#[test]
fn offline_network_and_filesystem_escapes_are_rejected() {
    let network = validate_markdown_response(&response(
        "javascript",
        "fetch(\"https://example.com\").then(console.log);",
    ));
    let filesystem = validate_markdown_response(&response(
        "python",
        "with open(\"/tmp/result.txt\", \"w\") as output:\n    output.write(\"ok\")\nprint(\"ok\")",
    ));
    let os_header = validate_markdown_response(&response(
        "cpp",
        "#include \"windows.h\"\n#include <iostream>\nint main() { std::cout << \"ok\\n\"; }",
    ));

    assert!(
        network
            .issues
            .iter()
            .any(|issue| issue.code == "network_access")
    );
    assert!(
        filesystem
            .issues
            .iter()
            .any(|issue| issue.code == "filesystem_escape")
    );
    assert!(
        os_header
            .issues
            .iter()
            .any(|issue| issue.code == "nonstandard_header")
    );
}

#[test]
fn output_contract_requires_fence_language_print_and_description() {
    let unsupported = validate_markdown_response("```bash\necho ok\n```\nExpected stdout: ok");
    let no_print = validate_markdown_response(&response("rust", "fn main() {}"));
    let no_description = validate_markdown_response("```python\nprint(\"ok\")\n```");
    let unfenced = validate_markdown_response("fn main() { println!(\"ok\"); }");

    assert!(
        unsupported
            .issues
            .iter()
            .any(|issue| issue.code == "unsupported_language")
    );
    assert!(
        no_print
            .issues
            .iter()
            .any(|issue| issue.code == "missing_stdout_write")
    );
    assert!(
        no_description
            .issues
            .iter()
            .any(|issue| issue.code == "missing_stdout_description")
    );
    assert!(
        unfenced
            .issues
            .iter()
            .any(|issue| issue.code == "unfenced_code")
    );
}

#[test]
fn prose_only_responses_do_not_require_an_execution_contract() {
    let validation = validate_markdown_response("The task completed without executable code.");

    assert_eq!(
        validation,
        super::CodeValidation {
            valid: true,
            contains_code: false,
            expected_stdout_description: true,
            blocks: Vec::new(),
            issues: Vec::new(),
        }
    );
}

#[test]
fn external_script_dependencies_are_rejected() {
    let validation =
        validate_markdown_response(&response("python", "import numpy\nprint(numpy.array([1]))"));

    assert!(validation.issues.iter().any(|issue| {
        issue.severity == ValidationSeverity::Error && issue.code == "external_dependency"
    }));
}
