//! Built-in documents: welcome note, help, Markdown guide, release notes.

pub const WELCOME: &str = "# Welcome to Quill

Quill is a quiet place to write. Everything you type is plain text in Markdown, saved automatically into your Library folder.

## Start writing

Just type. The first line becomes the file name. There is no Save button to remember: Quill saves as you go.

## Focus

Press **Ctrl+D** to turn on Focus Mode. Everything except the sentence you are writing fades into the background. Choose *Sentence* or *Paragraph* in the Focus menu, and try *Typewriter* to keep your line in the middle of the screen.

## See your writing

- **Ctrl+R** shows a preview next to your text.
- **Ctrl+Shift+D** highlights adjectives, nouns, adverbs, verbs and conjunctions.
- **Ctrl+Alt+D** points out fillers, clichés and redundancies.
- **F7** opens the Writing Studio: feedback that helps you become a better writer.

## Find your way

- **Ctrl+Shift+O** searches every document in your Library.
- **Ctrl+Alt+Shift+S** shows or hides the Library.
- The ••• button in the toolbar holds every menu.

Happy writing.
";

pub const HELP: &str = "# Quill Help

## Files

| Action | Shortcut |
| --- | --- |
| New document in Library | Ctrl+N |
| New tab | Ctrl+T |
| Open | Ctrl+O |
| Quick Search | Ctrl+Shift+O |
| Save / Save As | Ctrl+S / Ctrl+Shift+S |
| Export | Ctrl+Shift+E |
| Print | Ctrl+P |
| Close | Ctrl+W |

Documents are saved automatically about a second after you stop typing. New documents are created in the Library folder you are looking at, named after their first line.

## Writing

| Action | Shortcut |
| --- | --- |
| Bold / Italic | Ctrl+B / Ctrl+I |
| Strikethrough / Highlight | Ctrl+Alt+U / Ctrl+Shift+U |
| Heading 1–6 | Ctrl+1 … Ctrl+6 |
| List / Ordered list / Task list | Ctrl+L / Ctrl+Shift+L / Ctrl+Alt+L |
| Mark task completed | Ctrl+Alt+X |
| Blockquote | Ctrl+Shift+. |
| Comment | Ctrl+/ |
| Code / Code block | Ctrl+J / Ctrl+Shift+J |
| Link / Image / Wikilink / Footnote | Ctrl+K / Ctrl+Alt+K / Ctrl+Shift+K / Ctrl+Alt+Shift+K |
| Move line up / down | Alt+Up / Alt+Down |

Lists continue when you press Enter. Pressing Enter on an empty item ends the list. Tab and Shift+Tab indent and outdent.

## Focus and style

| Action | Shortcut |
| --- | --- |
| Focus Mode | Ctrl+D |
| Syntax Highlight | Ctrl+Shift+D |
| Style Check | Ctrl+Alt+D |
| Show Authors | Ctrl+Shift+A |
| Preview | Ctrl+R |
| Text size | Ctrl+= / Ctrl+- / Ctrl+0 |
| Dark appearance | Ctrl+Alt+Shift+N |
| Full screen | F11 |

## Authors

Text you paste with *Edit › Paste As › AI* is marked as written by AI. Turn on *Show Authors* to see it in grey and the share of human and AI text in the status bar. *Mark As* changes the author of a selection.

## Writing Studio

Press **F7** (or *Studio › Show Writing Studio*). The panel has four tabs:

- **Analyze** checks grammar, spelling, punctuation, vocabulary, sentence structure, clarity, tone and readability, entirely on your computer. Mistakes get an orange wavy line, style advice a thin blue line. Click a finding to read the rule with examples. The fix stays hidden until you have tried it yourself. **Alt+F7** jumps to the next finding; right-click an underlined word for fixes.
- **Coach** asks Claude for hints (**Shift+F7**), an explanation, an argument critique, brainstorming, rubric feedback or a rewrite that explains every change. By default it uses Claude Code already signed in on this computer (Haiku, high effort), so no API key is needed; you can switch the connection to the Anthropic API instead. Your text is only sent when you press a button.
- **Templates** start IELTS Task 1 and 2, essays, blog posts, technical documents and stories with a skeleton, a timer and a live checklist. *Customize* turns any template into a Markdown file you can edit.
- **Progress** remembers each writing session (on this computer only). It shows your recurring mistakes, weakest areas and progress, and it builds practice from your own sentences.

From a terminal, `quill --analyze essay.md` (optionally `--template ielts-task-2`) prints the same report.

## Content blocks

A line containing only `/file.md`, `/image.png` or `/table.csv` embeds that file (relative to the document) in the preview and exports.

// Lines starting with two slashes are comments: they stay in your text and never appear in the preview or exports.
";

pub const MARKDOWN: &str = "# Markdown Guide

Markdown is plain text with a few symbols for structure.

## Headings

# Heading 1
## Heading 2
### Heading 3

## Emphasis

*italic*, **bold**, ***bold italic***, ~~strikethrough~~, ==highlight==, `code`

## Lists

- Item
- Another item
    - Nested item

1. First
2. Second

- [ ] A task
- [x] A finished task

## Quotes and alerts

> A quotation.

> [!NOTE]
> Something worth knowing.

## Links and images

[Link text](https://example.com)
![Image description](picture.png)
[[Another document]]

## Code

```
Fenced code block
```

## Tables

| Name | Value |
| --- | --- |
| A | 1 |

## Footnotes

Text with a note.[^1]

[^1]: The note.

## Extras

A line with three dashes is a rule:

---

`+++` on its own line is a page break, and `{{TOC}}` inserts a table of contents.
";

pub const WHATS_NEW: &str = "# What's New in Quill

## 0.2 — Writing Studio

- English writing analysis: grammar, spelling, punctuation, vocabulary (lexical diversity, academic words, linking words), sentence rhythm, clarity, tone and six readability formulas, with a lesson for every rule.
- Writing coach powered by Claude: explanations, hints, argument critique, brainstorming, rubric feedback, explained rewrites and targeted exercises.
- Templates for IELTS, essays, blogs, technical writing and fiction, customizable as Markdown files.
- Progress tracking: recurring mistakes, weak areas, trends, and practice built from your own sentences.

## 0.1

- Writing surface with hanging heading marks, list continuation and smart typing.
- Focus Mode (sentence or paragraph) and Typewriter Mode.
- Syntax Highlight and Style Check.
- Library with folders, filter and Quick Search; tabs.
- Live preview with tables, tasks, alerts, footnotes, content blocks and table of contents.
- Export to PDF, HTML, Microsoft Word and plain text; printing.
- Authors: keep track of human and AI text.
- Light and dark appearance; iA Writer Mono, Duo and Quattro fonts.
";
