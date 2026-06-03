//! Convert an Apple Notes note body into Tolaria markdown.
//!
//! Apple stores a note body as one plain string plus an ordered list of
//! formatting runs. Each run spans a number of UTF-16 code units of the string
//! (Apple counts in UTF-16, not Unicode scalars, because it uses `NSString`),
//! carrying one paragraph (block) style and a set of inline styles. The runs
//! tile the string end to end: a run's span begins where the previous run's
//! span ended.
//!
//! This module is the fidelity core, kept deliberately free of the protobuf and
//! SQLite plumbing. The decode slice maps the generated protobuf types onto the
//! domain types here; everything below is pure and exhaustively testable.
//!
//! Known limitations recorded for later slices: markdown special characters in
//! note text are not escaped; embedded objects (tables, attachments) and nested
//! numbered-list counters are handled by their own slices.

/// Paragraph (block) style, mirroring Apple's `style_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStyle {
    /// Default paragraph (`style_type` -1).
    Body,
    /// Note title (`style_type` 0) — the note's single H1.
    Title,
    /// Heading (`style_type` 1).
    Heading,
    /// Subheading (`style_type` 2).
    Subheading,
    /// Monospaced block (`style_type` 4) — rendered as a fenced code block.
    Monospaced,
    /// Bulleted list (`style_type` 100).
    BulletedList,
    /// Dashed list (`style_type` 101).
    DashedList,
    /// Numbered list (`style_type` 102).
    NumberedList,
    /// Checklist item (`style_type` 103) with its done state.
    Checklist { done: bool },
}

/// Text baseline, mirroring Apple's `superscript` sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Baseline {
    /// On the baseline.
    #[default]
    Normal,
    /// Raised (superscript).
    Super,
    /// Lowered (subscript).
    Sub,
}

/// Inline styling applied to a run's text span.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InlineStyle {
    /// Bold (`font_weight` bold bit).
    pub bold: bool,
    /// Italic (`font_weight` italic bit).
    pub italic: bool,
    /// Strikethrough.
    pub strikethrough: bool,
    /// Underline.
    pub underline: bool,
    /// Super/subscript.
    pub baseline: Baseline,
    /// External link target, if this run is a link.
    pub link: Option<String>,
}

/// One formatting run over the note text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeRun {
    /// Span length in UTF-16 code units.
    pub length: usize,
    /// Block style of the paragraph this run belongs to.
    pub block: BlockStyle,
    /// Inline styling for this run's span.
    pub inline: InlineStyle,
    /// Blockquote flag on the paragraph.
    pub blockquote: bool,
    /// List/paragraph indent depth.
    pub indent: u32,
}

/// A reconstructed logical line: its block style and the styled spans on it.
struct Line<'a> {
    block: BlockStyle,
    blockquote: bool,
    indent: u32,
    spans: Vec<(String, &'a InlineStyle)>,
}

/// Convert a note body (`text` + `runs`) into a markdown string.
pub fn note_to_markdown(text: &str, runs: &[AttributeRun]) -> String {
    let lines = split_into_lines(&tile(text, runs));
    render_lines(&lines)
}

/// Slice `text` into `(span_text, run)` pairs by tiling runs over UTF-16 units.
fn tile<'a>(text: &str, runs: &'a [AttributeRun]) -> Vec<(String, &'a AttributeRun)> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut segments = Vec::new();
    let mut pos = 0usize;
    for run in runs {
        if pos >= units.len() {
            break;
        }
        let end = pos.saturating_add(run.length).min(units.len());
        segments.push((String::from_utf16_lossy(&units[pos..end]), run));
        pos = end;
    }
    if pos < units.len() {
        // Text the runs did not cover renders as default body text.
        segments.push((String::from_utf16_lossy(&units[pos..]), &DEFAULT_RUN));
    }
    segments
}

/// A default body run used for text not covered by any provided run.
static DEFAULT_RUN: AttributeRun = AttributeRun {
    length: 0,
    block: BlockStyle::Body,
    inline: InlineStyle {
        bold: false,
        italic: false,
        strikethrough: false,
        underline: false,
        baseline: Baseline::Normal,
        link: None,
    },
    blockquote: false,
    indent: 0,
};

/// Break tiled segments into logical lines on `\n` boundaries.
fn split_into_lines<'a>(segments: &[(String, &'a AttributeRun)]) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    let mut current = new_line();
    for (slice, run) in segments {
        let parts: Vec<&str> = slice.split('\n').collect();
        for (index, part) in parts.iter().enumerate() {
            if !part.is_empty() {
                current.block = run.block;
                current.blockquote = run.blockquote;
                current.indent = run.indent;
                current.spans.push(((*part).to_string(), &run.inline));
            }
            if index < parts.len() - 1 {
                lines.push(std::mem::replace(&mut current, new_line()));
            }
        }
    }
    if !current.spans.is_empty() {
        lines.push(current);
    }
    lines
}

fn new_line<'a>() -> Line<'a> {
    Line {
        block: BlockStyle::Body,
        blockquote: false,
        indent: 0,
        spans: Vec::new(),
    }
}

/// Render reconstructed lines, grouping consecutive monospace lines into one
/// fenced code block and numbering consecutive numbered-list items.
fn render_lines(lines: &[Line]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut number = 1u32;
    let mut in_code = false;
    for line in lines {
        let is_code = line.block == BlockStyle::Monospaced;
        if is_code && !in_code {
            out.push("```".to_string());
            in_code = true;
        } else if !is_code && in_code {
            out.push("```".to_string());
            in_code = false;
        }

        if line.block == BlockStyle::NumberedList {
            out.push(render_numbered(line, number));
            number += 1;
        } else {
            number = 1;
            out.push(render_line(line, in_code));
        }
    }
    if in_code {
        out.push("```".to_string());
    }
    out.join("\n")
}

fn render_numbered(line: &Line, number: u32) -> String {
    format!(
        "{}{number}. {}",
        indent_prefix(line.indent),
        inline_text(line)
    )
}

/// Render one non-numbered line. Inside a code fence, emit raw text.
fn render_line(line: &Line, in_code: bool) -> String {
    if in_code {
        return raw_text(line);
    }
    let content = inline_text(line);
    let prefix = block_prefix(line.block, line.indent);
    let body = format!("{prefix}{content}");
    if line.blockquote {
        format!("> {body}")
    } else {
        body
    }
}

/// The markdown prefix for a block style (lists/headings/checklists).
fn block_prefix(block: BlockStyle, indent: u32) -> String {
    match block {
        BlockStyle::Title => "# ".to_string(),
        BlockStyle::Heading => "## ".to_string(),
        BlockStyle::Subheading => "### ".to_string(),
        BlockStyle::BulletedList | BlockStyle::DashedList => format!("{}- ", indent_prefix(indent)),
        BlockStyle::Checklist { done } => {
            let mark = if done { "x" } else { " " };
            format!("{}- [{mark}] ", indent_prefix(indent))
        }
        BlockStyle::Body | BlockStyle::Monospaced | BlockStyle::NumberedList => String::new(),
    }
}

fn indent_prefix(indent: u32) -> String {
    "  ".repeat(indent as usize)
}

/// Concatenate a line's spans with inline markdown applied.
fn inline_text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|(text, style)| render_inline(text, style))
        .collect()
}

/// Concatenate a line's spans as raw text (for code fences).
fn raw_text(line: &Line) -> String {
    line.spans.iter().map(|(text, _)| text.as_str()).collect()
}

/// Apply inline styling to a text span.
fn render_inline(text: &str, style: &InlineStyle) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut rendered = match style.baseline {
        Baseline::Super => format!("<sup>{text}</sup>"),
        Baseline::Sub => format!("<sub>{text}</sub>"),
        Baseline::Normal => text.to_string(),
    };
    if style.strikethrough {
        rendered = format!("~~{rendered}~~");
    }
    if style.underline {
        rendered = format!("<u>{rendered}</u>");
    }
    rendered = apply_emphasis(rendered, style.bold, style.italic);
    if let Some(url) = &style.link {
        rendered = format!("[{rendered}]({url})");
    }
    rendered
}

fn apply_emphasis(text: String, bold: bool, italic: bool) -> String {
    match (bold, italic) {
        (true, true) => format!("***{text}***"),
        (true, false) => format!("**{text}**"),
        (false, true) => format!("*{text}*"),
        (false, false) => text,
    }
}

#[cfg(test)]
mod tests {
    use super::{note_to_markdown, AttributeRun, Baseline, BlockStyle, InlineStyle};

    fn run(text_units: usize, block: BlockStyle) -> AttributeRun {
        AttributeRun {
            length: text_units,
            block,
            inline: InlineStyle::default(),
            blockquote: false,
            indent: 0,
        }
    }

    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    #[test]
    fn plain_paragraph_passes_through() {
        let text = "hello world";
        let runs = vec![run(utf16_len(text), BlockStyle::Body)];
        assert_eq!(note_to_markdown(text, &runs), "hello world");
    }

    #[test]
    fn title_becomes_h1() {
        let text = "My Note\nbody";
        let runs = vec![
            run(utf16_len("My Note\n"), BlockStyle::Title),
            run(utf16_len("body"), BlockStyle::Body),
        ];
        assert_eq!(note_to_markdown(text, &runs), "# My Note\nbody");
    }

    #[test]
    fn heading_and_subheading() {
        let text = "Big\nMid";
        let runs = vec![
            run(utf16_len("Big\n"), BlockStyle::Heading),
            run(utf16_len("Mid"), BlockStyle::Subheading),
        ];
        assert_eq!(note_to_markdown(text, &runs), "## Big\n### Mid");
    }

    #[test]
    fn bold_italic_and_combined() {
        let text = "abc";
        let mut bold = run(1, BlockStyle::Body);
        bold.inline.bold = true;
        let mut italic = run(1, BlockStyle::Body);
        italic.inline.italic = true;
        let mut both = run(1, BlockStyle::Body);
        both.inline.bold = true;
        both.inline.italic = true;
        let runs = vec![bold, italic, both];
        assert_eq!(note_to_markdown(text, &runs), "**a***b****c***");
    }

    #[test]
    fn strikethrough_underline_super_sub() {
        let text = "wxyz";
        let mut strike = run(1, BlockStyle::Body);
        strike.inline.strikethrough = true;
        let mut under = run(1, BlockStyle::Body);
        under.inline.underline = true;
        let mut sup = run(1, BlockStyle::Body);
        sup.inline.baseline = Baseline::Super;
        let mut sub = run(1, BlockStyle::Body);
        sub.inline.baseline = Baseline::Sub;
        let runs = vec![strike, under, sup, sub];
        assert_eq!(
            note_to_markdown(text, &runs),
            "~~w~~<u>x</u><sup>y</sup><sub>z</sub>"
        );
    }

    #[test]
    fn external_link_wraps_styled_text() {
        let text = "site";
        let mut link = run(utf16_len(text), BlockStyle::Body);
        link.inline.bold = true;
        link.inline.link = Some("https://example.com".to_string());
        let runs = vec![link];
        assert_eq!(
            note_to_markdown(text, &runs),
            "[**site**](https://example.com)"
        );
    }

    #[test]
    fn bulleted_and_dashed_lists() {
        let text = "one\ntwo";
        let runs = vec![
            run(utf16_len("one\n"), BlockStyle::BulletedList),
            run(utf16_len("two"), BlockStyle::DashedList),
        ];
        assert_eq!(note_to_markdown(text, &runs), "- one\n- two");
    }

    #[test]
    fn numbered_list_increments_then_resets() {
        let text = "a\nb\nplain\nc";
        let runs = vec![
            run(utf16_len("a\n"), BlockStyle::NumberedList),
            run(utf16_len("b\n"), BlockStyle::NumberedList),
            run(utf16_len("plain\n"), BlockStyle::Body),
            run(utf16_len("c"), BlockStyle::NumberedList),
        ];
        assert_eq!(note_to_markdown(text, &runs), "1. a\n2. b\nplain\n1. c");
    }

    #[test]
    fn checklist_done_and_undone() {
        let text = "todo\ndone";
        let runs = vec![
            run(utf16_len("todo\n"), BlockStyle::Checklist { done: false }),
            run(utf16_len("done"), BlockStyle::Checklist { done: true }),
        ];
        assert_eq!(note_to_markdown(text, &runs), "- [ ] todo\n- [x] done");
    }

    #[test]
    fn monospace_lines_group_into_one_fence() {
        let text = "let x = 1\nlet y = 2";
        let runs = vec![
            run(utf16_len("let x = 1\n"), BlockStyle::Monospaced),
            run(utf16_len("let y = 2"), BlockStyle::Monospaced),
        ];
        assert_eq!(
            note_to_markdown(text, &runs),
            "```\nlet x = 1\nlet y = 2\n```"
        );
    }

    #[test]
    fn blockquote_prefixes_line() {
        let text = "quoted";
        let mut quote = run(utf16_len(text), BlockStyle::Body);
        quote.blockquote = true;
        let runs = vec![quote];
        assert_eq!(note_to_markdown(text, &runs), "> quoted");
    }

    #[test]
    fn nested_bullet_indents() {
        let text = "top\nchild";
        let mut child = run(utf16_len("child"), BlockStyle::BulletedList);
        child.indent = 1;
        let runs = vec![run(utf16_len("top\n"), BlockStyle::BulletedList), child];
        assert_eq!(note_to_markdown(text, &runs), "- top\n  - child");
    }

    #[test]
    fn utf16_tiling_handles_astral_emoji() {
        // A musical-symbol emoji is two UTF-16 code units; the run length must
        // count those units, not one Unicode scalar, or the tiling desyncs.
        let emoji = "\u{1D11E}"; // 𝄞, 2 UTF-16 units
        let text = format!("{emoji}b");
        let mut bold = run(2, BlockStyle::Body);
        bold.inline.bold = true;
        let plain = run(1, BlockStyle::Body);
        let runs = vec![bold, plain];
        assert_eq!(note_to_markdown(&text, &runs), format!("**{emoji}**b"));
    }

    #[test]
    fn inline_styles_compose_within_one_line() {
        let text = "Hello bold!";
        let plain = run(utf16_len("Hello "), BlockStyle::Body);
        let mut bold = run(utf16_len("bold"), BlockStyle::Body);
        bold.inline.bold = true;
        let bang = run(utf16_len("!"), BlockStyle::Body);
        let runs = vec![plain, bold, bang];
        assert_eq!(note_to_markdown(text, &runs), "Hello **bold**!");
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(note_to_markdown("", &[]), "");
    }

    #[test]
    fn text_beyond_runs_falls_back_to_body() {
        let text = "covered uncovered";
        let runs = vec![run(utf16_len("covered "), BlockStyle::Body)];
        assert_eq!(note_to_markdown(text, &runs), "covered uncovered");
    }
}
