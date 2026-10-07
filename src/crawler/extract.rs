use std::collections::BTreeSet;

use crate::model::WorldInfoEntry;

use super::pages::PageData;
use super::wikitext;
const CHARACTER_TEMPLATES: &[&str] = &["Char temp", "Character", "Infobox character", "Character_Tabs"];
const LOCATION_TEMPLATES: &[&str] = &["Location", "Infobox location", "City", "Country", "Continent"];
const PATHWAY_TEMPLATES: &[&str] = &["Pathway_template", "Pathway", "Pathway Template"];
const ITEM_TEMPLATES: &[&str] = &[
    "Sealed_Artifact_Template",
    "Sealed Artifact Template",
    "Sealed_Artifact",
    "Item",
    "Mystical_Item",
];
const ALIAS_PARAM_NAMES: &[&str] = &[
    "aliases", "alias", "other_names", "other_name", "other_aliases",
    "also_known_as", "aka", "alt_name", "alt_names", "names",
    "title", "titles", "honorific_name", "honorific names",
    "epithet", "epithets",
];

const TYPE_PARAM_NAMES: &[&str] = &[
    "type", "kind", "category", "species", "class",
];
/// Heuristically classify a page by scanning its categories + the first
/// few KB of wikitext for known templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Character,
    Location,
    Pathway,
    Item,
    Organization,
    Event,
    Terminology,
    Unknown,
}

pub fn classify(page: &PageData) -> PageKind {
    let cats = page.categories.iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if cats.iter().any(|c| c.contains("pathway")) || page.wikitext.contains("Pathway_template") {
        return PageKind::Pathway;
    }
    if cats.iter().any(|c|
           c.contains("sealed_artifact")
        || c.contains("mystical_item")
        || c == "item"
        || c == "items"
        || c.starts_with("item ")
        || c.starts_with("items ")
        || c.ends_with(" item")
        || c.ends_with(" items")
    ) || contains_template(&page.wikitext, ITEM_TEMPLATES) {
        return PageKind::Item;
    }
    if cats.iter().any(|c| c.contains("characters") || c.contains("beyonder"))
        || contains_template(&page.wikitext, CHARACTER_TEMPLATES) {
        return PageKind::Character;
    }
    if cats.iter().any(|c|
           c.contains("location") || c.contains("cities")
        || c.contains("countries") || c.contains("continent")
        || c.contains("realms")
    ) || contains_template(&page.wikitext, LOCATION_TEMPLATES) {
        return PageKind::Location;
    }
    if cats.iter().any(|c| c.contains("organization") || c.contains("organisations") || c.contains("churches")) {
        return PageKind::Organization;
    }
    if cats.iter().any(|c| c.contains("event") || c.contains("battle")) {
        return PageKind::Event;
    }
    if cats.iter().any(|c| c.contains("terminology")) {
        return PageKind::Terminology;
    }
    PageKind::Unknown
}
fn contains_template(wikitext: &str, names: &[&str]) -> bool {
    // Never slice a UTF-8 string at an arbitrary byte offset.
    // The old `[..8192]` could panic on Russian text when byte 8192
    // landed in the middle of a multi-byte UTF-8 character.
    let end = if wikitext.len() <= 8192 {
        wikitext.len()
    } else {
        let mut end = 8192;
        while end > 0 && !wikitext.is_char_boundary(end) {
            end -= 1;
        }
        end
    };
    let head = &wikitext[..end];
    for t in wikitext::find_templates(head) {
        if let Some(parsed) = wikitext::parse_template(&t.2) {
            let n = parsed.name.to_ascii_lowercase();
            if names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                return true;
            }
        }
    }
    false
}
pub fn default_priority(kind: PageKind) -> u32 {
    match kind {
        PageKind::Character => 100,
        PageKind::Pathway => 80,
        PageKind::Location => 60,
        PageKind::Item => 60,
        PageKind::Organization => 60,
        PageKind::Event => 40,
        PageKind::Terminology => 40,
        PageKind::Unknown => 30,
    }
}

pub fn default_position() -> u8 { 1 } // 0=before, 1=after (SillyTavern default)
/// Build a single world-info entry from a crawled page.
pub fn build_entry_from_page(
    page: &PageData,
    uid: u64,
    include_subpages: bool,
) -> Option<WorldInfoEntry> {
    let kind = classify(page);
    let priority = default_priority(kind);
    let position = default_position();
    let keys = extract_keys(page, kind);
    let content = extract_content(page, kind);
    if keys.is_empty() && content.trim().is_empty() {
        return None;
    }
    let comment = if let Some(cat) = page.categories.first() {
        format!("{} — Category:{}", page.title, cat)
    } else {
        page.title.clone()
    };
    let entry = WorldInfoEntry::new(
        uid,
        page.title.clone(),
        keys,
        content,
        comment,
        priority,
        position,
    );
    if !include_subpages { return Some(entry); }
    let _ = include_subpages; // future hook for emitting sub-entries
    Some(entry)
}
pub fn extract_keys(page: &PageData, kind: PageKind) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();

    // Always include the page title (with underscores replaced).
    set.insert(page.title.replace('_', " "));
    // Walk templates and pull alias-like params.
    for t in wikitext::find_templates(&page.wikitext) {
        if let Some(parsed) = wikitext::parse_template(&t.2) {
            for alias_param in ALIAS_PARAM_NAMES {
                if let Some(v) = parsed.named.get(*alias_param) {
                    for piece in split_aliases(v) {
                        if is_english_friendly(&piece) {
                            set.insert(piece);
                        }
                    }
                }
            }
            for tparam in TYPE_PARAM_NAMES {
                if let Some(v) = parsed.named.get(*tparam) {
                    for piece in split_aliases(v) {
                        if is_english_friendly(&piece) {
                            set.insert(piece);
                        }
                    }
                }
            }
        }
    }
    // For pathway pages, include a couple of obvious synonyms.
    if kind == PageKind::Pathway {
        let stem = page.title.replace(" Pathway", "").replace("_", " ");
        if !stem.is_empty() { set.insert(stem.clone()); }
        set.insert(format!("{stem} Pathway"));
    }

    set.into_iter().collect()
}
fn split_aliases(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == ';' || c == '|' || c == '\n')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty() && p.len() < 80)
        .map(|p| p.to_string())
        .collect()
}
/// Reject pieces that look like template markup, interwiki, or
/// `{{Language|...}}` Chinese/non-English names. We keep only English
/// alphanumerics + a small set of punctuation.
fn is_english_friendly(s: &str) -> bool {
    if s.is_empty() || s.len() > 80 { return false; }
    if s.starts_with("{{") || s.starts_with("[[") { return false; }
    if s.contains("lang=") || s.contains("zh:") || s.contains("Category:") { return false; }
    if s.chars().any(|c| {
        // Allow letters, digits, space, '.', '-', '\'', ',', ':', '(', ')', '/'
        !matches!(c,
            'A'..='Z' | 'a'..='z'
            | '0'..='9'
            | ' ' | '.' | '-' | '\'' | ',' | ':' | '(' | ')' | '/'
        )
    }) {
        return false;
    }
    true
}
pub fn extract_content(page: &PageData, _kind: PageKind) -> String {
    let intro = wikitext::first_paragraph(&page.wikitext);
    let body = wikitext::clean_prose(&page.wikitext);
    if body.is_empty() { return intro; }
    if intro.is_empty() { return body; }
    format!("{intro}\n\n{body}")
}
/// Build a Vec<WorldInfoEntry> from a slice of pages, with stable
/// monotonic UIDs starting at `start_uid`.
pub fn build_entries(pages: &[PageData], start_uid: u64, include_subpages: bool) -> Vec<WorldInfoEntry> {
    let mut out = Vec::with_capacity(pages.len());
    let mut uid = start_uid;
    for p in pages {
        if let Some(e) = build_entry_from_page(p, uid, include_subpages) {
            out.push(e);
            uid += 1;
        }
    }
    out
}
