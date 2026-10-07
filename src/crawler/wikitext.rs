use once_cell::sync::Lazy;
use regex::Regex;

static RE_TEMPLATE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)\{\{[^{}]*?\}\}").unwrap()
});
static RE_TEMPLATE_OUTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)\{\{([^{}]|\{\{[^{}]*?\}\})*\}\}").unwrap()
});
static RE_LINK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[\[([^\[\]\|]+)(?:\|([^\[\]]+))?\]\]").unwrap()
});
static RE_FILE_LINK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)\[\[(?:файл|file|изображение|image):[^\[\]]*\]\]").unwrap()
});
static RE_CATEGORY_LINK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)\[\[(?:категория|category):[^\[\]]*\]\]").unwrap()
});
static RE_GALLERY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<gallery\b[^>]*>.*?</gallery\s*>").unwrap()
});
static RE_HEADING: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^=+\s*([^=\n]+?)\s*=+\s*$").unwrap()
});
static RE_BULLET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*[\*#:;]+\s*").unwrap()
});
static RE_BOLD_ITAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"'''(.+?)'''|\'\'(.+?)\'\'").unwrap()
});
static RE_WS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[ \t]+").unwrap()
});
static RE_BLANK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\n{3,}").unwrap()
});
static RE_TABLE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)\{\|[^|]*?(?:\n.*?)*?\n\|\}").unwrap()
});
static RE_HTML_COMMENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)<!--.*?-->").unwrap()
});

const NOISE_SECTIONS: &[&str] = &[
    "References", "Translations", "Site Navigation", "Image Gallery",
    "Image gallery", "See Also", "See also", "Navigation",
    "External Links", "External links", "Trivia", "Notes",
    "См. также", "Смотрите также", "Примечания", "Примечание",
    "Ссылки", "Внешние ссылки", "Галерея", "Галерея изображений",
    "Навигация", "Переводы", "Источники", "Литература",
];

#[derive(Debug, Default, Clone)]
pub struct Template {
    pub name: String,
    pub params: Vec<String>,
    pub named: std::collections::BTreeMap<String, String>,
}

pub fn find_templates(wikitext: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    for m in RE_TEMPLATE_OUTER.find_iter(wikitext) {
        out.push((m.start(), m.end(), m.as_str().to_string()));
    }
    out
}

pub fn parse_template(body: &str) -> Option<Template> {
    let body = body.trim();
    let mut t = Template::default();
    let parts = split_top_level(body, '|');
    if parts.is_empty() { return None; }
    t.name = parts[0].trim().trim_start_matches(':').to_string();
    for raw in parts.iter().skip(1) {
        let s = raw.trim();
        if let Some(eq) = s.find('=') {
            let (k, v) = s.split_at(eq);
            let v = &v[1..];
            t.named.insert(k.trim().to_string(), v.trim().to_string());
        } else {
            t.params.push(s.to_string());
        }
    }
    Some(t)
}

fn split_top_level(s: &str, ch: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_str: Option<char> = None;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '\'' if in_str.is_none() => in_str = Some('\''),
            '\'' => { in_str = None; }
            '"' if in_str.is_none() => in_str = Some('"'),
            '"' => { in_str = None; }
            '[' if in_str.is_none() => depth_bracket += 1,
            ']' if in_str.is_none() && depth_bracket > 0 => depth_bracket -= 1,
            '{' if in_str.is_none() => depth_paren += 1,
            '}' if in_str.is_none() && depth_paren > 0 => depth_paren -= 1,
            _ => {}
        }
        if c == ch && depth_paren == 0 && depth_bracket == 0 && in_str.is_none() {
            out.push(s[start..i].to_string());
            start = i + ch.len_utf8();
        }
    }
    out.push(s[start..].to_string());
    out
}

pub fn clean_prose(wikitext: &str) -> String {
    let mut s = wikitext.to_string();
    s = RE_HTML_COMMENT.replace_all(&s, "").to_string();
    s = RE_GALLERY.replace_all(&s, "").to_string();
    s = RE_FILE_LINK.replace_all(&s, "").to_string();
    s = RE_CATEGORY_LINK.replace_all(&s, "").to_string();
    s = RE_TABLE.replace_all(&s, "").to_string();

    let sections = split_sections(&s);
    let mut kept: Vec<(String, String)> = Vec::new();
    for (heading, body) in sections {
        let h_norm = heading.trim();
        if NOISE_SECTIONS.iter().any(|n| n.eq_ignore_ascii_case(h_norm)) {
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
    let mut first_kept = true;
    for (h, body) in kept {
        if first_kept && h.is_empty() {
            out.push_str(&body);
            out.push_str("\n\n");
        } else {
            if !first_kept { out.push_str("\n\n"); }
            if !h.is_empty() {
                out.push_str(&h);
                out.push_str("\n\n");
            }
            out.push_str(&body);
        }
        first_kept = false;
    }
    collapse_ws(&out)
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
            current_heading = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
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
    t = RE_HTML_COMMENT.replace_all(&t, "").to_string();
    t = RE_GALLERY.replace_all(&t, "").to_string();
    t = RE_FILE_LINK.replace_all(&t, "").to_string();
    t = RE_CATEGORY_LINK.replace_all(&t, "").to_string();
    t = strip_all_templates(&t);
    t = RE_TABLE.replace_all(&t, "").to_string();
    t = RE_LINK.replace_all(&t, |caps: &regex::Captures| {
        caps.get(2).map(|m| m.as_str().to_string())
            .unwrap_or_else(|| {
                let raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                raw.split('#').next().unwrap_or(raw)
                    .split('/').next().unwrap_or(raw)
                    .replace('_', " ")
                    .to_string()
            })
    }).to_string();
    t = RE_BOLD_ITAL.replace_all(&t, "$1$2").to_string();
    t = RE_BULLET.replace_all(&t, "").to_string();
    collapse_ws(&t)
}

fn strip_all_templates(s: &str) -> String {
    let mut prev = s.to_string();
    loop {
        let next = RE_TEMPLATE_OUTER.replace_all(&prev, "").to_string();
        if next == prev { break; }
        prev = next;
    }
    prev
}

fn collapse_ws(s: &str) -> String {
    let mut t = RE_WS.replace_all(s, " ").to_string();
    t = RE_BLANK.replace_all(&t, "\n\n").to_string();
    t.trim().to_string()
}

pub fn first_paragraph(wikitext: &str) -> String {
    let sections = split_sections(wikitext);
    if sections.is_empty() { return String::new(); }
    let (_, body) = &sections[0];
    let cleaned = clean_body(body);
    cleaned
        .split("\n\n")
        .find(|p| !p.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_template_named() {
        let t = parse_template("Foo|bar|x=y").unwrap();
        assert_eq!(t.name, "Foo");
        assert_eq!(t.params, vec!["bar"]);
        assert_eq!(t.named.get("x").map(String::as_str), Some("y"));
    }

    #[test]
    fn removes_russian_file_markup() {
        let s = clean_prose("[[Файл:Example.jpg|thumb|center|250px|Подпись]]\nТекст.");
        assert!(!s.contains("thumb"));
        assert!(s.contains("Текст."));
    }
}
