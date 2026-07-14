use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::code_policy::SandboxLanguage;
use crate::code_policy::ValidationIssue;
use crate::code_policy::error_issue;

pub(crate) fn inspect_syntax_tree(
    language: SandboxLanguage,
    source: &str,
    root: Node<'_>,
    start_line: usize,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let line = start_line + node.start_position().row;
            if is_import_node(kind) {
                inspect_import(language, text, line, &mut issues);
            }
            if kind.contains("string") {
                inspect_string_literal(text, line, &mut issues);
            }
            if is_call_node(kind) {
                inspect_call(text, line, &mut issues);
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }

    let mut seen = BTreeSet::new();
    issues.retain(|issue| seen.insert((issue.code.clone(), issue.line, issue.message.clone())));
    issues
}

fn is_import_node(kind: &str) -> bool {
    kind.contains("import")
        || matches!(
            kind,
            "use_declaration" | "extern_crate_declaration" | "preproc_include"
        )
}

fn is_call_node(kind: &str) -> bool {
    kind.contains("call_expression")
        || matches!(
            kind,
            "call" | "macro_invocation" | "function_call_expression"
        )
}

fn inspect_import(
    language: SandboxLanguage,
    text: &str,
    line: usize,
    issues: &mut Vec<ValidationIssue>,
) {
    match language {
        SandboxLanguage::C => inspect_c_include(text, line, /*cpp*/ false, issues),
        SandboxLanguage::Cpp => inspect_c_include(text, line, /*cpp*/ true, issues),
        SandboxLanguage::Rust => inspect_rust_import(text, line, issues),
        SandboxLanguage::Go => inspect_go_import(text, line, issues),
        SandboxLanguage::Swift => inspect_swift_import(text, line, issues),
        SandboxLanguage::JavaScript | SandboxLanguage::TypeScript | SandboxLanguage::Tsx => {
            inspect_script_import(text, line, issues);
        }
        SandboxLanguage::Python => inspect_python_import(text, line, issues),
    }
}

fn inspect_c_include(text: &str, line: usize, cpp: bool, issues: &mut Vec<ValidationIssue>) {
    let normalized = text.to_ascii_lowercase();
    if [
        "windows.h",
        "winsock",
        "unistd.h",
        "sys/socket.h",
        "arpa/inet.h",
        "dlfcn.h",
        "curl/curl.h",
    ]
    .iter()
    .any(|header| normalized.contains(header))
    {
        issues.push(error_issue(
            "nonstandard_header",
            "OS-specific or networking headers are unavailable in the WASI sandbox",
            Some(line),
        ));
        return;
    }
    let Some(header) = text
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>').map(|(header, _)| header))
    else {
        return;
    };
    const C_HEADERS: &[&str] = &[
        "assert.h",
        "ctype.h",
        "errno.h",
        "float.h",
        "inttypes.h",
        "limits.h",
        "locale.h",
        "math.h",
        "setjmp.h",
        "signal.h",
        "stdalign.h",
        "stdarg.h",
        "stdatomic.h",
        "stdbool.h",
        "stddef.h",
        "stdint.h",
        "stdio.h",
        "stdlib.h",
        "stdnoreturn.h",
        "string.h",
        "tgmath.h",
        "threads.h",
        "time.h",
        "uchar.h",
        "wchar.h",
        "wctype.h",
    ];
    const CPP_HEADERS: &[&str] = &[
        "algorithm",
        "array",
        "atomic",
        "bit",
        "bitset",
        "charconv",
        "cassert",
        "cctype",
        "cerrno",
        "cfenv",
        "cfloat",
        "cinttypes",
        "climits",
        "clocale",
        "chrono",
        "cmath",
        "compare",
        "complex",
        "concepts",
        "csetjmp",
        "csignal",
        "cstdarg",
        "cstddef",
        "cstdint",
        "cstdio",
        "cstdlib",
        "cstring",
        "ctgmath",
        "ctime",
        "cuchar",
        "cwchar",
        "cwctype",
        "deque",
        "exception",
        "filesystem",
        "format",
        "forward_list",
        "fstream",
        "functional",
        "future",
        "initializer_list",
        "iomanip",
        "ios",
        "iosfwd",
        "iostream",
        "istream",
        "iterator",
        "limits",
        "list",
        "map",
        "memory",
        "mutex",
        "new",
        "numbers",
        "numeric",
        "optional",
        "ostream",
        "queue",
        "random",
        "ranges",
        "ratio",
        "regex",
        "set",
        "source_location",
        "span",
        "sstream",
        "stack",
        "stdexcept",
        "streambuf",
        "string",
        "string_view",
        "system_error",
        "tuple",
        "type_traits",
        "typeindex",
        "typeinfo",
        "unordered_map",
        "unordered_set",
        "utility",
        "valarray",
        "variant",
        "vector",
    ];
    if !(C_HEADERS.contains(&header) || cpp && CPP_HEADERS.contains(&header)) {
        issues.push(error_issue(
            "nonstandard_header",
            format!("header <{header}> is not part of the permitted standard library set"),
            Some(line),
        ));
    }
}

fn inspect_rust_import(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    let normalized = text.trim().to_ascii_lowercase();
    if [
        "use std::",
        "use std;",
        "use core::",
        "use core;",
        "use alloc::",
        "use alloc;",
        "use crate::",
        "use crate;",
        "use self::",
        "use self;",
        "use super::",
        "use super;",
        "extern crate alloc",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix))
    {
        return;
    }
    issues.push(error_issue(
        "external_dependency",
        "Rust sandbox code may import only std, core, alloc, or local crate modules",
        Some(line),
    ));
}

fn inspect_go_import(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    for import in quoted_values(text) {
        let normalized = import.to_ascii_lowercase();
        if normalized == "net"
            || normalized.starts_with("net/")
            || matches!(
                normalized.as_str(),
                "os/exec" | "plugin" | "runtime/cgo" | "syscall"
            )
        {
            issues.push(error_issue(
                "network_or_native_api",
                format!("Go import `{import}` is unavailable in the offline WASI sandbox"),
                Some(line),
            ));
        } else if normalized
            .split('/')
            .next()
            .is_some_and(|part| part.contains('.'))
        {
            issues.push(error_issue(
                "external_dependency",
                format!("Go import `{import}` requires an external module"),
                Some(line),
            ));
        }
    }
}

fn inspect_swift_import(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    let module = text.split_whitespace().nth(1).unwrap_or_default().trim();
    if module != "Swift" {
        issues.push(error_issue(
            "external_dependency",
            format!("Swift module `{module}` is not available to the portable WASI program"),
            Some(line),
        ));
    }
}

fn inspect_script_import(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    for specifier in quoted_values(text) {
        if !specifier.starts_with("./") || specifier.contains("../") || specifier.contains("..\\") {
            issues.push(error_issue(
                "external_dependency",
                format!("script import `{specifier}` is not a local /sandbox module"),
                Some(line),
            ));
        }
    }
}

fn inspect_python_import(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    const SAFE_MODULES: &[&str] = &[
        "base64",
        "bisect",
        "collections",
        "csv",
        "dataclasses",
        "datetime",
        "decimal",
        "enum",
        "functools",
        "hashlib",
        "heapq",
        "io",
        "itertools",
        "json",
        "math",
        "os",
        "pathlib",
        "random",
        "re",
        "statistics",
        "string",
        "textwrap",
        "typing",
    ];
    let trimmed = text.trim();
    let modules = if let Some(rest) = trimmed.strip_prefix("from ") {
        rest.split_whitespace()
            .next()
            .into_iter()
            .collect::<Vec<_>>()
    } else if let Some(rest) = trimmed.strip_prefix("import ") {
        rest.split(',')
            .filter_map(|part| part.split_whitespace().next())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for module in modules {
        let root = module.split('.').next().unwrap_or(module);
        if !SAFE_MODULES.contains(&root) {
            issues.push(error_issue(
                "external_dependency",
                format!("Python module `{root}` is not in the sandbox standard-library allowlist"),
                Some(line),
            ));
        }
    }
}

fn inspect_call(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    let normalized = text.to_ascii_lowercase();
    if normalized.contains("import(") {
        let local_import = quoted_values(text)
            .into_iter()
            .next()
            .is_some_and(|specifier| {
                specifier.starts_with("./")
                    && !specifier.contains("../")
                    && !specifier.contains("..\\")
            });
        if !local_import {
            issues.push(error_issue(
                "external_dependency",
                "dynamic imports must resolve to a local module beneath /sandbox/",
                Some(line),
            ));
        }
    }
    const BANNED_CALLS: &[(&str, &str, &str)] = &[
        (
            "fetch(",
            "network_access",
            "fetch is unavailable because the sandbox is offline",
        ),
        (
            "require(",
            "external_dependency",
            "CommonJS package loading is unavailable in the isolated script sandbox",
        ),
        (
            "xmlhttprequest",
            "network_access",
            "XMLHttpRequest is unavailable because the sandbox is offline",
        ),
        (
            "websocket",
            "network_access",
            "WebSocket is unavailable because the sandbox is offline",
        ),
        (
            "urlsession",
            "network_access",
            "URLSession is unavailable because the sandbox is offline",
        ),
        (
            "requests.",
            "network_access",
            "Python requests cannot be used in the offline sandbox",
        ),
        (
            "urllib.request",
            "network_access",
            "urllib network requests are unavailable in the offline sandbox",
        ),
        (
            "reqwest",
            "network_access",
            "reqwest cannot be used in the offline WASI sandbox",
        ),
        (
            "std::net",
            "network_access",
            "std::net is unavailable in the offline WASI sandbox",
        ),
        (
            "tokio::net",
            "network_access",
            "tokio networking is unavailable in the offline WASI sandbox",
        ),
        (
            "tcpstream",
            "network_access",
            "TCP sockets are unavailable in the offline WASI sandbox",
        ),
        (
            "udp",
            "network_access",
            "UDP sockets are unavailable in the offline WASI sandbox",
        ),
        (
            "subprocess.",
            "native_process",
            "subprocesses are unavailable in the isolated sandbox",
        ),
        (
            "system(",
            "native_process",
            "host shell commands are unavailable in the isolated sandbox",
        ),
        (
            "popen(",
            "native_process",
            "host processes are unavailable in the isolated sandbox",
        ),
        (
            "fork(",
            "native_process",
            "host process creation is unavailable in the isolated sandbox",
        ),
        (
            "deno.connect",
            "network_access",
            "Deno networking is unavailable because the sandbox is offline",
        ),
        (
            "process::command",
            "native_process",
            "host processes are unavailable in the WASI sandbox",
        ),
        (
            "std::process",
            "native_process",
            "host processes are unavailable in the WASI sandbox",
        ),
        (
            "child_process",
            "native_process",
            "host processes are unavailable in the isolated sandbox",
        ),
        (
            "ctypes.",
            "native_extension",
            "native C extensions are unavailable in the script sandbox",
        ),
        (
            "libc::",
            "native_extension",
            "native libc bindings are not portable to the WASI contract",
        ),
        (
            "winapi",
            "os_specific_api",
            "Windows APIs are not portable to wasm32-wasip2",
        ),
    ];
    for (needle, code, message) in BANNED_CALLS {
        if normalized.contains(needle) {
            issues.push(error_issue(*code, *message, Some(line)));
        }
    }
}

fn inspect_string_literal(text: &str, line: usize, issues: &mut Vec<ValidationIssue>) {
    let Some(value) = string_value(text) else {
        return;
    };
    let normalized = value.replace("\\\\", "\\");
    if normalized.starts_with("http://") || normalized.starts_with("https://") {
        issues.push(error_issue(
            "network_access",
            "remote URLs are unavailable because the sandbox is offline",
            Some(line),
        ));
    }
    let windows_absolute = normalized
        .as_bytes()
        .get(1)
        .is_some_and(|separator| *separator == b':')
        || normalized.starts_with("\\\\");
    let outside_posix_root = normalized.starts_with('/')
        && normalized != "/sandbox"
        && !normalized.starts_with("/sandbox/");
    if windows_absolute
        || outside_posix_root
        || normalized.contains("../")
        || normalized.contains("..\\")
    {
        issues.push(error_issue(
            "filesystem_escape",
            format!("path `{value}` escapes the isolated /sandbox/ filesystem"),
            Some(line),
        ));
    }
}

fn quoted_values(text: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(['\'', '"']) {
        let quote = remaining.as_bytes()[start] as char;
        let after_quote = &remaining[start + 1..];
        let Some(end) = after_quote.find(quote) else {
            break;
        };
        values.push(&after_quote[..end]);
        remaining = &after_quote[end + 1..];
    }
    values
}

fn string_value(text: &str) -> Option<&str> {
    quoted_values(text).into_iter().next()
}
