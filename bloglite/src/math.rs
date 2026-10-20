//! Render Markdown's math code elements to MathML without a JavaScript runtime.
use math_core::{LatexToMathML, MathCoreConfig, MathDisplay};
use std::sync::LazyLock;

static MATH: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?s)<pre><code class="language-math(?: math-display)?">([^<]*)</code></pre>|<code class="language-math(?: math-inline)?">([^<]*)</code>"#).unwrap()
});

pub fn html(html: &str) -> String {
    // One converter per document; local state prevents macros leaking between equations.
    let converter = LatexToMathML::new(MathCoreConfig::default()).unwrap();
    MATH.replace_all(html, |caps: &regex::Captures<'_>| {
        let block = caps.get(1).is_some();
        let source = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str();
        // Decode the Markdown compiler's entities exactly once.
        let source = source
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        let display = if block {
            MathDisplay::Block
        } else {
            MathDisplay::Inline
        };
        match converter.convert_with_local_state(&source, display) {
            Ok(result) => result.mathml,
            Err(_) => caps[0].to_owned(), // Preserve readable, escaped TeX on errors.
        }
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_posts_and_previews() {
        let source = "$a < b$\n\n$$\n\\frac{1}{2} + \\sqrt{x}\n$$";
        let html = crate::md::content_to_html(source);
        assert_eq!(html.matches("<math").count(), 2);
        assert!(html.contains("<mfrac>"));
        assert!(html.contains("<msqrt>"));
        assert!(html.contains("display=\"block\""));
        let mut post = crate::data::Post {
            id: 1,
            content: source.into(),
            created: chrono::Utc::now(),
            updated: chrono::Utc::now(),
        };
        crate::md::preview(&mut post, 600);
        assert_eq!(post.content.matches("<math").count(), 2);
        assert!(post.content.contains("<mfrac>"));
    }

    #[test]
    fn errors_preserve_escaped_source_and_code_is_untouched() {
        let source = "$\\notacommand{<script>} $";
        let html = crate::md::content_to_html(source);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<math"));
        assert_eq!(
            crate::md::content_to_html("`$x$`"),
            "<p><code>$x$</code></p>"
        );
        let html = crate::md::content_to_html("```math\nx^2\n```");
        assert!(html.contains("<math"));
    }
}
