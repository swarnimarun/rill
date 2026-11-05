//! Reserve source-item identities before admitting independent Markdown blocks.
use super::{check_label, slug, Heading, Page, Site, Targets, MAX_LABEL, MAX_SECTIONS};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Block {
    pub events: Vec<Event<'static>>,
    pub headings: Vec<Heading>,
    pub aliases: BTreeMap<String, String>,
    pub local_aliases: BTreeMap<String, String>,
}

pub(super) struct Item {
    pub heading: Heading,
    pub body: Block,
}

pub(super) struct PreparedPage {
    pub intro: Block,
    pub items: Vec<Item>,
    pub headings: Vec<Heading>,
    pub anchors: Targets,
}

struct Ids {
    used: BTreeSet<String>,
    next: BTreeMap<String, usize>,
}

impl Ids {
    fn new(used: BTreeSet<String>) -> Self {
        Self {
            used,
            next: BTreeMap::new(),
        }
    }

    fn allocate(&mut self, base: &str) -> String {
        let suffix = self.next.entry(base.to_owned()).or_insert(0);
        loop {
            let candidate = if *suffix == 0 {
                base.to_owned()
            } else {
                format!("{base}-{suffix}")
            };
            *suffix += 1;
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

pub(super) fn pages(site: &Site) -> Result<Vec<PreparedPage>, String> {
    let mut remaining = MAX_SECTIONS;
    site.pages
        .iter()
        .map(|page| page_blocks(page, &mut remaining))
        .collect()
}

fn page_blocks(page: &Page, remaining: &mut usize) -> Result<PreparedPage, String> {
    let mut ids = Ids::new(BTreeSet::from(["content".into(), "docs-query".into()]));
    // Reserve every real declaration before reading module or item commentary.
    let item_headings: Vec<_> = page
        .items
        .iter()
        .map(|item| {
            let title = format!("{} {}", item.kind, item.name);
            admit(remaining)?;
            Ok(Heading {
                id: ids.allocate(&slug(&title)),
                title,
                level: HeadingLevel::H2,
                empty: false,
            })
        })
        .collect::<Result<_, String>>()?;
    let protected: BTreeSet<_> = item_headings
        .iter()
        .map(|heading| heading.id.clone())
        .collect();
    let mut source_ids = Ids::new(BTreeSet::new());
    let intro = block(
        &page.markdown,
        "",
        &protected,
        &mut source_ids,
        &mut ids,
        remaining,
    )?;
    let mut all = intro.headings.clone();
    let mut items = Vec::new();
    for (item, heading) in page.items.iter().zip(item_headings) {
        let body = block(
            &item.markdown,
            &format!("{}--", heading.id),
            &protected,
            &mut Ids::new(BTreeSet::new()),
            &mut ids,
            remaining,
        )?;
        all.push(heading.clone());
        all.extend(body.headings.iter().cloned());
        items.push(Item { heading, body });
    }
    let published = all
        .iter()
        .map(|heading| (heading.id.clone(), heading.id.clone()))
        .collect();
    let mut source: BTreeMap<_, _> = protected
        .iter()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    for (raw, actual) in &intro.aliases {
        source.entry(raw.clone()).or_insert_with(|| actual.clone());
    }
    for item in &items {
        for (raw, actual) in &item.body.aliases {
            source.insert(format!("{}--{raw}", item.heading.id), actual.clone());
        }
    }
    Ok(PreparedPage {
        intro,
        items,
        headings: all,
        anchors: Targets { source, published },
    })
}

fn block(
    source: &str,
    prefix: &str,
    protected: &BTreeSet<String>,
    source_ids: &mut Ids,
    ids: &mut Ids,
    remaining: &mut usize,
) -> Result<Block, String> {
    let parser = Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    );
    let mut events = Vec::new();
    let mut count = 0;
    for event in parser {
        let event = if prefix.is_empty() {
            event
        } else {
            match event {
                Event::Start(Tag::Heading {
                    level,
                    id,
                    classes,
                    attrs,
                }) => Event::Start(Tag::Heading {
                    level: comment_level(level),
                    id,
                    classes,
                    attrs,
                }),
                Event::End(TagEnd::Heading(level)) => {
                    Event::End(TagEnd::Heading(comment_level(level)))
                }
                event => event,
            }
        };
        if matches!(event, Event::Start(Tag::Heading { .. })) {
            count += 1;
            if count > *remaining {
                return Err("E-DOCS-LIMIT: more than 50000 headings".into());
            }
        }
        events.push(event.into_static());
    }
    let mut headings = Vec::new();
    let mut current = None;
    let mut aliases = BTreeMap::new();
    for event in &events {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((*level, String::new(), 0_usize))
            }
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if let Some((_, title, length)) = &mut current {
                    *length += text.chars().take(MAX_LABEL + 1).count();
                    if *length > MAX_LABEL {
                        return Err("E-DOCS-LIMIT: heading label exceeds 256 characters".into());
                    }
                    title.push_str(text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, title, length)) = &mut current {
                    *length += 1;
                    if *length > MAX_LABEL {
                        return Err("E-DOCS-LIMIT: heading label exceeds 256 characters".into());
                    }
                    title.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, raw, _)) = current.take() {
                    let empty = raw.trim().is_empty();
                    let title = if empty { "Section".to_owned() } else { raw };
                    check_label(&title)?;
                    let base = slug(&title);
                    admit(remaining)?;
                    let raw_id = source_ids.allocate(&base);
                    let id = ids.allocate(&(prefix.to_owned() + &base));
                    aliases.insert(raw_id, id.clone());
                    headings.push(Heading {
                        title,
                        id,
                        level,
                        empty,
                    });
                }
            }
            _ => (),
        }
    }
    let local_aliases = aliases
        .iter()
        .filter(|(raw, _)| !protected.contains(*raw))
        .map(|(raw, actual)| (raw.clone(), actual.clone()))
        .collect();
    Ok(Block {
        events,
        headings,
        aliases,
        local_aliases,
    })
}

fn comment_level(level: HeadingLevel) -> HeadingLevel {
    match level {
        HeadingLevel::H1 => HeadingLevel::H3,
        HeadingLevel::H2 => HeadingLevel::H4,
        HeadingLevel::H3 => HeadingLevel::H5,
        _ => HeadingLevel::H6,
    }
}

fn admit(remaining: &mut usize) -> Result<(), String> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or("E-DOCS-LIMIT: more than 50000 headings")?;
    Ok(())
}
