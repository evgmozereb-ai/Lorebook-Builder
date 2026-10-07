use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, TextEdit, Ui};

use crate::crawler::{ApiClient, Crawler, PageData, ProgressEvent};
use crate::model::Store;
use crate::ui::toast::ToastQueue;
use crate::ui::widgets;
#[derive(Default)]
pub struct CrawlState {
    pub wiki_url: String,
    pub custom_category: String,
    pub req_per_sec: u32,
    pub include_subpages: bool,
    pub use_cache: bool,
    pub categories: Vec<CategoryToggle>,
    pub running: bool,
    pub started_at: Option<Instant>,
    pub ok: u64,
    pub err: u64,
    pub cached: u64,
    pub entries_built: u64,
    pub total_in: u64,
    pub last_message: String,
    pub rx: Option<Receiver<ProgressEvent>>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub error: Option<String>,
    pub log: Vec<(String, String)>,
}
pub struct CategoryToggle {
    pub name: String,
    pub enabled: bool,
}
impl CrawlState {
    pub fn new() -> Self {
        Self {
            wiki_url: "lordofthemysteries.fandom.com".into(),
            req_per_sec: 2,
            include_subpages: false,
            use_cache: true,
            categories: vec![
                CategoryToggle { name: "Characters".into(), enabled: true },
                CategoryToggle { name: "Locations".into(), enabled: true },
                CategoryToggle { name: "Pathways".into(), enabled: true },
                CategoryToggle { name: "Items".into(), enabled: true },
                CategoryToggle { name: "Organizations".into(), enabled: true },
                CategoryToggle { name: "Events".into(), enabled: true },
                CategoryToggle { name: "Terminology".into(), enabled: true },
            ],
            running: false,
            log: Vec::new(),
            ..Default::default()
        }
    }
    pub fn selected_seeds(&self) -> Vec<String> {
        let mut seeds: Vec<String> = self.categories.iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect();
        let custom = self.custom_category.trim();
        if !custom.is_empty() && !seeds.iter().any(|s| s.eq_ignore_ascii_case(custom)) {
            seeds.push(custom.to_string());
        }
        seeds
    }

    pub fn cache_dir() -> PathBuf {
        if let Some(pd) = ProjectDirs::from("com", "LorebookBuilder", "LorebookBuilder") {
            pd.cache_dir().to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join("cache")
        }
    }
}
pub fn draw(ui: &mut Ui, state: &mut CrawlState, store: &Store, _toasts: &mut ToastQueue, dirty: &mut bool) {
    widgets::section_header(ui, "Crawl", Some("Pull a Fandom/MediaWiki wiki into your library."));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Wiki URL:").strong());
        let r = ui.add_sized(
            [ui.available_width() - 80.0, 28.0],
            TextEdit::singleline(&mut state.wiki_url)
                .hint_text("e.g. lordofthemysteries.fandom.com")
                .font(egui::FontId::proportional(14.0)),
        );
        if r.lost_focus() && !state.wiki_url.is_empty() {
            *dirty = true;
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Seed categories:").strong());
    ui.horizontal_wrapped(|ui| {
        for c in state.categories.iter_mut() {
            // Icon (allocated separately so it aligns with the checkbox text)
            crate::ui::category_icons::draw(ui, &c.name);
            ui.add_space(2.0);
            if ui.checkbox(&mut c.enabled, &c.name).changed() {
                *dirty = true;
            }
            ui.add_space(10.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Custom category:").strong());
        let r = ui.add_sized(
            [ui.available_width(), 28.0],
            TextEdit::singleline(&mut state.custom_category)
                .hint_text("e.g. Factions, Imperium, Space Marines")
                .font(egui::FontId::proportional(14.0)),
        );
        if r.changed() {
            *dirty = true;
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.include_subpages, "Include subpages for major characters");
        ui.checkbox(&mut state.use_cache, "Use disk cache (resume safely)");
        ui.add(egui::DragValue::new(&mut state.req_per_sec).range(1..=20).prefix("Rate: ").suffix(" req/s"));
    });
    ui.add_space(6.0);
    let button_label = if state.running { "🟡 Crawling…" } else { "▶  Start crawl" };
    let start_enabled = !state.running
        && !state.wiki_url.trim().is_empty()
        && !state.selected_seeds().is_empty();
    if ui.add_enabled(start_enabled, egui::Button::new(RichText::new(button_label).strong())).clicked() {
        start_crawl(state, store);
    }
    if state.running {
        ui.horizontal(|ui| {
            if ui.button("⏹  Cancel").on_hover_text("Stop the crawl after the current page").clicked() {
                if let Some(c) = &state.cancel {
                    c.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                state.last_message = "Cancelling…".into();
            }
        });
    }
    if state.total_in > 0 {
        let frac = (state.ok + state.err + state.cached) as f32 / state.total_in as f32;
        ui.add(
            egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                .show_percentage()
                .desired_width(ui.available_width()),
        );
        let elapsed = state.started_at.map(|i| i.elapsed().as_secs_f32()).unwrap_or(0.0);
        let rps = if elapsed > 0.0 { (state.ok + state.cached) as f32 / elapsed } else { 0.0 };
        let eta = if rps > 0.0 {
            let remaining = state.total_in.saturating_sub(state.ok + state.err + state.cached);
            let secs = remaining as f32 / rps;
            format!("ETA: {}", format_duration(secs))
        } else {
            "ETA: --".into()
        };
        ui.label(format!(
            "✅ {} fetched   💾 {} cached   ❌ {} errors   📚 {} entries   ⏱ {}   {}",
            state.ok, state.cached, state.err, state.entries_built, format_duration(elapsed), eta,
        ));
    }
    if let Some(err) = &state.error {
        ui.colored_label(Color32::from_rgb(255, 100, 100), format!("Error: {err}"));
    }
    ui.collapsing(RichText::new(format!("Log ({} entries)", state.log.len())).strong(), |ui| {
        ui.horizontal(|ui| {
            if ui.add_enabled(!state.log.is_empty() && !state.running, egui::Button::new("🧹  Clear log"))
                .on_hover_text("Wipe the log (disabled while a crawl is running)")
                .clicked() {
                state.log.clear();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (ts, msg) in state.log.iter().rev().take(500) {
                    ui.label(RichText::new(format!("[{ts}] {msg}")).font(egui::FontId::monospace(11.0)).weak());
                }
            });
    });
    // Drain channel — flip the running flag when worker signals completion.
    // The actual UI updates (counters, toasts) happen in the app-level drain.
    if let Some(rx) = &state.rx {
        let mut finished = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ProgressEvent::Done | ProgressEvent::Cancelled => { finished = true; }
                _ => { /* handled in app.rs */ }
            }
        }
        if finished {
            state.running = false;
            state.cancel = None;
        }
    }
}
fn start_crawl(state: &mut CrawlState, store: &Store) {
    state.running = true;
    state.ok = 0;
    state.err = 0;
    state.cached = 0;
    state.entries_built = 0;
    state.total_in = 0;
    state.started_at = Some(Instant::now());
    state.error = None;
    state.log.clear();
    state.last_message = String::new();

    let (tx, rx) = std::sync::mpsc::channel();
    state.rx = Some(rx);
    state.cancel = Some(Arc::new(AtomicBool::new(false)));
    let wiki_url = state.wiki_url.trim().to_string();
    let seeds = state.selected_seeds();
    let rps = state.req_per_sec;
    let use_cache = state.use_cache;
    let include_subpages = state.include_subpages;
    let start_uid = store.max_uid().unwrap_or(0) + 1;
    let cancel = state.cancel.clone().unwrap();

    let cache_dir = CrawlState::cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);
    let db_path = crate::app::data_dir().join("lorebook.sqlite");
    let _ = std::fs::create_dir_all(crate::app::data_dir());
    std::thread::spawn(move || {
        // Open a Store inside the worker thread so we can write entries
        // to the DB without holding any lock on the UI's connection.
        let worker_store = match Store::open(&db_path) {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(ProgressEvent::PageFailed {
                    title: "store init".into(),
                    error: format!("{e:#}"),
                });
                let _ = tx.send(ProgressEvent::Done);
                return;
            }
        };
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(ProgressEvent::PageFailed {
                    title: "init".into(),
                    error: format!("tokio: {e}"),
                });
                let _ = tx.send(ProgressEvent::Done);
                return;
            }
        };
        runtime.block_on(async move {
            let api_base = format!("https://{wiki_url}/api.php");
            let api = match ApiClient::new(api_base, rps) {
                Ok(a) => a,
                Err(e) => {
                    let _ = tx.send(ProgressEvent::PageFailed {
                        title: "api init".into(),
                        error: format!("{e:#}"),
                    });
                    let _ = tx.send(ProgressEvent::Done);
                    return;
                }
            };
            let mut crawler = match Crawler::new(&api, seeds, &cache_dir) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(ProgressEvent::PageFailed {
                        title: "crawler init".into(),
                        error: format!("{e:#}"),
                    });
                    let _ = tx.send(ProgressEvent::Done);
                    return;
                }
            };
            if !use_cache { crawler.skip_cache = true; }
            crawler.include_subpages = include_subpages;
            crawler.cancel = cancel;
            let tx2 = tx.clone();
            crawler.set_progress_callback(move |e| {
                let _ = tx2.send(e);
            });
            let result_pages: anyhow::Result<Vec<PageData>> = crawler.run().await;
            match result_pages {
                Ok(pages) => {
                    let mut uid = start_uid;
                    for page in &pages {
                        if let Some(entry) = crate::crawler::build_entry_from_page(page, uid, include_subpages) {
                            let cat = page.categories.first().cloned().unwrap_or_default();
                            let _ = worker_store.upsert_entry(
                                &entry,
                                page.pageid,
                                page.revid,
                                &page.url,
                                &cat,
                            );
                            let _ = tx.send(ProgressEvent::EntryBuilt {
                                uid,
                                name: entry.name.clone(),
                                category: cat,
                            });
                            uid += 1;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(ProgressEvent::PageFailed {
                        title: "crawl".into(),
                        error: format!("{e:#}"),
                    });
                }
            }
        });
    });
}
fn now_hms() -> String {
    let t = chrono::Local::now();
    t.format("%H:%M:%S").to_string()
}

fn format_duration(secs: f32) -> String {
    let s = secs as u64;
    if s < 60 { return format!("{s}s"); }
    let m = s / 60;
    let s = s % 60;
    if m < 60 { return format!("{m}m {s:02}s"); }
    let h = m / 60;
    let m = m % 60;
    format!("{h}h {m:02}m")
}
