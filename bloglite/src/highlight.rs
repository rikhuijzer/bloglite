//! Server-side highlighting; syntax definitions and themes are bundled by syntect.
use std::sync::LazyLock;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static CODE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?s)<pre><code class="language-([^"<>]+)">([^<]*)</code></pre>"#).unwrap()
});
const STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "syntax-" };

pub fn escape(code: &str) -> String {
    code.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn html(html: &str) -> String {
    CODE.replace_all(html, |caps: &regex::Captures<'_>| {
        let language = &caps[1];
        if language.split_whitespace().next() == Some("math") {
            return caps[0].to_string();
        }
        let Some(syntax) = SYNTAXES.find_syntax_by_token(language) else {
            return caps[0].to_string();
        };
        // Decode exactly the entities emitted by the Markdown compiler, once.
        let code = caps[2]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        // Keep CRLF together before syntax spans can split it into two HTML
        // line breaks.
        let code = code.replace("\r\n", "\n");
        let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, &SYNTAXES, STYLE);
        for line in LinesWithEndings::from(&code) {
            if generator
                .parse_html_for_line_which_includes_newline(line)
                .is_err()
            {
                return caps[0].to_string();
            }
        }
        format!(
            "<pre class=\"syntax-code\"><code class=\"language-{language}\">{}</code></pre>",
            generator.finalize()
        )
    })
    .into_owned()
}

pub fn stylesheet() -> &'static str {
    include_str!("static/highlight.css")
}

#[cfg(test)]
mod tests {

    fn text(html: &str) -> String {
        let tags = regex::Regex::new("<[^>]+>").unwrap();
        tags.replace_all(html, "")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    #[test]
    fn highlights_languages_and_preserves_code() {
        for (language, code) in [
            ("rust", "fn main() { println!(\"<&lt;> 🦀\"); }\n"),
            ("julia", "function f(x)\n    println(1)\nend"),
            ("typescript", "const x: number = 1;\n"),
        ] {
            let result = crate::md::content_to_html(&format!("```{language}\n{code}\n```"));
            assert!(result.contains("syntax-code"), "{language}: {result}");
            assert!(result.contains("<span"));
            assert_eq!(text(&result).trim_end(), code.trim_end());
        }
    }

    #[test]
    fn csv_crlf_does_not_add_blank_lines() {
        let expected = "fruit,height,width\napple,9.33,6.5\npear,12.45,8.4\n";
        for ending in ["\n", "\r\n"] {
            let code = expected.replace('\n', ending);
            let source = format!("```csv{ending}{code}```");
            let html = crate::md::content_to_html(&source);
            assert!(html.contains("syntax-code"));

            // HTML input preprocessing normalizes CRLF and lone CR before
            // parsing tags. Normalize before stripping tags: Syntect can put
            // spans between CR and LF, making them two visible line breaks.
            let browser_html = html.replace("\r\n", "\n").replace('\r', "\n");
            assert_eq!(text(&browser_html), expected, "line ending: {ending:?}");
        }
    }

    #[test]
    fn leaves_math_unknown_languages_and_inline_code_alone() {
        for source in [
            "```unknown-language\n<hi>\n```",
            "```\n<hi>\n```",
            "`inline`",
            "$$\nx = 1\n$$",
            "$x$",
        ] {
            let expected = markdown::to_html_with_options(source, &{
                let mut opts = markdown::Options::default();
                opts.parse.constructs.math_flow = true;
                opts.parse.constructs.math_text = true;
                opts
            })
            .unwrap();
            assert_eq!(super::html(&expected), expected);
        }
    }

    #[test]
    fn preview_escapes_and_highlights_code() {
        let mut post = crate::data::Post {
            id: 1,
            content: "```rust\nlet x = \"<script>&lt;\";\n```".into(),
            created: chrono::Utc::now(),
            updated: chrono::Utc::now(),
        };
        crate::md::preview(&mut post, 600);
        assert!(post.content.contains("syntax-code"));
        assert!(!post.content.contains("<script>"));
        assert_eq!(text(&post.content).trim(), "let x = \"<script>&lt;\";");
    }
}
