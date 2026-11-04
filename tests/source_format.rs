use rill::{
    formatter,
    lexer::{self, Kind},
    parser,
    source::{LexemeKind, Position, SourceFile},
};
use std::{fs, path::Path, process::Command};

fn assert_lossless(source: &str) {
    let file = SourceFile::lex(source).unwrap();
    assert_eq!(file.text(), source);
    assert_eq!(file.reconstruct(), source);
    let mut position = Position {
        byte: 0,
        line: 1,
        column: 1,
    };
    let mut tokens = Vec::new();
    for piece in file.lexemes() {
        assert_eq!(piece.span().start, position);
        assert_eq!(&source[piece.span().byte_range()], piece.text());
        if let LexemeKind::Token(kind) = piece.kind() {
            tokens.push(lexer::Token {
                kind: kind.clone(),
                line: position.line,
                column: position.column,
            });
        }
        for ch in piece.text().chars() {
            position.byte += ch.len_utf8();
            if ch == '\n' {
                position.line += 1;
                position.column = 1;
            } else {
                position.column += 1;
            }
        }
        assert_eq!(piece.span().end, position);
    }
    assert_eq!(position.byte, source.len());
    assert_eq!(tokens, lexer::lex(source).unwrap());
    assert_eq!(
        file.lexemes().last().unwrap().kind(),
        &LexemeKind::Token(Kind::Eof)
    );
}

fn formatted(source: &str) -> String {
    let output = formatter::format(source).unwrap_or_else(|error| panic!("{error}\n{source}"));
    assert_eq!(
        parser::parse_module(source).unwrap(),
        parser::parse_module(&output).unwrap()
    );
    assert_eq!(formatter::format(&output).unwrap(), output);
    assert_lossless(&output);
    output
}

#[test]
fn source_round_trip_retains_unicode_crlf_tabs_escapes_and_comments() {
    for source in [
        "", " \t\r\n\u{2003}\n", "// λ🦀\t  \r\n// eof   ",
        "\u{2003}fn main(){\r\n\tlet s=\"é🦀\\n\\r\\t\\0\\\\\\\"\"; // λ  \r\n  c\"é\\n\\r\\t\\\\\\\"\"\n}",
        "@include(\"header.h\"); export extern fn take(p: const_ptr<Thing>, x: i32) -> unit;",
        "let mut move inout read record extern const fn type match while do in if then else true false @ {}|=> -> <- : . (),; + - * / % = == != < <= > >= && || _ name 00042",
    ] {
        assert_lossless(source);
    }
    let source = "// 🦀\r\n\t\"é🦀\" + 7";
    let tokens = lexer::lex(source).unwrap();
    assert_eq!((tokens[0].line, tokens[0].column), (2, 2));
    assert_eq!((tokens[1].line, tokens[1].column), (2, 7));
    assert_eq!((tokens[2].line, tokens[2].column), (2, 9));
}

#[test]
fn lossless_lexer_keeps_original_error_positions() {
    for (source, message) in [
        ("// λ\r\n\t?", "2:2: unexpected character '?'"),
        ("\"é\" λ", "1:5: unexpected character 'λ'"),
        ("fn main() { \"\\q\" }", "1:13: unsupported string escape"),
        ("c\"\\0\"", "1:1: unsupported string escape"),
        ("\"hi", "1:1: unterminated string"),
    ] {
        assert_eq!(SourceFile::lex(source).unwrap_err(), message);
        assert_eq!(lexer::lex(source).unwrap_err(), message);
    }
}

#[test]
fn formats_blocks_statements_and_inline_comments_usefully() {
    let input = "  fn main(){let mut x=  40; // keep  λ  \n x<-x+2; if x==42 {x} else{0}}  ";
    assert_eq!(formatted(input), "fn main() {\n    let mut x = 40; // keep  λ  \n    x <- x + 2;\n    if x == 42 {\n        x\n    } else {\n        0\n    }\n}\n");
}

#[test]
fn keeps_comment_text_order_and_inline_or_standalone_placement() {
    let source = "// lead 🦀  \r\n\nfn main(){// opening\n// standalone\n let x=1;// semicolon\n x // tail\n}// closing\n// final no newline  ";
    let output = formatted(source);
    let comments = |text: &str| {
        SourceFile::lex(text)
            .unwrap()
            .lexemes()
            .iter()
            .filter(|piece| matches!(piece.kind(), LexemeKind::LineComment))
            .map(|piece| piece.text().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(comments(source), comments(&output));
    assert!(output.contains("{ // opening\n"));
    assert!(output.contains("    // standalone\n"));
    assert!(output.contains("let x = 1; // semicolon\n"));
    assert!(output.ends_with("// final no newline  \n"));
}

#[test]
fn retains_literal_spellings_without_normalizing_strings_or_c_strings() {
    let input =
        "fn main(){let n=00042; let s=\"é🦀\\0\\t\\n\\r\\\\\\\"\";c\"utf8 🦀\\n\\t\\\\\\\"\"}";
    let output = formatted(input);
    assert!(output.contains("00042"));
    assert!(output.contains("\"é🦀\\0\\t\\n\\r\\\\\\\"\""));
    assert!(output.contains("c\"utf8 🦀\\n\\t\\\\\\\"\""));
}

#[test]
fn preserves_legacy_calls_constructor_adjacency_and_negative_arguments() {
    for expr in [
        "id(-1)",
        "id (- 1)",
        "id\n(-1)",
        "id((-(1)))",
        "id(-2147483648)",
        "id(0--1)",
    ] {
        let source = format!("let id x=x\nlet main = {expr}");
        let output = formatted(&source);
        let p = parser::parse(&source).unwrap();
        let q = parser::parse(&output).unwrap();
        assert_eq!(
            rill::interpreter::run(&p).unwrap(),
            rill::interpreter::run(&q).unwrap()
        );
        if expr.starts_with("id(") {
            assert!(output.contains("= id("));
        }
        if expr.starts_with("id (") {
            assert!(output.contains("= id ("));
        }
        if expr.starts_with("id\n(") {
            assert!(output.contains("= id\n("));
        }
    }
    formatted("type T=Some(i32)|None\nlet main=match Some (-1){Some (x)=>x|None=>0}");
    formatted(
        "let bump (inout x: i32) = x <- x + 1\nlet main = let mut x=41 in { bump(inout x); x }",
    );
}

#[test]
fn formats_nested_matches_records_types_and_native_modules_without_loading_them() {
    for source in [
        "type O<a> =None|Some(a) type R={x:O<O<i32>>;y:i32} fn main(){match Some(Some(42)){Some(Some(x))=>{x}|_=>{0}}}",
        "@include(\"not-installed.h\");@pkg_config(\"not-installed\");@link(\"missing\");import missing.module as m; export extern fn f(p:const_ptr<Missing>)->ptr<Other>;export extern const K:u32;export fn invoke(){f(c\"ok\")}",
        "type P={x:i32;y:i32} fn main(){let p=record P{x=42;y=0};p.x}",
        "fn f(\n x,\n y\n){x+y} fn main(){f(\n 40,\n 2\n)}",
    ] {
        formatted(source);
    }
}

#[test]
fn empty_sources_empty_blocks_and_final_semicolons_are_preserved() {
    assert_eq!(formatted("\r\n\t "), "");
    assert_eq!(formatted("// only  "), "// only  \n");
    assert_eq!(formatted("fn main(){ }"), "fn main() {}\n");
    for source in [
        "fn main(){42;}",
        "fn main(){let x=42;}",
        "fn main(){ {}; () }",
    ] {
        let output = formatted(source);
        assert_eq!(
            rill::interpreter::run(&parser::parse(&output).unwrap())
                .unwrap()
                .to_string(),
            "()"
        );
        assert_eq!(source.matches(';').count(), output.matches(';').count());
    }
}

#[test]
fn rejects_malformed_unsupported_and_too_deep_source() {
    for source in [
        "fn main(){1 2}",
        "fn main(){",
        "fn main(){if true {1} else }",
        "fn main(){/* no block comments */42}",
        "fn main(){\"unterminated}",
        "fn main(){c\"\\0\"}",
        "fn main(){\"bad\\q\"}",
        "fn main(){\"two\nlines\"}",
        "fn main(){λ}",
        "fn main(){999999999999999999999}",
        "fn main(){1} import too.late;",
        "@unknown(\"x\"); fn main(){42}",
    ] {
        let error = formatter::format(source).unwrap_err();
        assert!(error.starts_with("E-FMT-SYNTAX:"), "{error}");
    }
    let deep = format!("fn main(){{{}42{}}}", "(".repeat(1000), ")".repeat(1000));
    assert!(formatter::format(&deep)
        .unwrap_err()
        .contains("E-PARSE-DEPTH"));
}

fn corpus(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            corpus(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rill") {
            files.push(path);
        }
    }
}

#[test]
fn repository_corpus_round_trips_preserves_parsing_and_formats_idempotently() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for directory in ["examples", "stdlib", "tests/fixtures"] {
        corpus(&root.join(directory), &mut files);
    }
    assert!(files.len() >= 38);
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        assert_lossless(&source);
        let output = formatter::format(&source)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            parser::parse_module(&source).unwrap(),
            parser::parse_module(&output).unwrap(),
            "{}",
            path.display()
        );
        assert_eq!(
            formatter::format(&output).unwrap(),
            output,
            "{}",
            path.display()
        );
    }
    eprintln!(
        "formatted and losslessly reconstructed {} repository source files",
        files.len()
    );
}

#[test]
fn original_and_formatted_execution_agree_in_interpreter_and_c_o0_o2() {
    for (case, source) in [
        include_str!("../examples/blocks/main.rill"),
        "type T=Some(i32)|None let id x=x let main=match Some (id(-42)){Some (x)=>0-x|None=>0}",
        "fn main(){let x=system_box_i32_abort(42);box_read(x)}",
    ]
    .iter()
    .enumerate()
    {
        let output = formatted(source);
        let dir =
            std::env::temp_dir().join(format!("rill-format-exec-{}-{case}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        for (stage, text) in [("original", *source), ("formatted", output.as_str())] {
            let program = parser::parse(text).unwrap();
            assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
            let c_file = dir.join(format!("{stage}.c"));
            fs::write(&c_file, rill::compile(text).unwrap()).unwrap();
            for optimization in ["-O0", "-O2"] {
                let binary = dir.join(format!("{stage}{optimization}"));
                let build = Command::new("cc")
                    .args(["-std=c11", optimization])
                    .arg(&c_file)
                    .arg("-o")
                    .arg(&binary)
                    .output()
                    .unwrap();
                assert!(
                    build.status.success(),
                    "{}",
                    String::from_utf8_lossy(&build.stderr)
                );
                let result = Command::new(&binary).output().unwrap();
                assert!(result.status.success());
                assert_eq!(result.stdout, b"42\n");
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
