//! Offline HTML, addressable headings and a plain-text section search index.
use super::{Page, Site};
use pulldown_cmark::{html, CowStr, Event, HeadingLevel, Tag, TagEnd};
use std::collections::{BTreeMap, BTreeSet};

mod links;
mod prepare;

const MAX_CORPUS: usize = 32 * 1024 * 1024;
const MAX_SECTIONS: usize = 50_000;
const MAX_LABEL: usize = 256;

#[derive(Clone)]
struct Heading {
    title: String,
    id: String,
    level: HeadingLevel,
    empty: bool,
}

#[derive(Clone)]
struct Targets {
    source: BTreeMap<String, String>,
    published: BTreeMap<String, String>,
}

pub(super) fn render(site: &Site) -> Result<BTreeMap<String, Vec<u8>>, String> {
    check_label(&site.title)?;
    if site.pages.len() > super::MAX_FILES - 4 {
        return Err("E-DOCS-LIMIT: too many rendered pages".into());
    }
    let mut paths = BTreeSet::new();
    let mut total = 0_usize;
    for page in &site.pages {
        check_label(&page.title)?;
        check_label(&page.section)?;
        if !page.path.ends_with(".html")
            || page.path == "search.html"
            || page.path.starts_with("assets/")
            || page.path.contains('\\')
            || page
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !paths.insert(&page.path)
        {
            return Err(format!(
                "E-DOCS-PATH: invalid or duplicate page {}",
                page.path
            ));
        }
        total += page.markdown.len();
        for item in &page.items {
            if item.kind.chars().take(MAX_LABEL + 1).count()
                + 1
                + item.name.chars().take(MAX_LABEL + 1).count()
                > MAX_LABEL
            {
                return Err("E-DOCS-LIMIT: API item label exceeds 256 characters".into());
            }
            total += item.kind.len() + item.name.len() + item.signature.len() + item.markdown.len();
        }
    }
    if total > MAX_CORPUS {
        return Err("E-DOCS-LIMIT: rendered corpus exceeds 32 MiB".into());
    }
    let prepared = prepare::pages(site)?;
    let anchors: Vec<_> = prepared.iter().map(|page| page.anchors.clone()).collect();
    let mut files = BTreeMap::new();
    let mut output_bytes = 0;
    let mut index = SearchIndex::new();
    for (number, page) in site.pages.iter().enumerate() {
        let prepared = &prepared[number];
        search_entries(page, &prepared.intro, None, &mut index)?;
        let mut body = markdown(page, &prepared.intro, &site.pages, &anchors);
        for (item, prepared) in page.items.iter().zip(&prepared.items) {
            search_entries(
                page,
                &prepared.body,
                Some((&prepared.heading, &item.signature)),
                &mut index,
            )?;
            body.push_str(&format!(
                "<h2 id=\"{}\">{}{}</h2>",
                escape(&prepared.heading.id),
                escape(&prepared.heading.title),
                permalink(&prepared.heading)
            ));
            body.push_str(&format!(
                "<pre><code class=\"language-rill\">{}</code></pre>",
                escape(&item.signature)
            ));
            body.push_str(&markdown(page, &prepared.body, &site.pages, &anchors));
        }
        let content = if prepared
            .intro
            .headings
            .iter()
            .any(|heading| heading.level == HeadingLevel::H1)
        {
            body
        } else {
            format!("<h1>{}</h1>{body}", escape(&page.title))
        };
        let toc = contents(&prepared.headings);
        add_file(
            &mut files,
            &mut output_bytes,
            page.path.clone(),
            shell(site, Some(page), &content, &toc, false).into_bytes(),
        )?;
    }
    let search = "<h1>Search documentation</h1><p class=\"intro\">Find language syntax, project guides and source declarations.</p><p class=\"search-hint\">Try <code>inout</code>, <code>std.cli</code> or a function name.</p><p id=\"search-status\" role=\"status\" aria-live=\"polite\">Enter a term in the search field.</p><ol id=\"search-results\" class=\"search-results\"></ol><button id=\"search-more\" type=\"button\" hidden>Show more results</button><noscript><p>Search requires JavaScript. Use the page navigation or your browser's Find command to read the documentation.</p></noscript>";
    add_file(
        &mut files,
        &mut output_bytes,
        "search.html".into(),
        shell(site, None, search, "", true).into_bytes(),
    )?;
    add_file(
        &mut files,
        &mut output_bytes,
        "assets/search-index.js".into(),
        index.finish().into_bytes(),
    )?;
    add_file(
        &mut files,
        &mut output_bytes,
        "assets/style.css".into(),
        include_bytes!("assets/style.css").to_vec(),
    )?;
    add_file(
        &mut files,
        &mut output_bytes,
        "assets/site.js".into(),
        include_bytes!("assets/site.js").to_vec(),
    )?;
    Ok(files)
}

fn check_label(value: &str) -> Result<(), String> {
    if value.chars().take(MAX_LABEL + 1).count() > MAX_LABEL {
        return Err("E-DOCS-LIMIT: documentation label exceeds 256 characters".into());
    }
    Ok(())
}

fn add_file(
    files: &mut BTreeMap<String, Vec<u8>>,
    total: &mut usize,
    name: String,
    contents: Vec<u8>,
) -> Result<(), String> {
    let next = total
        .checked_add(contents.len())
        .ok_or("E-DOCS-LIMIT: output size")?;
    if files.len() >= super::MAX_FILES || next > super::MAX_OUTPUT_BYTES {
        return Err("E-DOCS-LIMIT: rendered output exceeds 2048 files or 128 MiB".into());
    }
    *total = next;
    files.insert(name, contents);
    Ok(())
}

struct SearchIndex {
    json: String,
    entries: usize,
    limit: usize,
}

impl SearchIndex {
    fn new() -> Self {
        Self {
            json: "[".into(),
            entries: 0,
            limit: MAX_CORPUS,
        }
    }

    fn push(&mut self, value: serde_json::Value) -> Result<(), String> {
        if self.entries >= MAX_SECTIONS {
            return Err("E-DOCS-LIMIT: search index exceeds 50000 sections".into());
        }
        let encoded =
            serde_json::to_string(&value).map_err(|error| format!("E-DOCS-RENDER: {error}"))?;
        let mut bytes = self.json.len() + usize::from(self.entries != 0) + 1;
        for character in encoded.chars() {
            bytes = bytes
                .checked_add(match character {
                    '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => 6,
                    other => other.len_utf8(),
                })
                .ok_or("E-DOCS-LIMIT: search index size")?;
            if bytes > self.limit {
                return Err("E-DOCS-LIMIT: serialized search index exceeds 32 MiB".into());
            }
        }
        if self.entries != 0 {
            self.json.push(',');
        }
        for character in encoded.chars() {
            match character {
                '<' => self.json.push_str("\\u003c"),
                '>' => self.json.push_str("\\u003e"),
                '&' => self.json.push_str("\\u0026"),
                '\u{2028}' => self.json.push_str("\\u2028"),
                '\u{2029}' => self.json.push_str("\\u2029"),
                other => self.json.push(other),
            }
        }
        self.entries += 1;
        Ok(())
    }

    fn finish(mut self) -> String {
        self.json.push(']');
        format!("window.RILL_DOCS_INDEX={};\n", self.json)
    }
}

fn escape(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            _ => result.push(character),
        }
    }
    result
}

fn slug(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            result.push(character);
        } else if character.is_whitespace() {
            result.push('-');
        }
    }
    if result.is_empty() {
        "section".into()
    } else {
        result
    }
}

fn markdown(page: &Page, block: &prepare::Block, pages: &[Page], anchors: &[Targets]) -> String {
    let mut output = Vec::new();
    let mut heading_number = 0;
    let mut link_stack = Vec::new();
    for event in &block.events {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let heading = &block.headings[heading_number];
                output.push(Event::Html(
                    format!("<{level} id=\"{}\">", escape(&heading.id)).into(),
                ));
                if heading.empty {
                    output.push(Event::Text(heading.title.clone().into()));
                }
            }
            Event::End(TagEnd::Heading(level)) => {
                let heading = &block.headings[heading_number];
                output.push(Event::Html(
                    format!("{}</{level}>\n", permalink(heading)).into(),
                ));
                heading_number += 1;
            }
            Event::Html(text) | Event::InlineHtml(text) => output.push(Event::Text(text.clone())),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let destination = links::href(page, dest_url, pages, anchors, &block.local_aliases);
                link_stack.push(destination.is_some());
                if let Some(destination) = destination {
                    output.push(Event::Start(Tag::Link {
                        link_type: *link_type,
                        dest_url: CowStr::from(destination),
                        title: title.clone(),
                        id: id.clone(),
                    }));
                }
            }
            Event::End(TagEnd::Link) => {
                if link_stack.pop().unwrap_or(false) {
                    output.push(event.clone());
                }
            }
            // Images render as alt text. No local copies or remote requests are
            // implied by a source Markdown image reference.
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => (),
            _ => output.push(event.clone()),
        }
    }
    let mut result = String::new();
    html::push_html(&mut result, output.into_iter());
    result
}

fn permalink(heading: &Heading) -> String {
    format!(
        "<a class=\"permalink\" href=\"#{}\" aria-label=\"Link to {}\">#</a>",
        links::encode(&heading.id, false),
        escape(&heading.title)
    )
}

fn contents(headings: &[Heading]) -> String {
    let mut output = String::new();
    for heading in headings
        .iter()
        .filter(|heading| heading.level != HeadingLevel::H1)
    {
        output.push_str(&format!(
            "<li class=\"toc-{}\"><a href=\"#{}\">{}</a></li>",
            heading.level,
            links::encode(&heading.id, false),
            escape(&heading.title)
        ));
    }
    if output.is_empty() {
        output
    } else {
        format!("<aside class=\"on-this-page\" aria-label=\"On this page\"><p>On this page</p><ul>{output}</ul></aside>")
    }
}

fn navigation(site: &Site, page: Option<&Page>, prefix: &str) -> String {
    let mut groups = vec!["Overview", "Language", "Project", "API"];
    for candidate in &site.pages {
        if !groups.contains(&candidate.section.as_str()) {
            groups.push(&candidate.section);
        }
    }
    let mut result = String::new();
    for section in groups {
        let pages: Vec<_> = site
            .pages
            .iter()
            .filter(|candidate| candidate.section == section)
            .collect();
        if pages.is_empty() {
            continue;
        }
        result.push_str(&format!(
            "<section class=\"nav-group\"><h2>{}</h2><ul>",
            escape(section)
        ));
        for candidate in pages {
            let url = prefix.to_owned() + &links::encode(&candidate.path, true);
            result.push_str("<li>");
            result.push_str(&links::link(
                &url,
                &candidate.title,
                page.is_some_and(|page| page.path == candidate.path),
            ));
            result.push_str("</li>");
        }
        result.push_str("</ul></section>");
    }
    result
}

fn shell(site: &Site, page: Option<&Page>, body: &str, toc: &str, search: bool) -> String {
    let path = page.map_or("search.html", |page| page.path.as_str());
    let title = page.map_or("Search", |page| page.title.as_str());
    let section = page.map_or("Documentation", |page| page.section.as_str());
    let prefix = links::root_prefix(path);
    let nav = navigation(site, page, &prefix);
    let index_script = if search {
        format!("<script src=\"{prefix}assets/search-index.js\" defer></script>")
    } else {
        String::new()
    };
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="rill docs"><title>{title} · {site_title} · Rill docs</title>
<link rel="stylesheet" href="{prefix}assets/style.css">{index_script}<script src="{prefix}assets/site.js" defer></script></head>
<body data-root="{prefix}"><a class="skip-link" href="#content">Skip to content</a>
<aside class="sidebar"><a class="brand" href="{prefix}index.html"><span class="brand-mark" aria-hidden="true">{{ }}</span> Rill docs</a><p class="workspace">{site_title}</p><nav aria-label="Documentation">{nav}</nav><p class="sidebar-note">Experimental language<br>Generated local reference</p></aside>
<header class="topbar"><a class="mobile-brand" href="{prefix}index.html">Rill docs</a><form class="search-form" role="search" action="{prefix}search.html" method="get"><label class="sr-only" for="docs-query">Search documentation</label><input id="docs-query" name="q" type="search" placeholder="Search language, modules, symbols" autocomplete="off" maxlength="200"><kbd aria-hidden="true">/</kbd><button type="submit">Search</button></form></header>
<details class="mobile-navigation"><summary>Browse documentation</summary><nav aria-label="Mobile documentation">{nav}</nav></details>
<main class="sheet" id="content"><div class="document-grid"><article class="document"><p class="breadcrumb">{section} / {title}</p>{body}<footer class="page-footer">Generated with <code>rill docs</code>. Rill is experimental.</footer></article>{toc}</div></main>
</body></html>"##,
        title = escape(title),
        site_title = escape(&site.title),
        section = escape(section)
    )
}

fn search_entries(
    page: &Page,
    block: &prepare::Block,
    item: Option<(&Heading, &str)>,
    index: &mut SearchIndex,
) -> Result<(), String> {
    let mut heading_number = 0;
    let mut title = item.map_or(page.title.as_str(), |(heading, _)| heading.title.as_str());
    let mut fragment = item.map(|(heading, _)| heading.id.as_str());
    let mut text = item.map_or_else(String::new, |(_, signature)| format!("{signature} "));
    let flush = |index: &mut SearchIndex,
                 title: &str,
                 fragment: Option<&str>,
                 text: &mut String|
     -> Result<(), String> {
        let plain = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !plain.is_empty() {
            let mut url = links::encode(&page.path, true);
            if let Some(fragment) = fragment {
                url.push('#');
                url.push_str(&links::encode(fragment, false));
            }
            index.push(serde_json::json!({"title":title,"page":page.title,"section":page.section,"url":url,"text":plain}))?;
        }
        text.clear();
        Ok(())
    };
    for event in &block.events {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                flush(index, title, fragment, &mut text)?;
                let heading = &block.headings[heading_number];
                title = &heading.title;
                fragment = Some(heading.id.as_str());
                heading_number += 1;
                if heading.empty {
                    text.push_str(&heading.title);
                }
            }
            Event::Text(value)
            | Event::Code(value)
            | Event::Html(value)
            | Event::InlineHtml(value) => text.push_str(value),
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::TableCell
                | TagEnd::TableRow
                | TagEnd::HtmlBlock,
            ) => text.push(' '),
            _ => (),
        }
    }
    flush(index, title, fragment, &mut text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(path: &str, source: &str, section: &str, markdown: &str) -> Page {
        Page {
            path: path.into(),
            source: source.into(),
            section: section.into(),
            title: "Test docs".into(),
            markdown: markdown.into(),
            items: Vec::new(),
        }
    }
    #[test]
    fn headings_tables_code_and_search_have_stable_addresses() {
        let site=Site { title:"Example".into(), pages:vec![page("api/math.html","src/math.rill","API", "# Math\n\n## fn double\n\nMultiply an integer.\n\n```rill\nfn double(x) { x + x }\n```\n\n## fn double\n\nSecond section.\n\n| Name | Type |\n| --- | --- |\n| x | i32 |\n")] };
        let output = render(&site).unwrap();
        let html = String::from_utf8(output["api/math.html"].clone()).unwrap();
        assert!(html.contains("id=\"fn-double\""));
        assert!(html.contains("id=\"fn-double-1\""));
        assert!(html.contains("href=\"#fn-double\""));
        assert!(html.contains("<table>"));
        assert!(html.contains("class=\"language-rill\""));
        assert!(html.contains("../assets/style.css"));
        assert!(!html.contains("search-index.js"));
        let index = String::from_utf8(output["assets/search-index.js"].clone()).unwrap();
        assert!(index.contains("api/math.html#fn-double"));
        assert!(index.contains("Multiply an integer"));
        let search = String::from_utf8(output["search.html"].clone()).unwrap();
        assert!(search.contains("assets/search-index.js"));
        assert!(search.contains("aria-live=\"polite\""));
    }
    #[test]
    fn links_are_rewritten_and_untrusted_markup_is_inert() {
        let site=Site { title:"<script>bad()</script>".into(), pages:vec![
            page("project/docs/guide.html","docs/guide.md","Project", "# Guide\n\n[Home](../README.md#start) [API](../src/math.rill#fn-double) [bad](javascript:alert) [missing](missing.md) [web](https://example.test)\n\n<script>alert(1)</script>\n\n![external](https://example.test/image.png)"),
            page("project/README.html","README.md","Project", "# Start"),
            page("api/math.html","src/math.rill","API", "## fn double") ] };
        let output = render(&site).unwrap();
        let html = String::from_utf8(output["project/docs/guide.html"].clone()).unwrap();
        assert!(html.contains("../../project/README.html#start"));
        assert!(html.contains("../../api/math.html#fn-double"));
        assert!(html.contains("href=\"https://example.test\""));
        assert!(!html.contains("href=\"javascript:"));
        assert!(!html.contains("href=\"missing.md"));
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!String::from_utf8(output["assets/search-index.js"].clone())
            .unwrap()
            .contains("<script>"));
    }
    #[test]
    fn authored_heading_fragments_follow_ids_reserved_by_the_reader() {
        let output = render(&Site {
            title: "Docs".into(),
            pages: vec![
                page(
                    "project/README.html",
                    "README.md",
                    "Project",
                    "[Content](#content) [Query](docs/query.md#docs-query)\n\n# Content\n",
                ),
                page(
                    "project/docs/query.html",
                    "docs/query.md",
                    "Project",
                    "# docs query\n",
                ),
            ],
        })
        .unwrap();
        let home = String::from_utf8(output["project/README.html"].clone()).unwrap();
        assert!(home.contains("id=\"content-1\""));
        assert!(home.contains("href=\"../project/README.html#content-1\""));
        assert!(home.contains("href=\"../project/docs/query.html#docs-query-1\""));
        let query = String::from_utf8(output["project/docs/query.html"].clone()).unwrap();
        assert!(query.contains("id=\"docs-query-1\""));
    }
    #[test]
    fn search_preserves_words_split_by_inline_markdown() {
        let output = render(&Site {
            title: "Docs".into(),
            pages: vec![page(
                "index.html",
                "index.md",
                "Overview",
                "# Guide\n\n**read**only `std.cli`.\n",
            )],
        })
        .unwrap();
        let script = String::from_utf8(output["assets/search-index.js"].clone()).unwrap();
        let json = script
            .strip_prefix("window.RILL_DOCS_INDEX=")
            .unwrap()
            .trim_end()
            .strip_suffix(';')
            .unwrap();
        let index: serde_json::Value = serde_json::from_str(json).unwrap();
        assert!(index[0]["text"]
            .as_str()
            .unwrap()
            .contains("readonly std.cli."));
    }
    #[test]
    fn real_api_items_precede_isolated_comment_headings_and_fences() {
        use crate::docgen::ApiItem;
        let mut api = page(
            "api/module.html",
            "src/module.rill",
            "API",
            "# Module\n\n## fn second\n\nModule prose.",
        );
        api.items = vec![
            ApiItem {
                kind: "fn".into(),
                name: "first".into(),
                signature: "fn first() -> i32".into(),
                markdown: "## fn second\n\n[Second declaration](#fn-second)\n\n```rill\nnot closed"
                    .into(),
            },
            ApiItem {
                kind: "fn".into(),
                name: "second".into(),
                signature: "fn second(x: i32) -> i32".into(),
                markdown: String::new(),
            },
        ];
        let output = render(&Site {
            title: "Docs".into(),
            pages: vec![
                api,
                page(
                    "project/README.html",
                    "README.md",
                    "Project",
                    "[Second](src/module.rill#fn-second)",
                ),
            ],
        })
        .unwrap();
        let html = String::from_utf8(output["api/module.html"].clone()).unwrap();
        assert!(html.contains("<h2 id=\"fn-second\">fn second"));
        assert!(html.contains("<h4 id=\"fn-first--fn-second\">fn second"));
        assert!(html.contains("href=\"../api/module.html#fn-second\">Second declaration"));
        let prose = html.find("not closed").unwrap();
        let fence_end = prose + html[prose..].find("</code></pre>").unwrap();
        let second = html.find("<h2 id=\"fn-second\">").unwrap();
        assert!(fence_end < second);
        assert!(html[second..].contains("fn second(x: i32) -&gt; i32</code>"));
        let home = String::from_utf8(output["project/README.html"].clone()).unwrap();
        assert!(home.contains("../api/module.html#fn-second"));
        let script = String::from_utf8(output["assets/search-index.js"].clone()).unwrap();
        let json = script
            .strip_prefix("window.RILL_DOCS_INDEX=")
            .unwrap()
            .trim_end()
            .strip_suffix(';')
            .unwrap();
        let index: serde_json::Value = serde_json::from_str(json).unwrap();
        assert!(index.as_array().unwrap().iter().any(|entry| entry["url"]
            == "api/module.html#fn-second"
            && entry["text"]
                .as_str()
                .unwrap()
                .contains("fn second(x: i32) -> i32")));
    }

    #[test]
    fn source_fragments_and_published_fragment_ids_are_distinct() {
        let output = render(&Site { title:"Docs".into(), pages:vec![
            page("project/guide.html", "guide.md", "Project", "# Content\n\n# content-1\n\n# a\n\n# a-1\n\n# a\n\n[Source](#content-1) [Published](guide.html#content-1) [Last](#a-2)"),
            page("project/README.html", "README.md", "Project", "[Second](guide.md#content-1) [Last](guide.md#a-2)") ] }).unwrap();
        let guide = String::from_utf8(output["project/guide.html"].clone()).unwrap();
        assert!(guide.contains("href=\"../project/guide.html#content-1-1\">Source"));
        assert!(guide.contains("href=\"../project/guide.html#content-1\">Published"));
        assert!(guide.contains("href=\"../project/guide.html#a-2\">Last"));
        let home = String::from_utf8(output["project/README.html"].clone()).unwrap();
        assert!(home.contains("href=\"../project/guide.html#content-1-1\">Second"));
        assert!(home.contains("href=\"../project/guide.html#a-2\">Last"));
    }

    #[test]
    fn empty_headings_have_visible_content_and_search_titles() {
        let output = render(&Site {
            title: "Docs".into(),
            pages: vec![page("index.html", "index.md", "Overview", "##\n\nText.")],
        })
        .unwrap();
        let html = String::from_utf8(output["index.html"].clone()).unwrap();
        assert!(html.contains("<h2 id=\"section\">Section"));
        let script = String::from_utf8(output["assets/search-index.js"].clone()).unwrap();
        let json = script
            .strip_prefix("window.RILL_DOCS_INDEX=")
            .unwrap()
            .trim_end()
            .strip_suffix(';')
            .unwrap();
        let index: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(index[0]["title"], "Section");
        assert!(index[0]["text"].as_str().unwrap().contains("Section"));
    }

    #[test]
    fn repeated_heading_ids_are_unique_and_admission_is_bounded() {
        let source = "## Examples\n\n".repeat(2_000);
        let output = render(&Site {
            title: "Docs".into(),
            pages: vec![page("index.html", "index.md", "Overview", &source)],
        })
        .unwrap();
        let html = String::from_utf8(output["index.html"].clone()).unwrap();
        let ids: BTreeSet<_> = html
            .split("<h2 id=\"")
            .skip(1)
            .map(|part| part.split('"').next().unwrap())
            .collect();
        assert_eq!(ids.len(), 2_000);
        assert!(ids.contains("examples"));
        assert!(ids.contains("examples-1999"));
        let source = "## Examples\n".repeat(MAX_SECTIONS + 1);
        assert!(
            matches!(render(&Site { title:"Docs".into(), pages:vec![page("index.html", "index.md", "Overview", &source)] }), Err(error) if error.contains("50000 headings"))
        );
    }

    #[test]
    fn repeated_metadata_and_raw_headings_have_explicit_label_limits() {
        use crate::docgen::ApiItem;
        let repeated = "## Examples\n".repeat(1_000);
        let mut long_page = page("index.html", "index.md", "Overview", &repeated);
        long_page.title = "A".repeat(4_096);
        assert!(
            matches!(render(&Site { title:"Docs".into(), pages:vec![long_page] }), Err(error) if error.contains("E-DOCS-LIMIT") && error.contains("256 characters"))
        );
        let mut api = page("index.html", "index.md", "API", "# Module");
        api.items.push(ApiItem {
            kind: "fn".into(),
            name: "A".repeat(4_096),
            signature: "fn source()".into(),
            markdown: repeated,
        });
        assert!(
            matches!(render(&Site { title:"Docs".into(), pages:vec![api] }), Err(error) if error.contains("API item label") && error.contains("256 characters"))
        );
        let source = format!(
            "# {}\n\n{}",
            "A".repeat(4_096),
            "## Examples\n".repeat(1_000)
        );
        assert!(
            matches!(render(&Site { title:"Docs".into(), pages:vec![page("index.html", "index.md", "Overview", &source)] }), Err(error) if error.contains("heading label") && error.contains("256 characters"))
        );
        let boundary = "é".repeat(256);
        let mut valid = page(
            "index.html",
            "index.md",
            "Overview",
            &format!("# {boundary}\n"),
        );
        valid.title = boundary;
        assert!(render(&Site {
            title: "Docs".into(),
            pages: vec![valid]
        })
        .is_ok());
    }

    #[test]
    fn escaped_index_and_output_budgets_reject_before_accumulation() {
        let mut index = SearchIndex {
            json: "[".into(),
            entries: 0,
            limit: 64,
        };
        assert!(index
            .push(serde_json::json!({"text":"<".repeat(12)}))
            .unwrap_err()
            .contains("serialized search index"));
        assert_eq!(index.json, "[");
        assert_eq!(index.entries, 0);
        let mut files = BTreeMap::new();
        let mut bytes = super::super::MAX_OUTPUT_BYTES - 1;
        assert!(
            add_file(&mut files, &mut bytes, "index.html".into(), vec![0, 1])
                .unwrap_err()
                .contains("128 MiB")
        );
        assert!(files.is_empty());
        assert_eq!(bytes, super::super::MAX_OUTPUT_BYTES - 1);
    }

    #[test]
    fn duplicate_generated_paths_and_anchor_collisions_are_checked() {
        let one = page(
            "index.html",
            "index.md",
            "Overview",
            "# a\n\n# a-1\n\n# a\n\n# Content\n\n# docs query",
        );
        let output = render(&Site {
            title: "Docs".into(),
            pages: vec![one],
        })
        .unwrap();
        let html = String::from_utf8(output["index.html"].clone()).unwrap();
        for anchor in ["a", "a-1", "a-2", "content-1", "docs-query-1"] {
            assert!(html.contains(&format!("id=\"{anchor}\"")));
        }
        let pages = vec![
            page("index.html", "index.md", "Overview", "# One"),
            page("index.html", "two.md", "Overview", "# Two"),
        ];
        assert!(render(&Site {
            title: "Docs".into(),
            pages
        })
        .unwrap_err()
        .contains("E-DOCS-PATH"));
    }
}
