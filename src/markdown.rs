use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Parse markdown text into ratatui Lines and a list of media URLs found in the text.
pub fn render(text: &str) -> (Vec<Line<'static>>, Vec<String>) {
    let mut r = Renderer::default();
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(text, opts) {
        r.event(event);
    }
    r.flush_pending();
    (r.lines, r.media_urls)
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    media_urls: Vec<String>,
    pending: Vec<Span<'static>>,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    heading: u8,       // 0 = not in heading
    in_image: bool,
    in_code_block: bool,
    in_blockquote: bool,
    list_depth: usize,
    ordered: Vec<bool>, // true = ordered
    item_counter: Vec<u64>,
}

impl Renderer {
    fn style(&self) -> Style {
        let mut s = Style::default();
        if self.heading > 0 {
            s = s.fg(if self.heading <= 2 { Color::Cyan } else { Color::Blue });
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.bold { s = s.add_modifier(Modifier::BOLD); }
        if self.italic { s = s.add_modifier(Modifier::ITALIC); }
        if self.strikethrough { s = s.add_modifier(Modifier::CROSSED_OUT); }
        s
    }

    fn push(&mut self, text: String) {
        if text.is_empty() { return; }
        let s = self.style();
        self.pending.push(Span::styled(text, s));
    }

    fn push_raw(&mut self, text: &'static str, style: Style) {
        self.pending.push(Span::styled(text, style));
    }

    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.pending);
        self.lines.push(Line::from(spans));
    }

    fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            self.flush();
        }
    }

    fn blank(&mut self) {
        self.flush_pending();
        self.lines.push(Line::raw(""));
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(t.into_string()),
            Event::Code(t) => {
                self.pending.push(Span::styled(
                    format!("`{}`", t.as_ref()),
                    Style::default().fg(Color::Yellow),
                ));
            }
            Event::SoftBreak => { self.push(" ".to_string()); }
            Event::HardBreak => { self.flush_pending(); }
            Event::Rule => {
                self.flush_pending();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(60),
                    Style::default().fg(Color::DarkGray),
                )));
                self.lines.push(Line::raw(""));
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "☑ " } else { "☐ " };
                self.push(marker.to_string());
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_pending();
                self.heading = level as u8;
                let prefix = match level {
                    HeadingLevel::H1 => "# ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "### ",
                    _ => "",
                };
                if !prefix.is_empty() {
                    let s = self.style();
                    self.pending.push(Span::styled(prefix.to_string(), s));
                }
            }
            Tag::Strong => self.bold = true,
            Tag::Emphasis => self.italic = true,
            Tag::Strikethrough => self.strikethrough = true,
            Tag::CodeBlock(_) => {
                self.flush_pending();
                self.in_code_block = true;
            }
            Tag::BlockQuote(_) => {
                self.in_blockquote = true;
            }
            Tag::List(first) => {
                self.list_depth += 1;
                let is_ordered = first.is_some();
                self.ordered.push(is_ordered);
                self.item_counter.push(first.unwrap_or(1));
            }
            Tag::Item => {
                self.flush_pending();
                let depth = self.list_depth.saturating_sub(1);
                let indent = "  ".repeat(depth);
                let is_ordered = self.ordered.last().copied().unwrap_or(false);
                let bullet = if is_ordered {
                    let n = self.item_counter.last_mut().map(|c| { let v = *c; *c += 1; v }).unwrap_or(1);
                    format!("{indent}{n}. ")
                } else {
                    format!("{indent}• ")
                };
                self.pending.push(Span::styled(bullet, Style::default().fg(Color::DarkGray)));
            }
            Tag::Image { dest_url, .. } => {
                self.in_image = true;
                let url = dest_url.to_string();
                let kind = if is_video(&url) { "video" } else { "image" };
                self.pending.push(Span::styled(
                    format!("[{kind}: {url}]  →  'o' to open"),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ));
                self.media_urls.push(url);
                self.flush();
            }
            Tag::Link { .. } => {
                // Style the link text; URL shown by browser on 'b'
                self.pending.push(Span::styled(
                    "",
                    Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
                ));
            }
            Tag::Table(_) => { self.flush_pending(); }
            Tag::TableHead => {}
            Tag::TableRow => {}
            Tag::TableCell => {
                self.pending.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
            }
            Tag::Paragraph => {}
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.heading = 0;
                self.flush();
                self.lines.push(Line::raw(""));
            }
            TagEnd::Paragraph => { self.blank(); }
            TagEnd::Strong => self.bold = false,
            TagEnd::Emphasis => self.italic = false,
            TagEnd::Strikethrough => self.strikethrough = false,
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.lines.push(Line::raw(""));
            }
            TagEnd::BlockQuote(_) => { self.in_blockquote = false; }
            TagEnd::List(_) => {
                self.list_depth -= 1;
                self.ordered.pop();
                self.item_counter.pop();
                if self.list_depth == 0 {
                    self.lines.push(Line::raw(""));
                }
            }
            TagEnd::Item => { self.flush_pending(); }
            TagEnd::Image => { self.in_image = false; }
            TagEnd::Link => {}
            TagEnd::TableHead => {
                self.flush_pending();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(60),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            TagEnd::TableRow => { self.flush_pending(); }
            TagEnd::TableCell => {}
            TagEnd::Table => { self.lines.push(Line::raw("")); }
            _ => {}
        }
    }

    fn text(&mut self, text: String) {
        if self.in_image { return; }

        if self.in_code_block {
            for line in text.lines() {
                self.pending.push(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Yellow),
                ));
                self.flush();
            }
            return;
        }

        if self.in_blockquote {
            for line in text.lines() {
                self.push_raw("│ ", Style::default().fg(Color::DarkGray));
                self.push(line.to_string());
                self.flush();
            }
            return;
        }

        // Normal text — split on newlines inside the text node
        let mut iter = text.splitn(2, '\n');
        if let Some(first) = iter.next() {
            self.push(first.to_string());
        }
        if let Some(rest) = iter.next() {
            self.flush_pending();
            self.text(rest.to_string());
        }
    }
}

fn is_video(url: &str) -> bool {
    let l = url.to_lowercase();
    l.ends_with(".mp4") || l.ends_with(".mov") || l.ends_with(".webm") || l.ends_with(".avi")
}
