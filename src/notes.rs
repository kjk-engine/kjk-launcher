//! Release notes in GitHub's Markdown, made into plain lines for the Play
//! page: headings, bullets and sentences, without the Markdown marks.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A line that started with one or more `#`.
    Heading(String),
    /// A line that started with "- " or "* ".
    Bullet(String),
    Text(String),
    /// One or more empty lines between paragraphs.
    Gap,
}

/// The lines of `markdown`. HTML comments are dropped, `**bold**` and
/// backticks lose their marks, and a link `[text](url)` keeps only its
/// text. Empty lines at the start and the end are dropped, and several
/// empty lines in a row become one gap.
pub fn lines(markdown: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for raw in without_comments(markdown).lines() {
        let line = raw.trim();
        let next = if line.is_empty() {
            if matches!(out.last(), None | Some(Line::Gap)) {
                continue;
            }
            Line::Gap
        } else if line.starts_with('#') {
            Line::Heading(plain(line.trim_start_matches('#').trim()))
        } else if let Some(rest) = line.strip_prefix("- ").or(line.strip_prefix("* ")) {
            Line::Bullet(plain(rest.trim()))
        } else {
            Line::Text(plain(line))
        };
        out.push(next);
    }
    if out.last() == Some(&Line::Gap) {
        out.pop();
    }
    out
}

/// `markdown` without its `<!-- … -->` comments. A comment that is never
/// closed runs to the end.
fn without_comments(markdown: &str) -> String {
    let mut out = String::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = match rest[start..].find("-->") {
            Some(end) => &rest[start + end + 3..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// A line's words without bold marks, backticks and link targets.
fn plain(line: &str) -> String {
    let line = line.replace("**", "").replace('`', "");
    let mut out = String::new();
    let mut rest = line.as_str();
    while let Some(open) = rest.find('[') {
        let link = rest[open..].find("](").and_then(|middle| {
            let close = rest[open + middle..].find(')')?;
            Some((open + middle, open + middle + close))
        });
        let Some((middle, close)) = link else { break };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..middle]);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_line() {
        let cases = [
            ("## What is new", vec![Line::Heading("What is new".into())]),
            (
                "- Faster loading",
                vec![Line::Bullet("Faster loading".into())],
            ),
            (
                "* Faster loading",
                vec![Line::Bullet("Faster loading".into())],
            ),
            (
                "Saves are **much** smaller with `zstd`.",
                vec![Line::Text("Saves are much smaller with zstd.".into())],
            ),
            (
                "See [the guide](https://example.com/guide) first.",
                vec![Line::Text("See the guide first.".into())],
            ),
            (
                "<!-- hidden\nstill hidden -->Shown",
                vec![Line::Text("Shown".into())],
            ),
            (
                "\n\nOne\n\n\nTwo\n\n",
                vec![
                    Line::Text("One".into()),
                    Line::Gap,
                    Line::Text("Two".into()),
                ],
            ),
        ];
        for (markdown, expected) in cases {
            assert_eq!(lines(markdown), expected, "{markdown:?}");
        }
    }
}
