use crate::{Error, MAX_OUTPUT_BYTES, links};
use markdown::{Constructs, ParseOptions, mdast::Node};
use std::collections::BTreeMap;

fn options() -> ParseOptions {
    ParseOptions {
        constructs: Constructs {
            html_flow: false,
            html_text: false,
            gfm_table: true,
            gfm_strikethrough: true,
            gfm_autolink_literal: true,
            ..Default::default()
        },
        gfm_strikethrough_single_tilde: false,
        ..Default::default()
    }
}

pub(crate) fn markdown(body: &str) -> Result<String, Error> {
    if body
        .bytes()
        .filter(|b| matches!(b, b'[' | b']' | b'*' | b'_' | b'>' | b'`' | b'~'))
        .take(crate::MAX_MARKDOWN_MARKERS + 1)
        .count()
        > crate::MAX_MARKDOWN_MARKERS
    {
        return Err(Error::Capacity);
    }
    // Normalize solely for parsing/source offsets; the DTO keeps original bytes.
    let mut source = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut ast = markdown::to_mdast(&source, &options()).map_err(|_| Error::Markdown)?;
    let mut rejected = Vec::new();
    let mut pending = vec![(&ast, 0)];
    while let Some((node, depth)) = pending.pop() {
        if depth > 100 {
            return Err(Error::Capacity);
        }
        if let Node::Definition(d) = node
            && !links::markdown_link(&d.url)
        {
            let position = d.position.as_ref().ok_or(Error::Markdown)?;
            let text = source
                .get(position.start.offset..position.end.offset)
                .ok_or(Error::Markdown)?;
            let mut escaped = false;
            let mut colon = None;
            for (i, c) in text.char_indices() {
                if !escaped && c == ']' && text.as_bytes().get(i + 1) == Some(&b':') {
                    colon = Some(position.start.offset + i + 1);
                    break;
                }
                escaped = !escaped && c == '\\';
            }
            rejected.push(colon.ok_or(Error::Markdown)?);
        }
        if let Some(children) = node.children() {
            pending.extend(children.iter().map(|n| (n, depth + 1)));
        }
    }
    if !rejected.is_empty() {
        // Markdown-it rejects unsafe definitions during parsing. Escape their
        // definition colon and parse once more so paragraph continuation and
        // later valid definitions retain that behavior, not invented block breaks.
        rejected.sort_unstable();
        for offset in rejected.into_iter().rev() {
            if source.as_bytes().get(offset) != Some(&b':') {
                return Err(Error::Markdown);
            }
            source.insert(offset, '\\');
        }
        ast = markdown::to_mdast(&source, &options()).map_err(|_| Error::Markdown)?;
    }
    let body = source.as_str();
    let mut renderer = MatrixRenderer {
        source: body,
        output: String::new(),
        definitions: BTreeMap::new(),
    };
    let mut stack = vec![(&ast, 0)];
    while let Some((node, depth)) = stack.pop() {
        if depth > 100 {
            return Err(Error::Capacity);
        }
        if let Node::Definition(d) = node
            && links::markdown_link(&d.url)
        {
            renderer
                .definitions
                .entry(d.identifier.clone())
                .or_insert((&d.url, d.title.as_deref()));
        }
        if let Some(children) = node.children() {
            stack.extend(children.iter().rev().map(|n| (n, depth + 1)));
        }
    }
    renderer.node(&ast, false, 0)?;
    Ok(renderer.output)
}

struct MatrixRenderer<'a> {
    source: &'a str,
    output: String,
    definitions: BTreeMap<String, (&'a str, Option<&'a str>)>,
}
impl<'a> MatrixRenderer<'a> {
    fn push(&mut self, text: &str) -> Result<(), Error> {
        if self.output.len() + text.len() > MAX_OUTPUT_BYTES {
            return Err(Error::Capacity);
        }
        self.output.push_str(text);
        Ok(())
    }
    fn text(&mut self, text: &str, attribute: bool) -> Result<(), Error> {
        for c in text.chars() {
            let mut buffer = [0; 4];
            self.push(match c {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' if attribute => "&quot;",
                '\0' => "\u{fffd}",
                _ => c.encode_utf8(&mut buffer),
            })?;
        }
        Ok(())
    }
    fn open(&mut self, tag: &str, attrs: &[(&str, String)]) -> Result<(), Error> {
        if !links::TAGS.contains(&tag) {
            return Err(Error::Markdown);
        }
        self.push("<")?;
        self.push(tag)?;
        for (name, value) in attrs {
            if value.is_empty()
                || !matches!(
                    (tag, *name),
                    ("a", "href" | "title") | ("code", "class") | ("ol", "start")
                )
                || (*name == "href" && !links::safe_href(value))
            {
                continue;
            }
            self.push(" ")?;
            self.push(name)?;
            self.push("=\"")?;
            self.text(value, true)?;
            self.push("\"")?;
        }
        self.push(if matches!(tag, "br" | "hr") {
            " />"
        } else {
            ">"
        })
    }
    fn close(&mut self, tag: &str) -> Result<(), Error> {
        if !links::TAGS.contains(&tag) {
            return Err(Error::Markdown);
        }
        self.push("</")?;
        self.push(tag)?;
        self.push(">")
    }
    fn nodes(&mut self, nodes: &[Node], protected: bool, depth: usize) -> Result<(), Error> {
        for n in nodes {
            self.node(n, protected, depth + 1)?;
        }
        Ok(())
    }
    fn wrap(
        &mut self,
        tag: &str,
        nodes: &[Node],
        protected: bool,
        depth: usize,
    ) -> Result<(), Error> {
        self.open(tag, &[])?;
        self.nodes(nodes, protected, depth)?;
        self.close(tag)
    }
    fn source(&self, node: &Node) -> Result<&'a str, Error> {
        let p = node.position().ok_or(Error::Markdown)?;
        self.source
            .get(p.start.offset..p.end.offset)
            .ok_or(Error::Markdown)
    }
    fn invalid_link(&mut self, node: &Node, depth: usize) -> Result<(), Error> {
        // JS rejects dangerous links during parsing, leaving their Markdown
        // syntax as text (with ordinary escapes/entities/emphasis still parsed).
        let original = self.source(node)?.to_owned();
        let mut opts = options();
        opts.constructs.label_start_link = false;
        opts.constructs.label_start_image = false;
        opts.constructs.definition = false;
        let parsed = markdown::to_mdast(&original, &opts).map_err(|_| Error::Markdown)?;
        if let Node::Root(root) = parsed {
            for node in root.children {
                if let Node::Paragraph(p) = node {
                    self.nodes(&p.children, true, depth + 1)?;
                } else {
                    return Err(Error::Markdown);
                }
            }
            Ok(())
        } else {
            Err(Error::Markdown)
        }
    }
    fn anchor(
        &mut self,
        url: &str,
        title: Option<&str>,
        children: &[Node],
        depth: usize,
    ) -> Result<(), Error> {
        let mut attrs = vec![("href", links::normalize(url))];
        if let Some(title) = title {
            attrs.push(("title", title.into()));
        }
        self.open("a", &attrs)?;
        self.nodes(children, true, depth)?;
        self.close("a")
    }
    fn node(&mut self, node: &Node, protected: bool, depth: usize) -> Result<(), Error> {
        if depth > 100 {
            return Err(Error::Capacity);
        }
        match node {
            Node::Root(root) => self.nodes(&root.children, false, depth),
            Node::Paragraph(p) => {
                self.wrap("p", &p.children, protected, depth)?;
                self.push("\n")
            }
            Node::Text(text) => {
                for (i, line) in text.value.split('\n').enumerate() {
                    if i > 0 {
                        self.open("br", &[])?;
                        self.push("\n")?;
                    }
                    if protected {
                        self.text(line, false)?;
                    } else {
                        self.extra_links(line)?;
                    }
                }
                Ok(())
            }
            Node::Emphasis(e) => self.wrap("em", &e.children, protected, depth),
            Node::Strong(e) => self.wrap("strong", &e.children, protected, depth),
            Node::Delete(e) => self.wrap("s", &e.children, protected, depth),
            Node::InlineCode(code) => {
                self.open("code", &[])?;
                self.text(&code.value, false)?;
                self.close("code")
            }
            Node::Code(code) => {
                self.open("pre", &[])?;
                let attrs = code
                    .lang
                    .as_ref()
                    .map(|l| vec![("class", format!("language-{l}"))])
                    .unwrap_or_default();
                self.open("code", &attrs)?;
                self.text(&code.value, false)?;
                if !code.value.is_empty() {
                    self.push("\n")?;
                }
                self.close("code")?;
                self.close("pre")?;
                self.push("\n")
            }
            Node::Break(_) => {
                self.open("br", &[])?;
                self.push("\n")
            }
            Node::Heading(h) => {
                self.wrap(&format!("h{}", h.depth), &h.children, protected, depth)?;
                self.push("\n")
            }
            Node::ThematicBreak(_) => {
                self.open("hr", &[])?;
                self.push("\n")
            }
            Node::Blockquote(q) => {
                self.open("blockquote", &[])?;
                self.push("\n")?;
                self.nodes(&q.children, protected, depth)?;
                self.close("blockquote")?;
                self.push("\n")
            }
            Node::List(list) => {
                let tag = if list.ordered { "ol" } else { "ul" };
                let attrs = list
                    .start
                    .filter(|s| *s != 1)
                    .map(|s| vec![("start", s.to_string())])
                    .unwrap_or_default();
                self.open(tag, &attrs)?;
                self.push("\n")?;
                let loose = list.spread
                    || list
                        .children
                        .iter()
                        .any(|n| matches!(n, Node::ListItem(i) if i.spread));
                for item in &list.children {
                    let Node::ListItem(item) = item else {
                        return Err(Error::Markdown);
                    };
                    self.open("li", &[])?;
                    for (i, child) in item.children.iter().enumerate() {
                        if !loose && let Node::Paragraph(p) = child {
                            self.nodes(&p.children, protected, depth + 1)?;
                            if i + 1 < item.children.len() {
                                self.push("\n")?;
                            }
                        } else {
                            if i == 0 {
                                self.push("\n")?;
                            }
                            self.node(child, protected, depth + 1)?;
                        }
                    }
                    self.close("li")?;
                    self.push("\n")?;
                }
                self.close(tag)?;
                self.push("\n")
            }
            Node::Link(link) => {
                if protected {
                    return self.nodes(&link.children, true, depth);
                }
                if !links::markdown_link(&link.url) {
                    return self.invalid_link(node, depth);
                }
                let original = self.source(node)?;
                if original.starts_with('[') {
                    return self.anchor(&link.url, link.title.as_deref(), &link.children, depth);
                }
                if original.starts_with('<') {
                    self.open("a", &[("href", links::normalize(&link.url))])?;
                    let display = original.trim_start_matches('<').trim_end_matches('>');
                    self.text(&links::display(display), false)?;
                    return self.close("a");
                }
                let original = original.to_owned();
                if original.to_ascii_lowercase().starts_with("www.") {
                    return self.text(&original, false);
                }
                let end = original
                    .find(links::unicode_punctuation)
                    .unwrap_or(original.len());
                let raw = &original[..end];
                let url = if link.url.starts_with("mailto:") && !raw.starts_with("mailto:") {
                    format!("mailto:{raw}")
                } else {
                    raw.into()
                };
                self.open("a", &[("href", links::normalize(&url))])?;
                self.text(&links::display(raw), false)?;
                self.close("a")?;
                self.text(&original[end..], false)
            }
            Node::LinkReference(link) => {
                if protected {
                    return self.nodes(&link.children, true, depth);
                }
                let Some((url, title)) = self.definitions.get(&link.identifier).copied() else {
                    return self.invalid_link(node, depth);
                };
                if !links::markdown_link(url) {
                    return self.invalid_link(node, depth);
                }
                self.anchor(url, title, &link.children, depth)
            }
            Node::Image(image) => {
                if links::markdown_link(&image.url) {
                    Ok(())
                } else {
                    self.invalid_link(node, depth)
                }
            }
            Node::ImageReference(image) => {
                if self.definitions.contains_key(&image.identifier) {
                    Ok(())
                } else {
                    self.invalid_link(node, depth)
                }
            }
            Node::Definition(definition) => {
                if links::markdown_link(&definition.url) {
                    Ok(())
                } else {
                    self.open("p", &[])?;
                    self.invalid_link(node, depth)?;
                    self.close("p")?;
                    self.push("\n")
                }
            }
            Node::Table(table) => {
                self.open("table", &[])?;
                self.push("\n")?;
                for (i, row) in table.children.iter().enumerate() {
                    if i == 0 {
                        self.open("thead", &[])?;
                        self.push("\n")?;
                    }
                    if i == 1 {
                        self.open("tbody", &[])?;
                        self.push("\n")?;
                    }
                    self.open("tr", &[])?;
                    self.push("\n")?;
                    let Node::TableRow(row) = row else {
                        return Err(Error::Markdown);
                    };
                    for column in 0..table.align.len() {
                        let tag = if i == 0 { "th" } else { "td" };
                        self.open(tag, &[])?;
                        if let Some(Node::TableCell(cell)) = row.children.get(column) {
                            self.nodes(&cell.children, protected, depth + 1)?;
                        }
                        self.close(tag)?;
                        self.push("\n")?;
                    }
                    self.close("tr")?;
                    self.push("\n")?;
                    if i == 0 {
                        self.close("thead")?;
                        self.push("\n")?;
                    }
                }
                if table.children.len() > 1 {
                    self.close("tbody")?;
                    self.push("\n")?;
                }
                self.close("table")?;
                self.push("\n")
            }
            _ => Err(Error::Markdown),
        }
    }
    fn extra_links(&mut self, text: &str) -> Result<(), Error> {
        // GFM handles ordinary HTTP/email/IRI text. The retained JS formatter
        // additionally recognizes ftp, whose href the Matrix allowlist strips.
        let mut position = 0;
        for link in linkify::LinkFinder::new().links(text) {
            if !link.as_str().to_ascii_lowercase().starts_with("ftp://") {
                continue;
            }
            self.text(&text[position..link.start()], false)?;
            let raw = link.as_str();
            let end = raw.find(links::unicode_punctuation).unwrap_or(raw.len());
            self.open("a", &[])?;
            self.text(&links::display(&raw[..end]), false)?;
            self.close("a")?;
            position = link.start() + end;
        }
        self.text(&text[position..], false)
    }
}
