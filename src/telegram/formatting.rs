//! Telegram display formatting. Canonical message content is stored separately.
use crate::contract::MessageFormat;
use crate::model::{Error, Result};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

const LIMIT: usize = 4096;
const TRUNCATED: &str = "\n\n…（已截断）";

pub(crate) fn message(
    agent: &str,
    content: &str,
    format: Option<MessageFormat>,
    no_header: bool,
) -> Result<(String, Option<String>)> {
    if content.is_empty() {
        return Err(Error::bad("message must be nonempty"));
    }
    if matches!(format, Some(MessageFormat::Markdown)) {
        let header = if no_header {
            String::new()
        } else {
            format!("{}:\n", escape_html(agent))
        };
        let budget = LIMIT.saturating_sub(header.encode_utf16().count());
        if budget < TRUNCATED.encode_utf16().count() {
            return Err(Error::bad(
                "Agent header leaves no room for Markdown content",
            ));
        }
        let rendered = render_bounded(content, budget);
        if rendered.trim().is_empty() {
            return Err(Error::bad("Markdown message must render nonempty text"));
        }
        return Ok((format!("{header}{rendered}"), Some("HTML".into())));
    }
    let text = if no_header {
        content.to_string()
    } else {
        crate::bot::conversation(agent, content)
    };
    if text.encode_utf16().count() > LIMIT {
        return Err(Error::bad(
            "message must fit Telegram's 4096 UTF-16 unit limit including the Agent header",
        ));
    }
    Ok((text, None))
}

// Render complete Markdown prefixes, so clipping never cuts an HTML tag or surrogate pair.
// Counting serialized HTML is conservative: decoded Telegram text can only be shorter.
fn render_bounded(raw: &str, budget: usize) -> String {
    let offsets: Vec<usize> = raw
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(raw.len()))
        .collect();
    let total = offsets.len() - 1;
    let mut keep = total;
    loop {
        let mut rendered = markdown_to_telegram_html(&raw[..offsets[keep]]);
        if keep < total {
            rendered.push_str(TRUNCATED);
        }
        let units = rendered.encode_utf16().count();
        if units <= budget {
            return rendered;
        }
        let shrink = (units - budget).div_ceil(2).max(keep / 8).max(1);
        keep = keep.saturating_sub(shrink);
    }
}

/// Convert CommonMark/GFM-style agent output into Telegram's restricted HTML
/// dialect. Raw HTML is always escaped; only tags emitted here can reach the
/// Bot API. This is safer than forwarding arbitrary text as MarkdownV2, whose
/// large reserved-character set makes one missed escape reject the message.
fn markdown_to_telegram_html(markdown: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut links = Vec::new();
    let mut code_blocks = Vec::new();
    let mut inline_depth = 0;

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { .. } => {
                    inline_depth += 1;
                    out.push_str("<b>");
                }
                Tag::BlockQuote(_) => {
                    ensure_newlines(&mut out, 2);
                    // Flatten quotations: Telegram forbids nested blockquotes.
                    out.push_str("› ");
                }
                Tag::CodeBlock(kind) => {
                    ensure_newlines(&mut out, 2);
                    match kind {
                        CodeBlockKind::Fenced(info) => {
                            let language = info.split_whitespace().next().filter(|value| {
                                !value.is_empty()
                                    && value.chars().all(|ch| {
                                        ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+')
                                    })
                            });
                            if let Some(language) = language {
                                out.push_str("<pre><code class=\"language-");
                                out.push_str(&escape_html_attribute(language));
                                out.push_str("\">");
                                code_blocks.push(true);
                            } else {
                                out.push_str("<pre>");
                                code_blocks.push(false);
                            }
                        }
                        CodeBlockKind::Indented => {
                            out.push_str("<pre>");
                            code_blocks.push(false);
                        }
                    }
                }
                Tag::List(start) => lists.push(start),
                Tag::Item => {
                    ensure_newlines(&mut out, 1);
                    out.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                    match lists.last_mut() {
                        Some(Some(next)) => {
                            out.push_str(&format!("{next}. "));
                            *next += 1;
                        }
                        _ => out.push_str("• "),
                    }
                }
                Tag::Emphasis => {
                    inline_depth += 1;
                    out.push_str("<i>");
                }
                Tag::Strong => {
                    inline_depth += 1;
                    out.push_str("<b>");
                }
                Tag::Strikethrough => {
                    inline_depth += 1;
                    out.push_str("<s>");
                }
                Tag::Link { dest_url, .. } => {
                    let allowed = is_allowed_telegram_link(&dest_url);
                    links.push(allowed);
                    if allowed {
                        inline_depth += 1;
                        out.push_str("<a href=\"");
                        out.push_str(&escape_html_attribute(&dest_url));
                        out.push_str("\">");
                    }
                }
                // Telegram does not support these Markdown container tags.
                // Their child text is still emitted and escaped below.
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph if lists.is_empty() => ensure_newlines(&mut out, 2),
                TagEnd::Paragraph => {}
                TagEnd::Heading(_) => {
                    inline_depth -= 1;
                    out.push_str("</b>");
                    ensure_newlines(&mut out, 2);
                }
                TagEnd::BlockQuote(_) => {
                    ensure_newlines(&mut out, 2);
                }
                TagEnd::CodeBlock => {
                    if code_blocks.pop().unwrap_or(false) {
                        out.push_str("</code></pre>");
                    } else {
                        out.push_str("</pre>");
                    }
                    ensure_newlines(&mut out, 2);
                }
                TagEnd::List(_) => {
                    lists.pop();
                    ensure_newlines(&mut out, if lists.is_empty() { 2 } else { 1 });
                }
                TagEnd::Item => ensure_newlines(&mut out, 1),
                TagEnd::Emphasis => {
                    inline_depth -= 1;
                    out.push_str("</i>");
                }
                TagEnd::Strong => {
                    inline_depth -= 1;
                    out.push_str("</b>");
                }
                TagEnd::Strikethrough => {
                    inline_depth -= 1;
                    out.push_str("</s>");
                }
                TagEnd::Link if links.pop().unwrap_or(false) => {
                    inline_depth -= 1;
                    out.push_str("</a>");
                }
                TagEnd::Link => {}
                _ => {}
            },
            Event::Text(text) => out.push_str(&escape_html(&text)),
            Event::Code(code) | Event::InlineMath(code) => {
                // Telegram disallows code inside styles or links; retain the outer style.
                if inline_depth == 0 {
                    out.push_str("<code>");
                }
                out.push_str(&escape_html(&code));
                if inline_depth == 0 {
                    out.push_str("</code>");
                }
            }
            Event::DisplayMath(math) => {
                ensure_newlines(&mut out, 2);
                out.push_str("<pre>");
                out.push_str(&escape_html(&math));
                out.push_str("</pre>");
                ensure_newlines(&mut out, 2);
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                out.push_str(&escape_html(&html));
            }
            Event::FootnoteReference(label) => {
                out.push('[');
                out.push_str(&escape_html(&label));
                out.push(']');
            }
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => {
                ensure_newlines(&mut out, 1);
                out.push_str("────────");
                ensure_newlines(&mut out, 1);
            }
            Event::TaskListMarker(checked) => {
                out.push_str(if checked { "☑ " } else { "☐ " });
            }
        }
    }

    out.trim_end_matches('\n').to_string()
}

fn is_allowed_telegram_link(value: &str) -> bool {
    reqwest::Url::parse(value)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https" | "tg" | "mailto"))
}

fn ensure_newlines(out: &mut String, count: usize) {
    if out.is_empty() {
        return;
    }
    let present = out.chars().rev().take_while(|ch| *ch == '\n').count();
    for _ in present..count {
        out.push('\n');
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn escape_html_attribute(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}
