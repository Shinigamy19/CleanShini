#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod i18n;
use anyhow::Result;
use clap::{Parser, Subcommand};
use i18n::{Lang, strings};
use cleanshini_core::{
    audit::{append_report, read_reports},
    config::{ensure_dirs, last_plan_path, load_rules, save_rules},
    model::{Category, DeleteMode, ExecutionReport, Plan, PlanItem, RuleSet},
    scan, execute_plan,
};
use eframe::egui;
use serde_json;
use std::fs;
use std::io::{self, Write};
use std::sync::mpsc;

// ---------- CLI ----------

#[derive(Parser)]
#[command(name = "cleanshini", version, about = "Limpieza de archivos temporales y residuales con control total")]
struct Cli {
    /// Lanzar la interfaz gráfica
    #[arg(long)]
    gui: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Scan {
        #[arg(short, long, value_delimiter = ',')]
        categories: Vec<String>,
        #[arg(long)]
        min_age: Option<u32>,
        #[arg(long)]
        min_size: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    Plan { #[arg(long)] json: bool },
    Clean {
        #[arg(long)] plan: Option<String>,
        #[arg(long)] permanent: bool,
        #[arg(long, short = 'y')] yes: bool,
        #[arg(short, long, value_delimiter = ',')] categories: Vec<String>,
        #[arg(long)] dry_run: bool,
    },
    Rules { #[command(subcommand)] sub: RulesCmd },
    Audit { #[arg(long, default_value = "20")] limit: usize, #[arg(long)] json: bool },
    Config { #[command(subcommand)] sub: ConfigCmd },
    EmptyRecycleBin,
    SystemCleanup {
        #[arg(long)] dism: bool,
        #[arg(long)] cleanmgr: bool,
        #[arg(long)] hibernation: bool,
        #[arg(long)] shadows: bool,
        #[arg(long)] all: bool,
    },
}

#[derive(Subcommand)]
enum RulesCmd {
    Show,
    AddExclude { path: String },
    RemoveExclude { path: String },
    SetMinAge { days: u32 },
    SetMinSize { bytes: u64 },
    EnableCategory { cat: String },
    DisableCategory { cat: String },
    Reset,
}

#[derive(Subcommand)]
enum ConfigCmd { Show, Path }

fn parse_cats(input: &[String]) -> Vec<Category> {
    if input.is_empty() { return Category::all(); }
    input.iter().filter_map(|s| Category::from_str(s)).collect()
}

fn cmd_scan(categories: Vec<String>, min_age: Option<u32>, min_size: Option<u64>, json: bool) -> Result<()> {
    let mut rules = load_rules();
    if !categories.is_empty() { rules.categories = parse_cats(&categories); }
    if let Some(a) = min_age { rules.min_age_days = a; }
    if let Some(s) = min_size { rules.min_size_bytes = s; }
    println!("Escaneando...");
    let items = scan(&rules)?;
    let plan_items: Vec<PlanItem> = items.into_iter().map(|it| PlanItem {
        selected: false,
        action: if rules.permanent_delete { DeleteMode::Permanent } else { DeleteMode::RecycleBin },
        item: it,
    }).collect();
    let plan = Plan { items: plan_items, generated_at: chrono::Utc::now(), roots: rules.include_paths.clone() };
    let p = last_plan_path();
    if let Some(parent) = p.parent() { fs::create_dir_all(parent)?; }
    fs::write(&p, serde_json::to_string_pretty(&plan)?)?;
    if json { println!("{}", serde_json::to_string_pretty(&plan)?); return Ok(()); }
    print_plan_summary(&plan);
    println!("\nPlan guardado en: {}", p.display());
    Ok(())
}

fn print_plan_summary(plan: &Plan) {
    use std::collections::BTreeMap;
    let mut by_cat: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for pi in &plan.items {
        if !pi.selected { continue; }
        let e = by_cat.entry(pi.item.category.as_str().to_string()).or_insert((0,0));
        e.0 += 1; e.1 += pi.item.size;
    }
    println!("=== Plan de limpieza ===");
    println!("Total items: {}  |  Tamaño: {}", plan.items.len(), cleanshini_core::scanner::format_size(plan.total_size()));
    println!();
    for (cat, (n, sz)) in &by_cat {
        println!("  {:<10} {:>6} archivos  {}", cat, n, cleanshini_core::scanner::format_size(*sz));
    }
    if plan.items.len() <= 40 {
        println!();
        for pi in &plan.items {
            if !pi.selected { continue; }
            println!("  [{}] {}  {}  - {}", if pi.selected {"x"} else {" "}, pi.item.category.as_str(), cleanshini_core::scanner::format_size(pi.item.size), pi.item.path.display());
        }
    } else {
        println!("\n(Más de 40 items; usá --json o la GUI para ver el detalle)");
    }
}

fn cmd_plan(json: bool) -> Result<()> {
    let p = last_plan_path();
    if !p.exists() { println!("No hay plan generado. Ejecutá `cleanshini scan` primero."); return Ok(()); }
    let s = fs::read_to_string(&p)?;
    let plan: Plan = serde_json::from_str(&s)?;
    if json { println!("{}", serde_json::to_string_pretty(&plan)?); } else { print_plan_summary(&plan); }
    Ok(())
}

fn cmd_clean(plan_path: Option<String>, permanent: bool, yes: bool, categories: Vec<String>, dry_run: bool) -> Result<()> {
    let p = plan_path.map(std::path::PathBuf::from).unwrap_or_else(last_plan_path);
    if !p.exists() { println!("No hay plan. Ejecutá `cleanshini scan` primero."); return Ok(()); }
    let s = fs::read_to_string(&p)?;
    let mut plan: Plan = serde_json::from_str(&s)?;
    if !categories.is_empty() {
        let cats = parse_cats(&categories);
        for pi in &mut plan.items { if !cats.contains(&pi.item.category) { pi.selected = false; } }
    }
    if dry_run { print_plan_summary(&plan); println!("\n(dry-run: no se borró nada)"); return Ok(()); }
    if plan.selected_count() == 0 { println!("No hay items seleccionados."); return Ok(()); }
    print_plan_summary(&plan);
    println!("\nSe borrarán {} items ({}).", plan.selected_count(), cleanshini_core::scanner::format_size(plan.total_size()));
    if !yes {
        print!("¿Confirmar? [s/N]: "); io::stdout().flush()?;
        let mut input = String::new(); io::stdin().read_line(&mut input)?;
        let ans = input.trim().to_lowercase();
        if ans != "s" && ans != "si" && ans != "y" && ans != "yes" { println!("Cancelado."); return Ok(()); }
    }
    let permanent_mode = permanent || load_rules().permanent_delete;
    let report = execute_plan(&plan, permanent_mode, |idx, total, path| {
        if idx % 50 == 0 || idx + 1 == total { println!("[{}/{}] {}", idx + 1, total, path.display()); }
    })?;
    append_report(&report)?;
    println!("\n=== Resultado ===");
    println!("Borrados: {}  |  Fallidos: {}  |  Liberados: {}", report.succeeded, report.failed, cleanshini_core::scanner::format_size(report.total_freed));
    if report.failed > 0 {
        for r in &report.records { if !r.success { println!("  FALLO: {} - {}", r.path.display(), r.error.as_deref().unwrap_or("?")); } }
    }
    Ok(())
}

fn cmd_rules(sub: RulesCmd) -> Result<()> {
    let mut rules = load_rules();
    match sub {
        RulesCmd::Show => { println!("{}", serde_json::to_string_pretty(&rules)?); }
        RulesCmd::AddExclude { path } => {
            let p = std::path::PathBuf::from(path);
            if !rules.exclude_paths.contains(&p) { rules.exclude_paths.push(p); }
            save_rules(&rules)?; println!("Exclusión agregada.");
        }
        RulesCmd::RemoveExclude { path } => {
            let p = std::path::PathBuf::from(path);
            rules.exclude_paths.retain(|x| x != &p);
            save_rules(&rules)?; println!("Exclusión removida.");
        }
        RulesCmd::SetMinAge { days } => { rules.min_age_days = days; save_rules(&rules)?; println!("min_age_days = {}", days); }
        RulesCmd::SetMinSize { bytes } => { rules.min_size_bytes = bytes; save_rules(&rules)?; println!("min_size_bytes = {}", bytes); }
        RulesCmd::EnableCategory { cat } => {
            if let Some(c) = Category::from_str(&cat) {
                if !rules.categories.contains(&c) { rules.categories.push(c); }
                save_rules(&rules)?; println!("Categoría habilitada: {}", cat);
            } else { println!("Categoría desconocida: {}", cat); }
        }
        RulesCmd::DisableCategory { cat } => {
            if let Some(c) = Category::from_str(&cat) {
                rules.categories.retain(|x| x != &c);
                save_rules(&rules)?; println!("Categoría deshabilitada: {}", cat);
            } else { println!("Categoría desconocida: {}", cat); }
        }
        RulesCmd::Reset => { let r = RuleSet::default(); save_rules(&r)?; println!("Reglas reseteadas a defaults."); }
    }
    Ok(())
}

fn cmd_audit(limit: usize, json: bool) -> Result<()> {
    let reports = read_reports(limit)?;
    if json { println!("{}", serde_json::to_string_pretty(&reports)?); return Ok(()); }
    if reports.is_empty() { println!("Sin historial de auditoría."); return Ok(()); }
    for r in &reports {
        println!("--- {} ---", r.started_at);
        println!("  borrados: {}  fallidos: {}  liberados: {}", r.succeeded, r.failed, cleanshini_core::scanner::format_size(r.total_freed));
        for rec in &r.records {
            let status = if rec.success { "OK" } else { "FAIL" };
            println!("  [{}] {} ({})", status, rec.path.display(), cleanshini_core::scanner::format_size(rec.size));
        }
    }
    Ok(())
}

fn cmd_config(sub: ConfigCmd) -> Result<()> {
    match sub {
        ConfigCmd::Show => {
            println!("config_dir: {}", cleanshini_core::config::config_dir().display());
            println!("data_dir:   {}", cleanshini_core::config::data_dir().display());
            println!("rules:      {}", cleanshini_core::config::rules_path().display());
            println!("plan:       {}", cleanshini_core::config::last_plan_path().display());
            println!("audit:      {}", cleanshini_core::config::audit_path().display());
        }
        ConfigCmd::Path => { println!("{}", cleanshini_core::config::config_dir().display()); }
    }
    Ok(())
}

// ---------- GUI ----------

#[derive(PartialEq, Clone, Copy)]
enum Tab { Scan, Review, Rules, Audit }

struct AppState {
    tab: Tab,
    lang: Lang,
    rules: RuleSet,
    plan: Option<Plan>,
    dark_mode: bool,
    scan_msg: String,
    scanning: bool,
    scan_rx: Option<mpsc::Receiver<Result<Plan, String>>>,
    selected: Vec<bool>,
    filter_cat: String,
    search: String,
    ext_filter: String,
    sort_col: String,
    sort_asc: bool,
    reports: Vec<ExecutionReport>,
    cleaning: bool,
    clean_msg: String,
    clean_rx: Option<mpsc::Receiver<Result<String, String>>>,
    permanent: bool,
    dry_run: bool,
    confirm_clean: bool,
    system_msg: String,
    system_running: bool,
    system_rx: Option<mpsc::Receiver<String>>,
}

impl Default for AppState {
    fn default() -> Self {
        let rules = load_rules();
        let plan = load_plan();
        let selected = plan.as_ref().map(|p| p.items.iter().map(|i| i.selected).collect()).unwrap_or_default();
        Self {
            tab: Tab::Scan,
            dark_mode: true,
            lang: Lang::Es,
            rules,
            plan,
            scan_msg: String::new(),
            scanning: false,
            scan_rx: None,
            selected,
            filter_cat: "all".into(),
            search: String::new(),
            ext_filter: String::new(),
            sort_col: "size".into(),
            sort_asc: false,
            reports: read_reports(20).unwrap_or_default(),
            cleaning: false,
            clean_msg: String::new(),
            clean_rx: None,
            permanent: false,
            dry_run: false,
            confirm_clean: false,
            system_msg: String::new(),
            system_running: false,
            system_rx: None,
        }
    }
}

fn cat_display(lang: i18n::Lang, key: &str) -> &'static str {
    let s = strings(lang);
    match key {
        "temp" => s.temp,
        "cache" => s.cache,
        "residue" => s.residue,
        "log" => s.log,
        "oldfile" => s.oldfile,
        "large" => s.large,
        "recyclebin" => s.recyclebin,
        "winupdate" => s.winupdate,
        "windowsold" => s.windowsold,
        "crashdump" => s.crashdump,
        "appcache" => s.appcache,
        "devcache" => s.devcache,
        "aimodel" => s.aimodel,
        "virtualdisk" => s.virtualdisk,
        "custom" => s.custom,
        "all" => s.all,
        _ => s.custom,
    }
}

fn category_color(cat: &str) -> egui::Color32 {
    match cat {
        "temp" => egui::Color32::from_rgb(66, 135, 245),
        "cache" => egui::Color32::from_rgb(155, 89, 182),
        "residue" => egui::Color32::from_rgb(231, 76, 60),
        "log" => egui::Color32::from_rgb(241, 196, 15),
        "oldfile" => egui::Color32::from_rgb(230, 126, 34),
        "large" => egui::Color32::from_rgb(26, 188, 156),
        "recyclebin" => egui::Color32::from_rgb(52, 73, 94),
        "winupdate" => egui::Color32::from_rgb(41, 128, 185),
        "windowsold" => egui::Color32::from_rgb(127, 140, 141),
        "crashdump" => egui::Color32::from_rgb(192, 57, 43),
        "appcache" => egui::Color32::from_rgb(142, 68, 173),
        "devcache" => egui::Color32::from_rgb(39, 174, 96),
        "aimodel" => egui::Color32::from_rgb(236, 112, 99),
        "virtualdisk" => egui::Color32::from_rgb(44, 62, 80),
        _ => egui::Color32::from_rgb(128, 128, 128),
    }
}

fn draw_category_chart(ui: &mut egui::Ui, plan: &Plan, selected_only: bool, lang: i18n::Lang) {
    use std::collections::BTreeMap;
    let mut by_cat: BTreeMap<String, u64> = BTreeMap::new();
    for pi in &plan.items {
        if selected_only && !pi.selected { continue; }
        if !selected_only && !pi.selected { /* include all scanned? */ }
        *by_cat.entry(pi.item.category.as_str().to_string()).or_insert(0) += if selected_only { if pi.selected { pi.item.size } else { 0 } } else { pi.item.size };
    }
    // if showing all scanned, include all items
    if !selected_only {
        by_cat.clear();
        for pi in &plan.items {
            *by_cat.entry(pi.item.category.as_str().to_string()).or_insert(0) += pi.item.size;
        }
    }
    if by_cat.is_empty() { return; }
    let total: u64 = by_cat.values().sum();
    let max = *by_cat.values().max().unwrap_or(&1);

    ui.label(if selected_only { "Espacio seleccionado por categoría:" } else { "Espacio escaneado por categoría:" });
    // donut
    let donut_size = 120.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(donut_size, donut_size), egui::Sense::hover());
    let center = rect.center();
    let radius = donut_size / 2.0 - 8.0;
    let stroke_w = radius * 0.45;
    let mut angle = -std::f32::consts::FRAC_PI_2;
    for (cat, sz) in &by_cat {
        let frac = *sz as f32 / total as f32;
        let sweep = frac * std::f32::consts::TAU;
        let color = category_color(cat);
        let steps = ((sweep.abs() * 40.0).max(4.0)) as usize;
        let mut pts = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let a = angle + sweep * (i as f32 / steps as f32);
            pts.push(center + egui::vec2(a.cos() * radius, a.sin() * radius));
        }
        ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(stroke_w, color)));
        angle += sweep;
    }
    // legend + bars
    ui.add_space(8.0);
    for (cat, sz) in &by_cat {
        let frac = *sz as f32 / max as f32;
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
        let bar_w = r.width() * frac;
        let bar_rect = egui::Rect::from_min_size(r.min, egui::vec2(bar_w, r.height()));
        ui.painter().rect_filled(bar_rect, 2.0, category_color(cat));
        ui.painter().text(r.min + egui::vec2(4.0, 1.0), egui::Align2::LEFT_TOP,
            format!("{}  {}  ({:.1}%)", cat_display(lang, cat), cleanshini_core::scanner::format_size(*sz), *sz as f64 / total as f64 * 100.0),
            egui::FontId::proportional(11.0), egui::Color32::WHITE);
    }
    ui.label(format!("Total: {}", cleanshini_core::scanner::format_size(total)));
}

fn load_plan() -> Option<Plan> {
    let p = last_plan_path();
    if !p.exists() { return None; }
    let s = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&s).ok()
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.scan_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(plan) => {
                        self.selected = plan.items.iter().map(|i| i.selected).collect();
                        self.scan_msg = format!("Escaneo completo: {} items ({}).", plan.items.len(), cleanshini_core::scanner::format_size(plan.total_size()));
                        self.plan = Some(plan);
                        if let Some(p) = &self.plan {
                            let path = last_plan_path();
                            if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
                            let _ = std::fs::write(&path, serde_json::to_string_pretty(p).unwrap_or_default());
                        }
                    }
                    Err(e) => { self.scan_msg = format!("Error: {}", e); }
                }
                self.scanning = false;
                self.scan_rx = None;
            }
        }
        if let Some(rx) = &self.clean_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(m) => {
                        self.clean_msg = m;
                        self.reports = read_reports(20).unwrap_or_default();
                        // Reload plan (deleted items removed)
                        self.plan = load_plan();
                        self.selected = self.plan.as_ref().map(|p| p.items.iter().map(|i| i.selected).collect()).unwrap_or_default();
                    }
                    Err(e) => { self.clean_msg = format!("Error: {}", e); }
                }
                self.cleaning = false;
                self.clean_rx = None;
            }
        }
        if let Some(rx) = &self.system_rx {
            if let Ok(m) = rx.try_recv() {
                self.system_msg = m;
                self.system_running = false;
                self.system_rx = None;
            }
        }

        ctx.set_visuals(if self.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() });

        let s = strings(self.lang);
        egui::TopBottomPanel::top("topbar").frame(egui::Frame { fill: egui::Color32::from_rgb(24, 34, 56), inner_margin: egui::Margin::same(8), ..Default::default() }).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading(egui::RichText::new(s.app_title).color(egui::Color32::from_rgb(120, 180, 255)));
                ui.separator();
                for (t, label) in [(Tab::Scan, s.tab_scan),(Tab::Review, s.tab_review),(Tab::Rules, s.tab_rules),(Tab::Audit, s.tab_audit)] {
                    let sel = self.tab == t;
                    let text = egui::RichText::new(label).color(if sel { egui::Color32::from_rgb(255, 255, 255) } else { egui::Color32::from_rgb(180, 190, 210) });
                    if ui.selectable_label(sel, text).clicked() { self.tab = t; }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(self.lang.code()).clicked() { self.lang = self.lang.next(); }
                    if ui.button(if self.dark_mode { s.theme_light } else { s.theme_dark }).clicked() {
                        self.dark_mode = !self.dark_mode;
                    }
                });

        });
        });
        egui::TopBottomPanel::bottom("statusbar").frame(egui::Frame::none().fill(egui::Color32::from_rgb(24, 34, 56)).inner_margin(egui::Margin::symmetric(8, 4))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(plan) = &self.plan {
                    ui.label(egui::RichText::new(format!("Plan: {} items  |  {}  |  Sel: {}", plan.items.len(), cleanshini_core::scanner::format_size(plan.total_size()), plan.items.iter().filter(|i| i.selected).count())).color(egui::Color32::from_rgb(180, 190, 210)).size(11.0));
                } else {
                    ui.label(egui::RichText::new("Sin plan. Escaneá para empezar.").color(egui::Color32::from_rgb(180, 190, 210)).size(11.0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.scanning { ui.spinner(); }
                    if self.cleaning { ui.label(egui::RichText::new("Limpiando…").color(egui::Color32::from_rgb(180, 190, 210)).size(11.0)); }
                });
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(egui::Margin::same(8))).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.tab {
                    Tab::Scan => self.ui_scan(ui),
                    Tab::Review => self.ui_review(ui, ctx),
                    Tab::Rules => self.ui_rules(ui),
                    Tab::Audit => self.ui_audit(ui),
                }
            });
        });
    }
}

impl AppState {
    fn ui_scan(&mut self, ui: &mut egui::Ui) {
        let s = strings(self.lang);
        ui.label(s.scan_desc);
        ui.add_space(4.0);
        if self.scanning {
            ui.horizontal(|ui| { ui.spinner(); ui.label(s.scanning); });
        } else {
            if ui.add(egui::Button::new(egui::RichText::new(format!("\u{25B6} {}", s.scan_now)).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(40, 120, 220))).clicked() {
                self.scanning = true;
                self.scan_msg.clear();
                let rules = self.rules.clone();
                let (tx, rx) = mpsc::channel();
                self.scan_rx = Some(rx);
                std::thread::spawn(move || {
                    match scan(&rules) {
                        Ok(items) => {
                            let plan_items: Vec<PlanItem> = items.into_iter().map(|it| PlanItem {
                                selected: false,
                                action: if rules.permanent_delete { DeleteMode::Permanent } else { DeleteMode::RecycleBin },
                                item: it,
                            }).collect();
                            let plan = Plan { items: plan_items, generated_at: chrono::Utc::now(), roots: rules.include_paths.clone() };
                            let _ = tx.send(Ok(plan));
                        }
                        Err(e) => { let _ = tx.send(Err(e.to_string())); }
                    }
                });
            }
        }
        if !self.scan_msg.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::from_rgb(80, 200, 120), &self.scan_msg);
        }
        if let Some(plan) = &self.plan {
            ui.add_space(8.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(" {}: {} items", s.plan_summary, plan.items.len()));
                    ui.separator();
                    ui.label(format!(" {}", cleanshini_core::scanner::format_size(plan.total_size())));
                    ui.separator();
                    ui.label(format!(" {}: {}", s.selected_count, plan.items.iter().filter(|i| i.selected).count()));
                });
            });
            ui.add_space(8.0);
            let p = plan.clone();
            draw_category_chart(ui, &p, false, self.lang);
        } else {
            ui.add_space(12.0);
            ui.label(s.no_plan);
        }

        ui.add_space(12.0);
        ui.label(s.system_cleanup);
        if self.system_running {
            ui.horizontal(|ui| { ui.spinner(); ui.label(s.running); });
        } else {
            ui.horizontal_wrapped(|ui| {
                if ui.button("\u{2715} Vaciar papelera").clicked() {
                    self.system_running = true; self.system_msg.clear();
                    let (tx, rx) = mpsc::channel();
                    self.system_rx = Some(rx);
                    std::thread::spawn(move || {
                        match cleanshini_core::empty_recycle_bin() { Ok(_) => { let _ = tx.send(s.recycle_emptied.into()); } Err(e) => { let _ = tx.send(format!("Error: {}", e)); } }
                    });
                }
                if ui.button("\u{2699} DISM (lento)").clicked() {
                    self.system_running = true; self.system_msg.clear();
                    let (tx, rx) = mpsc::channel();
                    self.system_rx = Some(rx);
                    std::thread::spawn(move || {
                        let res = cleanshini_core::run_system_cleanup(true, false, false, false);
                        let msg = res.iter().map(|r| format!("{}: {}", r.name, if r.success {"OK"} else {"FAIL"})).collect::<Vec<_>>().join(" | ");
                        let _ = tx.send(msg);
                    });
                }
                if ui.button("\u{23FB} Desactivar hibernacion").clicked() {
                    self.system_running = true; self.system_msg.clear();
                    let (tx, rx) = mpsc::channel();
                    self.system_rx = Some(rx);
                    std::thread::spawn(move || {
                        let res = cleanshini_core::run_system_cleanup(false, false, true, false);
                        let msg = res.iter().map(|r| format!("{}: {}", r.name, if r.success {"OK"} else {"FAIL"})).collect::<Vec<_>>().join(" | ");
                        let _ = tx.send(msg);
                    });
                }
                if ui.button("\u{21BA} Eliminar shadow copies").clicked() {
                    self.system_running = true; self.system_msg.clear();
                    let (tx, rx) = mpsc::channel();
                    self.system_rx = Some(rx);
                    std::thread::spawn(move || {
                        let res = cleanshini_core::run_system_cleanup(false, false, false, true);
                        let msg = res.iter().map(|r| format!("{}: {}", r.name, if r.success {"OK"} else {"FAIL"})).collect::<Vec<_>>().join(" | ");
                        let _ = tx.send(msg);
                    });
                }
            });
        }
        if !self.system_msg.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::from_rgb(80, 200, 120), &self.system_msg);
        }

        ui.add_space(6.0);
        egui::CollapsingHeader::new("ℹ ¿Qué hace cada opción?").show(ui, |ui| {
            ui.label(" Vaciar papelera: elimina el contenido de la Papelera de reciclaje (recupera espacio).");
            ui.label(" DISM (lento): limpia el almacén de componentes de Windows (WinSxS). Puede liberar varios GB. Requiere administrador y tarda minutos.");
            ui.label("Desactivar hibernacion: elimina hiberfil.sys (su tamanio = tu RAM) y desactiva la hibernacion.");
            ui.label(" Eliminar shadow copies: borra puntos de restauración del sistema, liberando espacio en disco.");
            ui.label(" Ejecutá como administrador para que DISM y shadow copies funcionen.");
        });
    }

    fn ui_review(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let s = strings(self.lang);
        let Some(plan) = &mut self.plan else {
            ui.label(s.no_plan);
            return;
        };
        ui.horizontal(|ui| {
            ui.label(s.filter_category);
            egui::ComboBox::from_label("").selected_text(&self.filter_cat)
                .show_ui(ui, |ui| {
                    for c in ["all","temp","cache","residue","log","oldfile","large","recyclebin","winupdate","windowsold","crashdump","appcache","devcache","aimodel","virtualdisk","custom"] {
                        ui.selectable_value(&mut self.filter_cat, c.to_string(), cat_display(self.lang, c));
                    }
                });
            ui.label(s.search);
            ui.text_edit_singleline(&mut self.search);
            ui.label("Ext:");
            ui.text_edit_singleline(&mut self.ext_filter);
        });
        ui.horizontal(|ui| {
            if ui.button(s.select_all).clicked() {
                for s in &mut self.selected { *s = true; }
                for pi in &mut plan.items { pi.selected = true; }
            }
            if ui.button(s.deselect_all).clicked() {
                for s in &mut self.selected { *s = false; }
                for pi in &mut plan.items { pi.selected = false; }
            }
            ui.checkbox(&mut self.permanent, s.permanent);
            ui.checkbox(&mut self.dry_run, s.dry_run);
        });

        let sel_count = self.selected.iter().filter(|b| **b).count();
        let sel_size: u64 = plan.items.iter().zip(self.selected.iter()).filter(|(_,s)| **s).map(|(pi,_)| pi.item.size).sum();

        // Action bar (always visible)
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.confirm_clean, s.confirm_clean);
            if self.cleaning {
                ui.spinner(); ui.label("Limpiando...");
            } else {
                if ui.add(egui::Button::new(egui::RichText::new(format!("\u{2715} {}", s.clean_selected)).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(200, 60, 60))).clicked() {
                    if sel_count == 0 { self.clean_msg = s.no_selection.into(); }
                    else if !self.confirm_clean { self.clean_msg = s.confirm_needed.into(); }
                    else {
                        self.cleaning = true;
                        self.clean_msg.clear();
                        let mut p = plan.clone();
                        for (idx, pi) in p.items.iter_mut().enumerate() { pi.selected = self.selected[idx]; }
                        let permanent = self.permanent;
                        let dry = self.dry_run;
                        let (tx, rx) = mpsc::channel();
                        self.clean_rx = Some(rx);
                        std::thread::spawn(move || {
                            if dry {
                                let _ = tx.send(Ok(format!("Dry-run: {} items ({}).", p.selected_count(), cleanshini_core::scanner::format_size(p.total_size()))));
                                return;
                            }
                            match execute_plan(&p, permanent, |_i,_t,_path| {}) {
                                Ok(rep) => {
                                    let _ = append_report(&rep);
                                    // Remove successfully deleted items from the plan
                                    let succeeded_paths: std::collections::HashSet<_> = rep.records.iter().filter(|r| r.success).map(|r| r.path.clone()).collect();
                                    let mut new_plan = p.clone();
                                    new_plan.items.retain(|pi| !succeeded_paths.contains(&pi.item.path));
                                    let plan_path = cleanshini_core::last_plan_path();
                                    if let Some(parent) = plan_path.parent() { let _ = std::fs::create_dir_all(parent); }
                                    let _ = std::fs::write(&plan_path, serde_json::to_string_pretty(&new_plan).unwrap_or_default());
                                    let _ = tx.send(Ok(format!("Borrados: {}  Fallidos: {}  Liberados: {}", rep.succeeded, rep.failed, cleanshini_core::scanner::format_size(rep.total_freed))));
                                }
                                Err(e) => { let _ = tx.send(Err(e.to_string())); }
                            }
                        });
                    }
                }
            }
        });
        if !self.clean_msg.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(80, 200, 120), &self.clean_msg);
        }
        ui.separator();

        // build sorted index list
        let mut indices: Vec<usize> = (0..plan.items.len())
            .filter(|&idx| {
                let pi = &plan.items[idx];
                if self.filter_cat != "all" && pi.item.category.as_str() != self.filter_cat { return false; }
                if !self.search.is_empty() && !pi.item.path.to_string_lossy().to_lowercase().contains(&self.search.to_lowercase()) { return false; }
                if !self.ext_filter.is_empty() {
                    let ext = pi.item.path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    if !ext.contains(&self.ext_filter.to_lowercase()) { return false; }
                }
                true
            })
            .collect();
        let sort_col = self.sort_col.clone();
        let sort_asc = self.sort_asc;
        indices.sort_by(|&a, &b| {
            let pa = &plan.items[a].item;
            let pb = &plan.items[b].item;
            let ord = match sort_col.as_str() {
                "cat" => pa.category.as_str().cmp(pb.category.as_str()),
                "size" => pa.size.cmp(&pb.size),
                "path" => pa.path.cmp(&pb.path),
                "date" => pa.modified.cmp(&pb.modified),
                _ => std::cmp::Ordering::Equal,
            };
            if sort_asc { ord } else { ord.reverse() }
        });

        let mut shown = 0usize;
        let cap = 2000usize;
        let table_h = (ui.available_height() - 220.0).max(200.0);
        egui::ScrollArea::vertical().max_height(table_h).show(ui, |ui| {
            egui::Grid::new("items").striped(true).show(ui, |ui| {
                ui.label("Sel");
                if ui.button(format!("Categoría {}", if self.sort_col=="cat" { if self.sort_asc {"\u{25B2}"} else {"\u{25BC}"} } else {""})).clicked() {
                    let was = self.sort_col == "cat"; self.sort_col = "cat".into();
                    if was { self.sort_asc = !self.sort_asc; } else { self.sort_asc = true; }
                }
                if ui.button(format!("Tamaño {}", if self.sort_col=="size" { if self.sort_asc {"\u{25B2}"} else {"\u{25BC}"} } else {""})).clicked() {
                    let was = self.sort_col == "size"; self.sort_col = "size".into();
                    if was { self.sort_asc = !self.sort_asc; } else { self.sort_asc = false; }
                }
                if ui.button(format!("Ruta {}", if self.sort_col=="path" { if self.sort_asc {"\u{25B2}"} else {"\u{25BC}"} } else {""})).clicked() {
                    let was = self.sort_col == "path"; self.sort_col = "path".into();
                    if was { self.sort_asc = !self.sort_asc; } else { self.sort_asc = true; }
                }
                if ui.button(format!("Fecha {}", if self.sort_col=="date" { if self.sort_asc {"\u{25B2}"} else {"\u{25BC}"} } else {""})).clicked() {
                    let was = self.sort_col == "date"; self.sort_col = "date".into();
                    if was { self.sort_asc = !self.sort_asc; } else { self.sort_asc = false; }
                }
                ui.end_row();
                for &idx in &indices {
                    if shown >= cap { break; }
                    let pi = &plan.items[idx];
                    let row_top = ui.cursor().min.y;
                    let row_left = ui.cursor().min.x;
                    ui.checkbox(&mut self.selected[idx], "");
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 16.0), egui::Sense::hover());
                    let color = category_color(pi.item.category.as_str());
                    ui.painter().rect_filled(rect, 4.0, color.gamma_multiply(0.35));
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, cat_display(self.lang, pi.item.category.as_str()), egui::FontId::proportional(11.0), color);
                    ui.label(egui::RichText::new(cleanshini_core::scanner::format_size(pi.item.size)).color(egui::Color32::from_rgb(120, 200, 255)));
                    ui.label(pi.item.path.display().to_string());
                    ui.label(egui::RichText::new(format!("{}", pi.item.modified.elapsed().map(|d| format!("hace {} d", d.as_secs()/86400)).unwrap_or_else(|_| "?".into()))).color(egui::Color32::GRAY).size(10.0));
                    ui.end_row();
                    let row_bottom = ui.cursor().min.y;
                    let row_rect = egui::Rect::from_min_max(
                        egui::pos2(row_left, row_top),
                        egui::pos2(ui.max_rect().max.x, row_bottom),
                    );
                    if ui.rect_contains_pointer(row_rect) {
                        ui.painter().rect_filled(row_rect, 0.0, egui::Color32::from_rgba_unmultiplied(100, 160, 255, 35));
                    }
                    shown += 1;
                }
            });
        });
        if shown >= cap { ui.label(format!("Mostrando primeros {} items. Usá el filtro/búsqueda para acotar.", cap)); }

        for (idx, pi) in plan.items.iter_mut().enumerate() {
            pi.selected = self.selected[idx];
            pi.action = if self.permanent { DeleteMode::Permanent } else { DeleteMode::RecycleBin };
        }

        ui.separator();
        ui.label(format!("Seleccionados: {} ({})", sel_count, cleanshini_core::scanner::format_size(sel_size)));
        ui.label(egui::RichText::new(format!("{}: {} {}", s.sorted_by, self.sort_col, if self.sort_asc { "\u{25B2}" } else { "\u{25BC}" })).color(egui::Color32::GRAY).size(11.0));
        ui.separator();
        let p = plan.clone();
        draw_category_chart(ui, &p, true, self.lang);
        if !self.clean_msg.is_empty() { ui.label(&self.clean_msg); }
    }

    fn ui_rules(&mut self, ui: &mut egui::Ui) {
        let s = strings(self.lang);
        ui.label(s.rules_title);
        ui.horizontal(|ui| { ui.label(s.min_age); ui.add(egui::DragValue::new(&mut self.rules.min_age_days)); });
        ui.horizontal(|ui| { ui.label(s.min_size); ui.add(egui::DragValue::new(&mut self.rules.min_size_bytes)); });
        ui.horizontal(|ui| { ui.label(s.large_threshold); ui.add(egui::DragValue::new(&mut self.rules.large_min_bytes)); });
        ui.label(s.categories_enabled);
        for cat in Category::all() {
            let mut enabled = self.rules.categories.contains(&cat);
            if ui.checkbox(&mut enabled, cat.as_str()).changed() {
                if enabled { if !self.rules.categories.contains(&cat) { self.rules.categories.push(cat); } }
                else { self.rules.categories.retain(|c| *c != cat); }
            }
        }
        ui.label("Discos a escanear:");
        let drives = cleanshini_core::list_drives();
        for d in &drives {
            let mut checked = self.rules.selected_drives.contains(&d.root);
            if ui.checkbox(&mut checked, format!("{} ({})", d.label, d.root)).changed() {
                if checked {
                    if !self.rules.selected_drives.contains(&d.root) { self.rules.selected_drives.push(d.root.clone()); }
                } else {
                    self.rules.selected_drives.retain(|x| x != &d.root);
                }
            }
        }
        ui.add_space(4.0);

        ui.label(s.excluded_paths);
        let mut to_remove = None;
        for (i, p) in self.rules.exclude_paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(p.display().to_string());
                if ui.button(s.remove).clicked() { to_remove = Some(i); }
            });
        }
        if let Some(i) = to_remove { self.rules.exclude_paths.remove(i); }

        let mut new_excl = ui.data_mut(|d| d.get_temp::<String>(egui::Id::new("newexcl")).unwrap_or_default());
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut new_excl);
            if ui.button(s.add_exclusion).clicked() && !new_excl.is_empty() {
                let p = std::path::PathBuf::from(&new_excl);
                if !self.rules.exclude_paths.contains(&p) { self.rules.exclude_paths.push(p); }
                new_excl.clear();
            }
        });
        ui.data_mut(|d| d.insert_temp(egui::Id::new("newexcl"), new_excl));

        ui.separator();
        if ui.button(s.save_rules).clicked() { let _ = save_rules(&self.rules); }
        if ui.button(s.reset_rules).clicked() { self.rules = RuleSet::default(); }
    }

    fn ui_audit(&mut self, ui: &mut egui::Ui) {
        let s = strings(self.lang);
        if ui.button(s.audit_reload).clicked() { self.reports = read_reports(20).unwrap_or_default(); }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for r in &self.reports {
                ui.label(egui::RichText::new(format!("{} - borrados: {} fallidos: {} liberados: {}", r.started_at, r.succeeded, r.failed, cleanshini_core::scanner::format_size(r.total_freed))).color(egui::Color32::from_rgb(120, 180, 255)));
                for rec in &r.records {
                    let (st, col) = if rec.success { ("OK", egui::Color32::from_rgb(80, 200, 120)) } else { ("FAIL", egui::Color32::from_rgb(230, 80, 80)) };
                    ui.label(egui::RichText::new(format!("  [{}] {} ({})", st, rec.path.display(), cleanshini_core::scanner::format_size(rec.size))).color(col));
                }
                ui.separator();
            }
        });
    }
}

fn run_gui() -> eframe::Result<()> {
    ensure_dirs().ok();
    let options = eframe::NativeOptions::default();
    eframe::run_native("CleanShini", options, Box::new(|cc| {
        // Load Segoe UI Symbol font for icons/symbols
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(data) = std::fs::read("C:/Windows/Fonts/seguisym.ttf") {
            fonts.font_data.insert("seguisym".to_owned(), egui::FontData::from_owned(data).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                if let Some(list) = fonts.families.get_mut(&family) {
                    list.insert(0, "seguisym".to_owned());
                }
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        Ok(Box::new(AppState::default()))
    }))
}

fn main() {
    let cli = Cli::parse();
    if cli.gui || cli.command.is_none() {
        if let Err(e) = run_gui() { eprintln!("GUI error: {}", e); }
        return;
    }
    // CLI mode: attach a console on Windows (GUI subsystem has none)
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Console::AllocConsole;
        unsafe { let _ = AllocConsole(); }
    }
    if let Err(e) = run_cli(cli.command.unwrap()) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_cli(cmd: Commands) -> Result<()> {
    ensure_dirs()?;
    tracing_subscriber::fmt().init();
    match cmd {
        Commands::Scan { categories, min_age, min_size, json } => cmd_scan(categories, min_age, min_size, json),
        Commands::Plan { json } => cmd_plan(json),
        Commands::Clean { plan, permanent, yes, categories, dry_run } => cmd_clean(plan, permanent, yes, categories, dry_run),
        Commands::Rules { sub } => cmd_rules(sub),
        Commands::Audit { limit, json } => cmd_audit(limit, json),
        Commands::Config { sub } => cmd_config(sub),
        Commands::EmptyRecycleBin => { cleanshini_core::empty_recycle_bin()?; println!("Papelera vaciada."); Ok(()) }
        Commands::SystemCleanup { dism, cleanmgr, hibernation, shadows, all } => {
            let (d, c, h, s) = if all { (true, true, true, true) } else { (dism, cleanmgr, hibernation, shadows) };
            let res = cleanshini_core::run_system_cleanup(d, c, h, s);
            for r in res { println!("{}: {} - {}", r.name, if r.success {"OK"} else {"FAIL"}, r.output.trim()); }
            Ok(())
        }
    }
}
