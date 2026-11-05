//! Basic languages: declarations and name-level callers with the enclosing function or method,
//! read through the grammar's shipped tags query where it serves (C#, PHP, Ruby) and a short
//! query of our own where it does not (C, C++). No resolution and, beyond C/C++ includes, no
//! imports.
use crate::syntax::{Language, Vocabulary};

pub const C: Language = Language {
    name: "c",
    provider: "c-tree-sitter-0.24.2/v1",
    extensions: &["c", "h"],
    grammar: || tree_sitter_c::LANGUAGE.into(),
    // The tags query cites a function by its signature, not its body, so calls would have no
    // enclosing function; a short query of our own reads C instead.
    query: || include_str!("../queries/c.scm").into(),
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "c:declares-function",
        "c:declares-macro",
        "c:declares-type",
        "c:imports",
        "c:invokes-syntax",
    ],
    import: None,
};

pub const CPP: Language = Language {
    name: "cpp",
    provider: "cpp-tree-sitter-0.23.4/v1",
    extensions: &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
    grammar: || tree_sitter_cpp::LANGUAGE.into(),
    query: || include_str!("../queries/cpp.scm").into(),
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "cpp:declares-function",
        "cpp:declares-macro",
        "cpp:declares-method",
        "cpp:declares-module",
        "cpp:declares-type",
        "cpp:imports",
        "cpp:invokes-syntax",
    ],
    import: None,
};

pub const CSHARP: Language = Language {
    name: "csharp",
    provider: "csharp-tree-sitter-0.23.5/tags-v1",
    extensions: &["cs"],
    grammar: || tree_sitter_c_sharp::LANGUAGE.into(),
    // The tags query names member calls (`a.F()`) only.
    query: || {
        [
            tree_sitter_c_sharp::TAGS_QUERY,
            "(invocation_expression function: (identifier) @name) @reference.call",
        ]
        .join("\n")
    },
    vocabulary: Vocabulary::Tags,
    kinds: &[
        "csharp:declares-class",
        "csharp:declares-interface",
        "csharp:declares-method",
        "csharp:declares-module",
        "csharp:invokes-syntax",
    ],
    import: None,
};

pub const PHP: Language = Language {
    name: "php",
    provider: "php-tree-sitter-0.25.1/tags-v1",
    extensions: &["php"],
    grammar: || tree_sitter_php::LANGUAGE_PHP.into(),
    // The tags query names qualified function calls only, not `helper()`.
    query: || {
        [
            tree_sitter_php::TAGS_QUERY,
            "(function_call_expression function: (name) @name) @reference.call",
        ]
        .join("\n")
    },
    vocabulary: Vocabulary::Tags,
    kinds: &[
        "php:declares-class",
        "php:declares-field",
        "php:declares-function",
        "php:declares-interface",
        "php:declares-module",
        "php:invokes-syntax",
    ],
    import: None,
};

pub const RUBY: Language = Language {
    name: "ruby",
    provider: "ruby-tree-sitter-0.23.1/tags-v2",
    extensions: &["rb"],
    grammar: || tree_sitter_ruby::LANGUAGE.into(),
    // The tags query also names every bare identifier a call and leaves local variables out
    // through tree-sitter-tags' locals tracking, which this extractor does not run: parameters
    // and locals would read as calls. Its definitions stay; calls are those with a method.
    query: || {
        let tags = tree_sitter_ruby::TAGS_QUERY;
        let definitions = tags
            .split_once("; Calls")
            .map_or(tags, |(definitions, _)| definitions);
        [
            definitions,
            "(call method: (identifier) @name) @reference.call",
        ]
        .join("\n")
    },
    vocabulary: Vocabulary::Tags,
    kinds: &[
        "ruby:declares-class",
        "ruby:declares-method",
        "ruby:declares-module",
        "ruby:invokes-syntax",
    ],
    import: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::tests::rows;

    fn owners(
        language: &Language,
        path: &str,
        source: &str,
    ) -> Vec<(String, String, Option<String>)> {
        let result = language.extract(path, source.as_bytes()).unwrap();
        assert!(!result.partial, "{path}");
        let mut rows: Vec<_> = rows(&result, ":")
            .into_iter()
            .map(|(k, n, o)| (k.to_string(), n.to_string(), o.map(str::to_string)))
            .collect();
        rows.sort();
        rows
    }

    fn row(kind: &str, name: &str, owner: Option<&str>) -> (String, String, Option<String>) {
        (kind.into(), name.into(), owner.map(Into::into))
    }

    #[test]
    fn each_language_declares_and_cites_calls_in_their_function() {
        assert!(
            owners(
                &C,
                "a.c",
                "int helper(void);\nint run(struct s *p) { helper(); return p->go(); }\n"
            )
            .contains(&row("c:invokes-syntax", "go", Some("run")))
        );
        assert!(
            owners(&CPP, "a.cpp", "class A { void f() { ns::g(); } };\n").contains(&row(
                "cpp:invokes-syntax",
                "g",
                Some("f")
            ))
        );
        let cs = owners(
            &CSHARP,
            "A.cs",
            "namespace N { class A { void F() { G(); this.H(); } } }\n",
        );
        for expected in [
            row("csharp:declares-class", "A", None),
            row("csharp:invokes-syntax", "G", Some("F")),
            row("csharp:invokes-syntax", "H", Some("F")),
        ] {
            assert!(cs.contains(&expected), "{expected:?} in {cs:?}");
        }
        assert!(
            owners(&PHP, "a.php", "<?php\nfunction run() { helper(); }\n").contains(&row(
                "php:invokes-syntax",
                "helper",
                Some("run")
            ))
        );
        let ruby = owners(
            &RUBY,
            "a.rb",
            "class Cart\n  def total\n    apply_discount(items)\n  end\nend\n",
        );
        assert!(
            ruby.contains(&row("ruby:declares-class", "Cart", None)),
            "{ruby:?}"
        );
        assert!(
            ruby.contains(&row("ruby:invokes-syntax", "apply_discount", Some("total"))),
            "{ruby:?}"
        );
        // Parameters, locals and a method's own name are not calls.
        let calls: Vec<_> = owners(
            &RUBY,
            "b.rb",
            "def a(value)\n  x = value\n  x\nend\n\ndef b(value)\n  a(value) + cart.total\nend\n",
        )
        .into_iter()
        .filter(|(kind, _, _)| kind == "ruby:invokes-syntax")
        .collect();
        assert_eq!(
            calls,
            [
                row("ruby:invokes-syntax", "a", Some("b")),
                row("ruby:invokes-syntax", "total", Some("b"))
            ]
        );
    }
}
