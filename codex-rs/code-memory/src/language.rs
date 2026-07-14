use serde::Deserialize;
use serde::Serialize;
use tree_sitter::Language as TreeSitterLanguage;

/// Languages supported by the bundled Tree-sitter parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    C,
    Cpp,
    Go,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Rust,
    Swift,
}

impl Language {
    /// Infers a supported language from a filename or relative path.
    pub fn from_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Some(Self::Cpp),
            "go" => Some(Self::Go),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "py" | "pyi" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            "swift" => Some(Self::Swift),
            _ => None,
        }
    }

    /// Returns the bundled grammar for use by hosts that share this registry.
    ///
    /// Keeping this mapping here prevents validation and indexing code from
    /// silently selecting different grammars for the same language tag.
    pub fn tree_sitter_language(self) -> TreeSitterLanguage {
        match self {
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
        }
    }

    /// Returns the stable lowercase tag used in persisted IDs and UI status.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Go => "go",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Swift => "swift",
        }
    }

    pub(crate) fn is_structure(self, kind: &str) -> bool {
        match self {
            Self::C => matches!(
                kind,
                "function_definition"
                    | "struct_specifier"
                    | "union_specifier"
                    | "enum_specifier"
                    | "type_definition"
            ),
            Self::Cpp => matches!(
                kind,
                "function_definition"
                    | "class_specifier"
                    | "struct_specifier"
                    | "union_specifier"
                    | "enum_specifier"
                    | "namespace_definition"
                    | "template_declaration"
                    | "type_definition"
            ),
            Self::Go => matches!(
                kind,
                "function_declaration"
                    | "method_declaration"
                    | "type_declaration"
                    | "const_declaration"
                    | "var_declaration"
            ),
            Self::JavaScript | Self::TypeScript | Self::Tsx => matches!(
                kind,
                "function_declaration"
                    | "generator_function_declaration"
                    | "class_declaration"
                    | "method_definition"
                    | "interface_declaration"
                    | "type_alias_declaration"
                    | "enum_declaration"
                    | "abstract_class_declaration"
                    | "lexical_declaration"
            ),
            Self::Python => matches!(
                kind,
                "function_definition" | "class_definition" | "decorated_definition"
            ),
            Self::Rust => matches!(
                kind,
                "function_item"
                    | "function_signature_item"
                    | "struct_item"
                    | "enum_item"
                    | "union_item"
                    | "trait_item"
                    | "impl_item"
                    | "mod_item"
                    | "type_item"
                    | "const_item"
                    | "static_item"
            ),
            Self::Swift => matches!(
                kind,
                "function_declaration"
                    | "class_declaration"
                    | "struct_declaration"
                    | "protocol_declaration"
                    | "enum_declaration"
                    | "extension_declaration"
                    | "typealias_declaration"
            ),
        }
    }

    pub(crate) fn identifier_kinds(self) -> &'static [&'static str] {
        match self {
            Self::C | Self::Cpp => &["identifier", "type_identifier", "field_identifier"],
            Self::Go => &["field_identifier", "type_identifier", "identifier"],
            Self::JavaScript | Self::TypeScript | Self::Tsx => {
                &["property_identifier", "type_identifier", "identifier"]
            }
            Self::Python => &["identifier"],
            Self::Rust => &["type_identifier", "identifier"],
            Self::Swift => &["type_identifier", "simple_identifier"],
        }
    }
}
