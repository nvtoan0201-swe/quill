//! Export: HTML, PDF (through a headless Chromium), DOCX and plain text.

use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd, html};

use crate::markdown;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Html,
    Pdf,
    Docx,
    Text,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Text => "txt",
        }
    }
}

/// Drop `// comment` lines; they are notes for the writer, not the reader.
pub fn strip_comments(md: &str) -> String {
    md.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("// ") || t.trim_end() == "//")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `==text==` → `<mark>text</mark>` inside text events.
fn mark_highlights(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find("==") {
        let after = &rest[a + 2..];
        match after.find("==") {
            Some(z) if z > 0 => {
                out.push_str(&escape(&rest[..a]));
                out.push_str("<mark>");
                out.push_str(&escape(&after[..z]));
                out.push_str("</mark>");
                rest = &after[z + 2..];
            }
            _ => break,
        }
    }
    out.push_str(&escape(rest));
    out
}

/// Rewrite table cells as raw HTML so MultiMarkdown `||` becomes `colspan`.
/// A cell with an empty source range continues the cell before it.
fn with_colspans<'a>(events: Vec<(Event<'a>, std::ops::Range<usize>)>) -> Vec<Event<'a>> {
    use pulldown_cmark::Alignment;
    let mut out = Vec::new();
    let mut aligns: Vec<Alignment> = Vec::new();
    let mut in_head = false;
    let mut i = 0;
    while i < events.len() {
        let (ev, _) = &events[i];
        match ev {
            Event::Start(Tag::Table(a)) => {
                aligns = a.clone();
                out.push(ev.clone());
            }
            Event::Start(Tag::TableHead) => {
                in_head = true;
                out.push(ev.clone());
            }
            Event::End(TagEnd::TableHead) => {
                in_head = false;
                out.push(ev.clone());
            }
            Event::Start(Tag::TableCell) => {
                // Gather this cell, then count following empty-range cells.
                let mut j = i + 1;
                let mut depth = 1;
                while j < events.len() && depth > 0 {
                    match events[j].0 {
                        Event::Start(Tag::TableCell) => depth += 1,
                        Event::End(TagEnd::TableCell) => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                let col = events[..i].iter().rev().take_while(|(e, _)| !matches!(e, Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead))).filter(|(e, _)| matches!(e, Event::Start(Tag::TableCell))).count();
                let mut span = 1;
                let mut k = j;
                while k < events.len() && matches!(events[k].0, Event::Start(Tag::TableCell)) && events[k].1.is_empty() {
                    span += 1;
                    k += 2; // empty cell: Start + End
                }
                let tag = if in_head { "th" } else { "td" };
                let align = match aligns.get(col) {
                    Some(Alignment::Left) => " style=\"text-align: left\"",
                    Some(Alignment::Center) => " style=\"text-align: center\"",
                    Some(Alignment::Right) => " style=\"text-align: right\"",
                    _ => "",
                };
                let colspan = if span > 1 { format!(" colspan=\"{span}\"") } else { String::new() };
                out.push(Event::InlineHtml(format!("<{tag}{align}{colspan}>").into()));
                out.extend(events[i + 1..j - 1].iter().map(|(e, _)| e.clone()));
                out.push(Event::InlineHtml(format!("</{tag}>").into()));
                i = k;
                continue;
            }
            _ => out.push(ev.clone()),
        }
        i += 1;
    }
    out
}

pub fn body_html(md: &str) -> String {
    let md = strip_comments(md);
    let raw: Vec<_> = Parser::new_ext(&md, markdown::options()).into_offset_iter().collect();
    let events = with_colspans(raw).into_iter().map(|ev| match ev {
        Event::Text(t) if t.contains("==") => Event::InlineHtml(mark_highlights(&t).into()),
        Event::InlineMath(m) => Event::InlineHtml(format!("<code class=\"math\">{}</code>", escape(&m)).into()),
        Event::DisplayMath(m) => Event::Html(format!("<pre class=\"math\">{}</pre>", escape(&m)).into()),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

pub const PREVIEW_CSS: &str = r#"
:root { color-scheme: light dark; }
html { background: #fcfcfc; color: #1a1a1a; }
body { margin: 0 auto; padding: 4em 2em; max-width: 40em;
  font-family: "iA Writer Quattro", "iA Writer Quattro S", "IBM Plex Sans", system-ui, sans-serif;
  font-size: 18px; line-height: 1.6; -webkit-font-smoothing: antialiased; }
h1, h2, h3, h4, h5, h6 { font-weight: 700; line-height: 1.25; margin: 1.6em 0 0.6em; }
h1 { font-size: 2em; } h2 { font-size: 1.5em; } h3 { font-size: 1.25em; } h4, h5, h6 { font-size: 1em; }
p, ul, ol, blockquote, pre, table { margin: 0 0 1.1em; }
a { color: inherit; text-decoration-color: rgba(128,128,128,.6); }
blockquote { margin-left: 0; padding-left: 1.2em; border-left: 3px solid #d6d6d6; font-style: italic; }
code, pre { font-family: "iA Writer Mono", "iA Writer Mono S", monospace; font-size: .9em; }
pre { background: rgba(36,36,36,.055); padding: .8em 1em; border-radius: 4px; overflow-x: auto; }
:not(pre) > code { background: rgba(36,36,36,.055); padding: .1em .3em; border-radius: 3px; }
hr { border: 0; border-top: 1px solid #d6d6d6; margin: 2em 0; }
mark { background: rgba(234,140,0,.29); color: #1a1a1a; border-bottom: .175em solid rgba(234,140,0,.43); padding: 0; }
table { border-collapse: collapse; width: 100%; }
th, td { border-bottom: 1px solid #d6d6d6; padding: .4em .6em; text-align: left; }
img { max-width: 100%; }
li > input[type=checkbox] { margin-right: .5em; }
@media (prefers-color-scheme: dark) {
  html { background: #101010; color: #cccccc; }
  blockquote, hr, th, td { border-color: #444; }
  pre, :not(pre) > code { background: rgba(245,245,245,.06); }
  mark { background: rgba(255,210,101,.17); color: #ffd265; border-bottom-color: rgba(255,210,101,.75); }
}
@media print { html { background: none; } body { padding: 0; font-size: 11pt; } }
"#;

pub fn full_html(md: &str, title: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>{}</style>\n</head>\n<body>\n{}</body>\n</html>\n",
        escape(title),
        PREVIEW_CSS,
        body_html(md)
    )
}

/// Markdown with markup removed (Print Plain Text, Copy as text).
pub fn plain_text(md: &str) -> String {
    let md = strip_comments(md);
    let mut out = String::new();
    for ev in Parser::new_ext(&md, markdown::options()) {
        match ev {
            Event::Text(t) | Event::Code(t) => out.push_str(&t.replace("==", "")),
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push('\n'),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock) => out.push_str("\n\n"),
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::Start(Tag::Item) => out.push_str("• "),
            Event::Rule => out.push_str("———\n\n"),
            _ => {}
        }
    }
    out.trim_end().to_string() + "\n"
}

fn chrome() -> Option<&'static str> {
    ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge"]
        .into_iter()
        .find(|c| Command::new("which").arg(c).output().is_ok_and(|o| o.status.success()))
}

pub fn pdf(md: &str, title: &str, out: &Path) -> Result<()> {
    let browser = chrome().context("PDF export needs Chrome or Chromium installed")?;
    let html_path = std::env::temp_dir().join(format!("quill-export-{}.html", std::process::id()));
    std::fs::write(&html_path, full_html(md, title))?;
    let status = Command::new(browser)
        .args(["--headless", "--disable-gpu", "--no-pdf-header-footer"])
        .arg(format!("--print-to-pdf={}", out.display()))
        .arg(format!("file://{}", html_path.display()))
        .output()?;
    let _ = std::fs::remove_file(&html_path);
    if !status.status.success() || !out.exists() {
        bail!("{browser} failed: {}", String::from_utf8_lossy(&status.stderr));
    }
    Ok(())
}

// ---- DOCX -------------------------------------------------------------------

fn xml(s: &str) -> String {
    escape(s).replace('"', "&quot;")
}

#[derive(Default, Clone, Copy)]
struct Run {
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
}

/// Minimal WordprocessingML: paragraphs, headings, lists, quotes, code, inline styles.
pub fn docx_document_xml(md: &str) -> String {
    let md = strip_comments(md);
    let mut body = String::new();
    let mut para: Option<String> = None;
    let mut style = Run::default();
    let mut list_depth = 0usize;
    let mut ordered: Vec<Option<u64>> = Vec::new();
    let mut quote = 0usize;

    let open = |pstyle: &str, prefix: &str| {
        let mut p = String::from("<w:p>");
        if !pstyle.is_empty() {
            p.push_str(&format!("<w:pPr><w:pStyle w:val=\"{pstyle}\"/></w:pPr>"));
        }
        if !prefix.is_empty() {
            p.push_str(&format!("<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", xml(prefix)));
        }
        p
    };
    let run = |text: &str, s: Run| {
        let mut r = String::from("<w:r><w:rPr>");
        if s.bold {
            r.push_str("<w:b/>");
        }
        if s.italic {
            r.push_str("<w:i/>");
        }
        if s.strike {
            r.push_str("<w:strike/>");
        }
        if s.code {
            r.push_str("<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\"/>");
        }
        r.push_str(&format!("</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>", xml(text)));
        r
    };

    for ev in Parser::new_ext(&md, markdown::options()) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                para = Some(open(&format!("Heading{n}"), ""));
            }
            Event::Start(Tag::Paragraph) => {
                if para.is_none() {
                    para = Some(open(if quote > 0 { "Quote" } else { "" }, ""));
                }
            }
            Event::Start(Tag::Item) => {
                let prefix = match ordered.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}{}. ", "    ".repeat(list_depth - 1), *n - 1)
                    }
                    _ => format!("{}• ", "    ".repeat(list_depth.saturating_sub(1))),
                };
                para = Some(open("ListParagraph", &prefix));
            }
            Event::TaskListMarker(done) => {
                if let Some(p) = para.as_mut() {
                    p.push_str(&run(if done { "☑ " } else { "☐ " }, Run::default()));
                }
            }
            Event::Start(Tag::List(start)) => {
                if let Some(p) = para.take() {
                    body.push_str(&p);
                    body.push_str("</w:p>");
                }
                list_depth += 1;
                ordered.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                list_depth -= 1;
                ordered.pop();
            }
            Event::Start(Tag::BlockQuote(_)) => quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote -= 1,
            Event::Start(Tag::CodeBlock(_)) => {
                para = Some(open("Code", ""));
                style.code = true;
            }
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::Text(t) => {
                let p = para.get_or_insert_with(|| open("", ""));
                if style.code {
                    for (i, line) in t.trim_end_matches('\n').split('\n').enumerate() {
                        if i > 0 {
                            p.push_str("<w:r><w:br/></w:r>");
                        }
                        p.push_str(&run(line, style));
                    }
                } else {
                    p.push_str(&run(&t.replace("==", ""), style));
                }
            }
            Event::Code(t) => {
                let p = para.get_or_insert_with(|| open("", ""));
                p.push_str(&run(&t, Run { code: true, ..style }));
            }
            Event::SoftBreak => {
                if let Some(p) = para.as_mut() {
                    p.push_str(&run(" ", style));
                }
            }
            Event::HardBreak => {
                if let Some(p) = para.as_mut() {
                    p.push_str("<w:r><w:br/></w:r>");
                }
            }
            Event::Rule => body.push_str(
                "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"auto\"/></w:pBdr></w:pPr></w:p>",
            ),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock) => {
                style.code = false;
                if let Some(p) = para.take() {
                    body.push_str(&p);
                    body.push_str("</w:p>");
                }
            }
            _ => {}
        }
    }
    if let Some(p) = para.take() {
        body.push_str(&p);
        body.push_str("</w:p>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
    )
}

const DOCX_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="IBM Plex Sans" w:hAnsi="IBM Plex Sans"/><w:sz w:val="22"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="300" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:spacing w:before="360"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:pPr><w:spacing w:before="320"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:rPr><w:b/><w:sz w:val="24"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:pPr><w:ind w:left="567"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:pPr><w:spacing w:after="60"/><w:ind w:left="360"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:pPr><w:shd w:val="clear" w:fill="F0F0F0"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/><w:sz w:val="20"/></w:rPr></w:style>
</w:styles>"#;

pub fn docx(md: &str, out: &Path) -> Result<()> {
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    let parts: [(&str, String); 5] = [
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#.into()),
        ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.into()),
        ("word/_rels/document.xml.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.into()),
        ("word/styles.xml", DOCX_STYLES.into()),
        ("word/document.xml", docx_document_xml(md)),
    ];
    for (name, data) in parts {
        zip.start_file(name, opts)?;
        zip.write_all(data.as_bytes())?;
    }
    zip.finish()?;
    Ok(())
}

pub fn export(md: &str, title: &str, format: Format, out: &Path) -> Result<()> {
    match format {
        Format::Html => std::fs::write(out, full_html(md, title))?,
        Format::Text => std::fs::write(out, plain_text(md))?,
        Format::Pdf => pdf(md, title, out)?,
        Format::Docx => docx(md, out)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "# Title\n\n// secret note\nSome **bold** and ==marked== text.\n\n- one\n- [x] two\n";

    #[test]
    fn html_has_marks_and_no_comments() {
        let h = body_html(MD);
        assert!(h.contains("<h1>Title</h1>"));
        assert!(h.contains("<strong>bold</strong>"));
        assert!(h.contains("<mark>marked</mark>"));
        assert!(!h.contains("secret"));
        assert!(full_html(MD, "T").contains("<title>T</title>"));
    }

    #[test]
    fn colspans_and_alignment() {
        let h = body_html("| a | b | c |\n|:-|---|--:|\n| x || y |\n");
        assert!(h.contains("<th style=\"text-align: left\">a</th>"), "{h}");
        assert!(h.contains("<td style=\"text-align: left\" colspan=\"2\">x</td>"), "{h}");
        assert!(h.contains("<td style=\"text-align: right\">y</td>"), "{h}");
    }

    #[test]
    fn plain_text_strips_markup() {
        let t = plain_text(MD);
        assert!(t.starts_with("Title\n\nSome bold and marked text."));
        assert!(t.contains("• one"));
    }

    #[test]
    fn docx_is_a_valid_zip_with_document() {
        let xml = docx_document_xml(MD);
        assert!(xml.contains("<w:pStyle w:val=\"Heading1\"/>"));
        assert!(xml.contains("<w:b/>"));
        let out = std::env::temp_dir().join(format!("quill-{}.docx", std::process::id()));
        docx(MD, &out).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        assert!(z.by_name("word/document.xml").is_ok());
        std::fs::remove_file(out).unwrap();
    }

    #[test]
    #[ignore = "needs Chrome/Chromium"]
    fn pdf_through_chrome() {
        let out = std::env::temp_dir().join(format!("quill-{}.pdf", std::process::id()));
        pdf(MD, "Test", &out).unwrap();
        assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
        std::fs::remove_file(out).unwrap();
    }
}
