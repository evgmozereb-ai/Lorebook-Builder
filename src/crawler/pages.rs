use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::api::ApiClient;
use super::api::url_encode;
use super::extract::build_entry_from_page;
use crate::model::WorldInfoEntry;

/// A page discovered during the crawl. Includes the wikitext content and
/// the explicit (non-hidden) categories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageData {
    pub pageid: u64,
    pub title: String,
    pub revid: u64,
    pub wikitext: String,
    pub categories: Vec<String>,
    pub url: String,
}

pub struct Crawler<'a> {
    pub api: &'a ApiClient,
    pub seeds: Vec<String>,
    pub include_subpages: bool,
    pub skip_cache: bool,
    pub cache: super::cache::Cache,
    pub visited_categories: HashSet<String>,
    pub visited_pages: HashSet<u64>,
    pub progress: Option<Box<dyn Fn(ProgressEvent) + Send + Sync>>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    CategoryEntered { title: String, total_in: usize },
    PageFetched { title: String, pageid: u64, cached: bool },
    PageFailed { title: String, error: String },
    EntryBuilt { uid: u64, name: String, category: String },
    Cancelled,
    Done,
}

impl<'a> Crawler<'a> {
    pub fn new(api: &'a ApiClient, seeds: Vec<String>, cache_dir: &Path) -> Result<Self> {
        let cache = super::cache::Cache::open(cache_dir)?;
        Ok(Self {
            api,
            seeds,
            include_subpages: false,
            skip_cache: false,
            cache,
            visited_categories: HashSet::new(),
            visited_pages: HashSet::new(),
            progress: None,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn set_progress_callback<F: Fn(ProgressEvent) + Send + Sync + 'static>(&mut self, f: F) {
        self.progress = Some(Box::new(f));
    }

    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Run the crawl. Returns all newly-fetched pages.
    pub async fn run(&mut self) -> Result<Vec<PageData>> {
        let mut out = Vec::new();
        for seed in self.seeds.clone() {
            if self.cancel.load(Ordering::Relaxed) { break; }
            self.walk_category(&seed, &mut out).await?;
        }
        if self.cancel.load(Ordering::Relaxed) {
            if let Some(p) = &self.progress { p(ProgressEvent::Cancelled); }
            return Ok(out);
        }
        if let Some(p) = &self.progress {
            p(ProgressEvent::Done);
        }
        Ok(out)
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn report(&self, e: ProgressEvent) {
        if let Some(p) = &self.progress { p(e); }
    }

    async fn walk_category(
        &mut self,
        title: &str,
        out: &mut Vec<PageData>,
    ) -> Result<()> {
        if self.is_cancelled() { return Ok(()); }
        if !self.visited_categories.insert(title.to_string()) { return Ok(()); }
        let (subcats, pages) = self.list_category_members(title).await?;
        self.report(ProgressEvent::CategoryEntered { title: title.to_string(), total_in: pages.len() });
        for s in subcats {
            if self.is_cancelled() { return Ok(()); }
            Box::pin(self.walk_category(&s, out)).await?;
        }
        for p_title in pages {
            if self.is_cancelled() { return Ok(()); }
            match self.fetch_page(&p_title).await {
                Ok(Some(page)) => {
                    if self.visited_pages.insert(page.pageid) {
                        out.push(page);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    self.report(ProgressEvent::PageFailed {
                        title: p_title.clone(),
                        error: format!("{e:#}"),
                    });
                }
            }
        }
        Ok(())
    }

    /// Returns (subcategories, pages_in_category).
    pub async fn list_category_members(&self, category: &str) -> Result<(Vec<String>, Vec<String>)> {
        let mut subcats = Vec::new();
        let mut pages = Vec::new();
        let mut cont: Option<Value> = None;
        loop {
            let mut params: Vec<(&str, String)> = vec![
                ("action", "query".into()),
                ("list", "categorymembers".into()),
                ("cmtitle", format!("Category:{category}")),
                ("cmlimit", "500".into()),
                ("formatversion", "2".into()),
            ];
            if let Some(c) = &cont {
                if let Some(gcm) = c.get("gcmcontinue").and_then(|v| v.as_str()) {
                    params.push(("gcmcontinue", gcm.to_string()));
                }
                if let Some(cont) = c.get("continue").and_then(|v| v.as_str()) {
                    params.push(("continue", cont.to_string()));
                }
            }
            let v = self.api.call::<Value>(&params).await?;
            if let Some(members) = v.pointer("/query/categorymembers").and_then(|x| x.as_array()) {
                for m in members {
                    let ns = m.get("ns").and_then(|x| x.as_i64()).unwrap_or(-1);
                    let title = m.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    if title.is_empty() { continue; }
                    match ns {
                        14 => subcats.push(title),
                        0 => pages.push(title),
                        _ => {} // skip file/talk/etc.
                    }
                }
            }
            if let Some(c) = v.get("continue") {
                cont = Some(c.clone());
            } else {
                break;
            }
        }
        Ok((subcats, pages))
    }

    /// Fetch a single page's wikitext + categories, using the disk cache.
    pub async fn fetch_page(&mut self, title: &str) -> Result<Option<PageData>> {
        if !self.skip_cache {
            if let Some(c) = self.cache.get(title)? {
                self.report(ProgressEvent::PageFetched {
                    title: title.to_string(),
                    pageid: c.pageid,
                    cached: true,
                });
                return Ok(Some(c));
            }
        }
        let mut cont: Option<Value> = None;
        let mut pageid: u64 = 0;
        let mut revid: u64 = 0;
        let mut wikitext = String::new();
        let mut categories: Vec<String> = Vec::new();
        loop {
            let mut params: Vec<(&str, String)> = vec![
                ("action", "query".into()),
                ("titles", title.to_string()),
                ("prop", "revisions|categories".into()),
                ("rvprop", "ids|timestamp|content".into()),
                ("rvslots", "main".into()),
                ("clshow", "!hidden".into()),
                ("cllimit", "500".into()),
                ("formatversion", "2".into()),
            ];
            if let Some(c) = &cont {
                if let Some(rv) = c.get("rvcontinue").and_then(|v| v.as_str()) {
                    params.push(("rvcontinue", rv.to_string()));
                }
                if let Some(cont) = c.get("continue").and_then(|v| v.as_str()) {
                    params.push(("continue", cont.to_string()));
                }
            }
            let v = self.api.call::<Value>(&params).await?;
            if let Some(p) = v.pointer("/query/pages").and_then(|x| x.as_array()) {
                for pg in p {
                    if let Some(id) = pg.get("pageid").and_then(|x| x.as_u64()) {
                        pageid = id;
                    }
                    if let Some(revs) = pg.get("revisions").and_then(|x| x.as_array()) {
                        for r in revs {
                            if let Some(rid) = r.get("revid").and_then(|x| x.as_u64()) {
                                revid = rid;
                            }
                            // MediaWiki with formatversion=2 returns slot content as
                            // revisions[].slots.main.content. Older responses may still
                            // expose the legacy `*` field, so keep that fallback.
                            if let Some(content) = r.pointer("/slots/main/content").and_then(|x| x.as_str()) {
                                wikitext.push_str(content);
                                wikitext.push('\n');
                            } else if let Some(content) = r.get("*").and_then(|x| x.as_str()) {
                                wikitext.push_str(content);
                                wikitext.push('\n');
                            }
                        }
                    }
                    if let Some(cats) = pg.get("categories").and_then(|x| x.as_array()) {
                        for c in cats {
                            if let Some(t) = c.get("title").and_then(|x| x.as_str()) {
                                let t = t.trim_start_matches("Category:").to_string();
                                categories.push(t);
                            }
                        }
                    }
                }
            }
            if let Some(c) = v.get("continue") {
                cont = Some(c.clone());
            } else {
                break;
            }
        }
        if pageid == 0 {
            return Ok(None);
        }
        let page = PageData {
            pageid,
            title: title.to_string(),
            revid,
            wikitext,
            categories,
            url: page_url(self.api.base(), title),
        };
        self.cache.put(title, &page).context("writing cache")?;
        self.report(ProgressEvent::PageFetched {
            title: title.to_string(),
            pageid,
            cached: false,
        });
        Ok(Some(page))
    }
}

pub fn page_url(api_base: &str, title: &str) -> String {
    let root = api_base.trim_end_matches("/api.php");
    let mut root = root.trim_end_matches('/').to_string();
    if !root.starts_with("http") { root = format!("https://{root}"); }
    let encoded = url_encode(title).replace("%2F", "/");
    format!("{root}/wiki/{encoded}")
}

/// Build entries from a slice of pages with stable UIDs starting at `start_uid`.
pub fn materialize(pages: &[PageData], start_uid: u64, include_subpages: bool) -> Vec<WorldInfoEntry> {
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
