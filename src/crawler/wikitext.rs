use once_cell::sync::Lazy;
use regex::Regex;

static RE_GALLERY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<gallery\b[^>]*>.*?</gallery\s*>").unwrap()
});
static RE_COMMENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)<!--.*?-->").unwrap()
});
static RE_REF_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<ref\b[^>]*>.*?</ref\s*>").unwrap()
});
static RE_REF_SELF: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<ref\b[^>]*/\s*>").unwrap()
});
static RE_TAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<[^>]+>").unwrap()
});
static RE_HEADING: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^=+\s*([^=\n]+?)\s*=+\s*$").unwrap()
});
static RE_LIST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*[*#:;]+\s*").unwrap()
});
static RE_BOLD_ITALIC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"'''(.*?)'''|''(.*?)''").unwrap()
});
static RE_EXTERNAL_LINK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[(https?://[^\s\]]+)(?:\s+([^\]]+))?\]").unwrap()
});
static RE_WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+\n").unwrap());
static RE_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+").unwrap());
static RE_BLANK: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());
static RE_HTML_ENTITY: Lazy<Regex> = Lazy::new(|| Regex::new(r"&(?:nbsp|amp|lt|gt|quot);|&#(?:x[0-9A-Fa-f]+|[0-9]+);").unwrap());
static RE_ENGLISH_PAREN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?iu)\(\s*(?:англ?\.?|английское название)\s*:[^()\n]*\)").unwrap()
});
static RE_DOUBLE_ANGLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<<.*?>>").unwrap());

const NOISE_SECTIONS: &[&str] = &[
    "References", "Reference", "Sources", "Source", "Citations", "Bibliography",
    "Translations", "Site Navigation", "Navigation", "Image Gallery", "Image gallery",
    "Gallery", "See Also", "See also", "External Links", "External links", "Links",
    "Trivia", "Notes", "Footnotes", "См. также", "Смотрите также", "Примечания",
    "Примечание", "Ссылки", "Внешние ссылки", "Галерея", "Галерея изображений",
    "Навигация", "Переводы", "Источники", "Источник", "Литература", "Библиография",
    "Сноски", "Сноска", "Цитаты",
];

#[derive(Debug, Default, Clone)]
pub struct Template {
    pub name: String,
    pub params: Vec<String>,
    pub named: std::collections::BTreeMap<String, String>,
}

pub fn find_templates(wikitext: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < wikitext.len() {
        if wikitext[i..].starts_with("{{") {
            if let Some(end) = find_template_end(wikitext, i) {
                out.push((i, end, wikitext[i..end].to_string()));
                i = end;
                continue;
            }
        }
        i += wikitext[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

pub fn parse_template(body: &str) -> Option<Template> {
    let mut body = body.trim();
    if let Some(rest) = body.strip_prefix("{{") {
        body = rest.strip_suffix("}}").unwrap_or(rest).trim();
    }
    let mut t = Template::default();
    let parts = split_top_level(body, '|');
    if parts.is_empty() {
        return None;
    }
    t.name = parts[0].trim().trim_start_matches(':').to_string();
    for raw in parts.iter().skip(1) {
        let s = raw.trim();
        if let Some(eq) = s.find('=') {
            let (k, v) = s.split_at(eq);
            t.named.insert(k.trim().to_string(), v[1..].trim().to_string());
        } else if !s.is_empty() {
            t.params.push(s.to_string());
        }
    }
    Some(t)
}

fn split_top_level(s: &str, ch: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth_template = 0i32;
    let mut depth_link = 0i32;
    let mut start = 0usize;
    let mut i = 0usize;

    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("{{") {
            depth_template += 1;
            i += 2;
            continue;
        }
        if rest.starts_with("}}") && depth_template > 0 {
            depth_template -= 1;
            i += 2;
            continue;
        }
        if rest.starts_with("[[") {
            depth_link += 1;
            i += 2;
            continue;
        }
        if rest.starts_with("]]" ) && depth_link > 0 {
            depth_link -= 1;
            i += 2;
            continue;
        }
        let c = rest.chars().next().unwrap();
        if c == ch && depth_template == 0 && depth_link == 0 {
            out.push(s[start..i].to_string());
            i += c.len_utf8();
            start = i;
        } else {
            i += c.len_utf8();
        }
    }
    out.push(s[start..].to_string());
    out
}

pub fn clean_prose(wikitext: &str) -> String {
    let mut s = wikitext.to_string();

    // Block-level material that is useful to a browser but not to an RP lore entry.
    s = RE_COMMENT.replace_all(&s, "").to_string();
    s = RE_GALLERY.replace_all(&s, "").to_string();
    s = RE_REF_BLOCK.replace_all(&s, "").to_string();
    s = RE_REF_SELF.replace_all(&s, "").to_string();
    s = strip_html_block_tags(&s);
    s = RE_DOUBLE_ANGLE.replace_all(&s, "").to_string();
    s = strip_tables(&s);
    s = strip_templates(&s);
    s = replace_wikilinks(&s);
    s = RE_EXTERNAL_LINK.replace_all(&s, |caps: &regex::Captures| {
        caps.get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    }).to_string();

    let sections = split_sections(&s);
    let mut kept: Vec<(String, String)> = Vec::new();
    for (heading, body) in sections {
        let h_norm = heading.trim();
        if is_noise_section(h_norm) {
            continue;
        }
        let cleaned = clean_body(&body);
        if !cleaned.trim().is_empty() {
            kept.push((h_norm.to_string(), cleaned));
        }
    }

    if kept.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let mut first = true;
    for (heading, body) in kept {
        if !first {
            out.push_str("\n\n");
        }
        if !heading.is_empty() {
            out.push_str(&heading);
            out.push_str("\n\n");
        }
        out.push_str(&body);
        first = false;
    }

    let out = decode_basic_html_entities(&out);
    strip_combining_marks(&collapse_whitespace(&out))
}

pub fn split_sections(wikitext: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current_heading = String::new();
    let mut current_body = String::new();

    for line in wikitext.lines() {
        if let Some(c) = RE_HEADING.captures(line) {
            if !current_body.is_empty() || !current_heading.is_empty() {
                out.push((current_heading.clone(), current_body.clone()));
            }
            current_heading = c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            current_body.clear();
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    if !current_body.is_empty() || !current_heading.is_empty() {
        out.push((current_heading, current_body));
    }
    out
}

fn clean_body(s: &str) -> String {
    let mut t = s.to_string();
    t = strip_html_block_tags(&t);
    t = strip_tables(&t);
    t = strip_templates(&t);
    t = replace_wikilinks(&t);
    t = RE_EXTERNAL_LINK.replace_all(&t, |caps: &regex::Captures| {
        caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default()
    }).to_string();
    t = RE_BOLD_ITALIC.replace_all(&t, |caps: &regex::Captures| {
        caps.get(1).or_else(|| caps.get(2)).map(|m| m.as_str()).unwrap_or("").to_string()
    }).to_string();
    t = RE_LIST.replace_all(&t, "").to_string();
    t = RE_ENGLISH_PAREN.replace_all(&t, "").to_string();
    strip_combining_marks(&collapse_whitespace(&t))
}

fn is_noise_section(heading: &str) -> bool {
    let normalized = heading.trim().trim_matches(|c: char| c == ':' || c == ';').to_lowercase();
    NOISE_SECTIONS.iter().any(|n| n.to_lowercase() == normalized)
        || normalized.contains("external links")
        || normalized.contains("внешние ссылки")
        || normalized.contains("references")
        || normalized.contains("источники")
        || normalized.contains("примечания")
}

fn strip_html_block_tags(s: &str) -> String {
    let mut t = s.to_string();
    for (pattern, replacement) in [
        (r"(?is)<ref\b[^>]*>.*?</ref\s*>", ""),
        (r"(?is)<gallery\b[^>]*>.*?</gallery\s*>", ""),
        (r"(?is)<table\b[^>]*>.*?</table\s*>", ""),
        (r"(?is)<timeline\b[^>]*>.*?</timeline\s*>", ""),
        (r"(?is)<syntaxhighlight\b[^>]*>.*?</syntaxhighlight\s*>", ""),
        (r"(?is)<pre\b[^>]*>.*?</pre\s*>", ""),
        (r"(?is)<math\b[^>]*>.*?</math\s*>", ""),
        (r"(?is)<nowiki\b[^>]*>.*?</nowiki\s*>", ""),
    ] {
        let re = Regex::new(pattern).unwrap();
        t = re.replace_all(&t, replacement).to_string();
    }

    let re_br = Regex::new(r"(?is)<br\s*/?>").unwrap();
    t = re_br.replace_all(&t, "\n").to_string();
    t = RE_REF_SELF.replace_all(&t, "").to_string();
    RE_TAG.replace_all(&t, "").to_string()
}

fn strip_tables(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_table = false;

    for line in s.lines() {
        let trimmed = line.trim_start();
        if !in_table && trimmed.starts_with("{|") {
            in_table = true;
            continue;
        }
        if in_table {
            if trimmed == "|}" || trimmed.starts_with("|}") {
                in_table = false;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn strip_templates(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        if s[i..].starts_with("{{") {
            if let Some(end) = find_template_end(s, i) {
                i = end;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn find_template_end(s: &str, start: usize) -> Option<usize> {
    let mut i = start + 2;
    let mut depth = 1usize;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("{{{") {
            depth += 1;
            i += 3;
            continue;
        }
        if rest.starts_with("}}}") && depth > 0 {
            depth -= 1;
            i += 3;
            if depth == 0 {
                return Some(i);
            }
            continue;
        }
        if rest.starts_with("{{") {
            depth += 1;
            i += 2;
            continue;
        }
        if rest.starts_with("}}") && depth > 0 {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(i);
            }
            continue;
        }
        i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

fn replace_wikilinks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;

    while i < s.len() {
        if !s[i..].starts_with("[[") {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }

        if let Some(end) = find_balanced_end(s, i, "[[", "]]" ) {
            let inner = &s[i + 2..end - 2];
            let parts = split_top_level(inner, '|');
            let target_raw = parts.first().map(|p| p.trim()).unwrap_or("");
            let target = target_raw.trim_start_matches(':');
            let target_lower = target.to_lowercase();

            if is_media_or_category_namespace(&target_lower) {
                i = end;
                continue;
            }

            let replacement = if parts.len() > 1 {
                replace_wikilinks(parts.last().unwrap())
            } else {
                target
                    .split('#')
                    .next()
                    .unwrap_or(target)
                    .replace('_', " ")
            };

            out.push_str(replacement.trim());
            i = end;
        } else {
            // Unclosed [[...:...] is common in malformed wiki markup. Remove a clearly media/category line.
            let line_end = s[i..].find('\n').map(|n| i + n).unwrap_or(s.len());
            let candidate = &s[i + 2..line_end];
            let target = candidate.split('|').next().unwrap_or(candidate).trim().to_lowercase();
            if is_media_or_category_namespace(&target) {
                i = line_end;
            } else {
                out.push_str("[[");
                i += 2;
            }
        }
    }

    out
}

fn is_media_or_category_namespace(target_lower: &str) -> bool {
    let t = target_lower.trim_start_matches(':');
    [
        "файл:", "file:", "изображение:", "image:", "медиа:", "media:",
        "категория:", "category:",
    ].iter().any(|prefix| t.starts_with(prefix))
}

fn find_balanced_end(s: &str, start: usize, open: &str, close: &str) -> Option<usize> {
    let mut i = start + open.len();
    let mut depth = 1usize;

    while i < s.len() {
        if s[i..].starts_with(open) {
            depth += 1;
            i += open.len();
            continue;
        }
        if s[i..].starts_with(close) {
            depth -= 1;
            i += close.len();
            if depth == 0 {
                return Some(i);
            }
            continue;
        }
        i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

fn decode_basic_html_entities(s: &str) -> String {
    RE_HTML_ENTITY.replace_all(s, |caps: &regex::Captures| {
        let raw = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        match raw.to_ascii_lowercase().as_str() {
            "&nbsp;" => " ".to_string(),
            "&amp;" => "&".to_string(),
            "&lt;" => "<".to_string(),
            "&gt;" => ">".to_string(),
            "&quot;" => "\"".to_string(),
            _ if raw.starts_with("&#x") || raw.starts_with("&#X") => {
                u32::from_str_radix(raw[3..raw.len() - 1].trim(), 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
            _ if raw.starts_with("&#") => {
                raw[2..raw.len() - 1]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
            _ => raw.to_string(),
        }
    }).to_string()
}

fn strip_combining_marks(s: &str) -> String {
    s.chars()
        .filter(|&ch| !(('\u{0300}'..='\u{036F}').contains(&ch)))
        .collect()
}

fn collapse_whitespace(s: &str) -> String {
    let t = RE_SPACES.replace_all(s, " ");
    let t = RE_WS.replace_all(&t, "\n");
    let t = RE_BLANK.replace_all(&t, "\n\n");
    t.trim().to_string()
}

pub fn first_paragraph(wikitext: &str) -> String {
    let sections = split_sections(wikitext);
    if sections.is_empty() {
        return String::new();
    }
    let (_, body) = &sections[0];
    clean_body(body)
        .split("\n\n")
        .find(|p| !p.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_template_parses() {
        let t = parse_template("Foo|bar|x=y").unwrap();
        assert_eq!(t.name, "Foo");
        assert_eq!(t.params, vec!["bar"]);
        assert_eq!(t.named.get("x").map(String::as_str), Some("y"));
    }

    #[test]
    fn removes_media_links_with_nested_markup() {
        let s = clean_prose(
            "[[Файл:Ork_Activity_8th.jpg|thumb|250px|+++<br /><<пс:/миграции/вторжения>><br />[[Миграции орков|миграции]]]]\nОрки — зелёные ксеносы."
        );
        assert!(!s.contains("Ork_Activity_8th.jpg"));
        assert!(!s.contains("thumb"));
        assert!(!s.contains("<<пс:"));
        assert!(s.contains("Орки — зелёные ксеносы."));
    }

    #[test]
    fn removes_english_name_parenthetical_and_accents() {
        let s = clean_prose("Некро́ны (англ. Necrons) — древняя раса.");
        assert_eq!(s, "Некроны — древняя раса.");
    }

    #[test]
    fn converts_regular_links_to_labels() {
        let s = clean_prose("[[Адептус Астартес]] и [[Империум Человечества|Империум]].");
        assert_eq!(s, "Адептус Астартес и Империум.");
    }

    #[test]
    fn removes_noise_sections() {
        let s = clean_prose("Текст.\n\n== Ссылки ==\n* [https://example.com Источник]");
        assert_eq!(s, "Текст.");
    }
}
