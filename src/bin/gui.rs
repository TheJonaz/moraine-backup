//! Moraine desktop client, built on GTK 4.
//!
//! The whole backup engine lives in the `moraine` library (config, rsync,
//! rclone, ssh, snapshot, prune, history); this binary is only the view layer.
//! It is feature-gated behind `gui` so the CLI can build without GTK.
//!
//! Tabs:
//!  * Quick Backup — edit targets, run dry-run/backup with a live log.
//!  * Schedule — create schedules and install them in crontab / Task Scheduler.
//!  * Restore — list snapshots, browse the tree, selective restore.
//!  * History — the run log.

use gtk4 as gtk;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;

use moraine::config::{Backend, Config, Frequency, Retention, Schedule, Target};
use moraine::history::{self, LogEntry};
use moraine::{prune, rclone, rsync, snapshot, ssh};

const CONFIG_PATH: &str = "moraine.toml";
const APP_ID: &str = "io.thern.moraine";

// ─────────────────────────── form models ───────────────────────────

#[derive(Default, Clone)]
struct TargetForm {
    name: String,
    backend: Backend,
    host: String,
    user: String,
    port: String,
    key: String,
    password: String,
    dest: String,
    sources: Vec<String>,
    exclude: Vec<String>,
    keep_last: String,
    keep_daily: String,
    keep_weekly: String,
    keep_monthly: String,
}

impl TargetForm {
    fn from_target(t: &Target) -> Self {
        let r = t.retention.clone().unwrap_or_default();
        TargetForm {
            name: t.name.clone(),
            backend: t.backend,
            host: t.host.clone(),
            user: t.user.clone(),
            port: t.port.to_string(),
            key: t.key.clone().unwrap_or_default(),
            password: t.password.clone(),
            dest: t.dest.clone(),
            sources: t.sources.clone(),
            exclude: t.exclude.clone(),
            keep_last: r.keep_last.to_string(),
            keep_daily: r.keep_daily.to_string(),
            keep_weekly: r.keep_weekly.to_string(),
            keep_monthly: r.keep_monthly.to_string(),
        }
    }

    fn retention(&self) -> Retention {
        let n = |s: &str| s.trim().parse().unwrap_or(0);
        Retention {
            keep_last: n(&self.keep_last),
            keep_daily: n(&self.keep_daily),
            keep_weekly: n(&self.keep_weekly),
            keep_monthly: n(&self.keep_monthly),
        }
    }

    fn to_target(&self) -> Target {
        let key = match self.key.trim() {
            "" => None,
            k => Some(k.to_string()),
        };
        let retention = self.retention();
        Target {
            name: self.name.trim().to_string(),
            backend: self.backend,
            host: self.host.trim().to_string(),
            user: self.user.trim().to_string(),
            port: self.port.trim().parse().unwrap_or(22),
            key,
            password: self.password.trim().to_string(),
            dest: self.dest.trim().to_string(),
            sources: clean(&self.sources),
            exclude: clean(&self.exclude),
            retention: if retention.is_empty() {
                None
            } else {
                Some(retention)
            },
        }
    }

    fn label(&self) -> String {
        if self.name.trim().is_empty() {
            "(unnamed target)".to_string()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone)]
struct ScheduleForm {
    name: String,
    target: String,
    frequency: Frequency,
    hour: String,
    minute: String,
    weekday: u8,
    enabled: bool,
}

impl ScheduleForm {
    fn from_schedule(s: &Schedule) -> Self {
        ScheduleForm {
            name: s.name.clone(),
            target: s.target.clone(),
            frequency: s.frequency,
            hour: s.hour.to_string(),
            minute: s.minute.to_string(),
            weekday: s.weekday,
            enabled: s.enabled,
        }
    }

    fn to_schedule(&self) -> Schedule {
        Schedule {
            name: self.name.trim().to_string(),
            target: self.target.trim().to_string(),
            frequency: self.frequency,
            minute: self.minute.trim().parse().unwrap_or(0).min(59),
            hour: self.hour.trim().parse().unwrap_or(0).min(23),
            weekday: self.weekday.min(6),
            enabled: self.enabled,
        }
    }
}

fn clean(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[derive(Clone)]
struct TreeEntry {
    path: String,
    name: String,
    is_dir: bool,
}

// ─────────────────────────── shared state ───────────────────────────

#[derive(Default)]
struct State {
    targets: Vec<TargetForm>,
    selected: Option<usize>,
    schedules: Vec<ScheduleForm>,
    counts: HashMap<String, usize>,
    history: Vec<LogEntry>,
    // Restore
    restore_target: Option<String>,
    snapshots: Vec<String>,
    selected_snapshot: Option<usize>,
    tree: Vec<TreeEntry>,
    cwd: String,
    running: bool,
}

impl State {
    fn load() -> State {
        let mut st = State::default();
        if let Ok(cfg) = Config::load(Path::new(CONFIG_PATH)) {
            st.targets = cfg.targets.iter().map(TargetForm::from_target).collect();
            st.schedules = cfg
                .schedules
                .iter()
                .map(ScheduleForm::from_schedule)
                .collect();
            if !st.targets.is_empty() {
                st.selected = Some(0);
            }
        }
        st.history = history::read(Path::new(CONFIG_PATH));
        st
    }

    fn build_config(&self) -> Config {
        Config {
            targets: self.targets.iter().map(|f| f.to_target()).collect(),
            schedules: self.schedules.iter().map(|f| f.to_schedule()).collect(),
        }
    }

    fn save(&self) -> Result<(), String> {
        self.build_config()
            .save(Path::new(CONFIG_PATH))
            .map_err(|e| format!("{e:#}"))
    }

    fn selected_target(&self) -> Option<&TargetForm> {
        self.selected.and_then(|i| self.targets.get(i))
    }
}

/// Widgets the signal handlers need to read or refresh.
struct Ui {
    window: gtk::ApplicationWindow,
    target_list: gtk::ListBox,
    // Connection (selected target)
    name: gtk::Entry,
    backend: gtk::DropDown,
    host: gtk::Entry,
    port: gtk::Entry,
    // Log + status
    log: gtk::TextView,
    status: gtk::Label,
    run_btn: gtk::Button,
    dry_btn: gtk::Button,
    // Schedule tab
    sched_list: gtk::ListBox,
    // Restore tab
    restore_target: gtk::DropDown,
    snap_list: gtk::ListBox,
    tree_list: gtk::ListBox,
    restore_dest: gtk::Entry,
    crumb: gtk::Label,
    // History tab
    history_list: gtk::ListBox,
}

type Shared = Rc<RefCell<State>>;

// Messages streamed from a worker thread to the main loop.
enum Worker {
    Line(String),
    Done(bool, String, Option<(String, String, String)>), // ok, detail, (op,target,info)
}

// ─────────────────────────── entry point ───────────────────────────

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        // Don't require the D-Bus session bus for single-instance handling.
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);
    app.run()
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

const CSS: &str = r#"
window { background-color: #0b1a2c; color: #e8eef6; }
.card { background-color: #14273f; border: 1px solid #1f3a59; border-radius: 12px; padding: 14px; }
.title { font-size: 22px; font-weight: 800; }
.subtitle { color: #8aa0bd; font-size: 12px; }
.muted { color: #8aa0bd; font-size: 12px; }
.section { font-weight: 700; }
.accent { background-image: none; background-color: #0fd4a0; color: #06231b; font-weight: 700; }
.accent:hover { background-color: #1fe3b3; }
.danger { color: #ff6b6b; }
button { border-radius: 8px; }
entry { border-radius: 8px; }
row.selected-target { background-color: #0fd4a0; color: #06231b; border-radius: 8px; }
.crumb { color: #8aa0bd; font-family: monospace; font-size: 12px; }
textview, textview text { background-color: #0a1626; color: #cfe6dd; font-family: monospace; font-size: 12px; }
.statusbar { color: #8aa0bd; }
.linkbtn { background: none; border: none; color: #2e8be0; padding: 2px; }
"#;

fn build_ui(app: &gtk::Application) {
    let state: Shared = Rc::new(RefCell::new(State::load()));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Moraine Backup")
        .default_width(1040)
        .default_height(720)
        .build();

    // Header: logo + title + tab switcher.
    let header = gtk::HeaderBar::new();
    let logo = gtk::Image::from_file(asset("moraine-64.png"));
    logo.set_pixel_size(28);
    let title_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    title_box.append(&logo);
    let title = gtk::Label::new(Some("Moraine"));
    title.add_css_class("title");
    title_box.append(&title);
    header.set_title_widget(Some(&title_box));

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    header.pack_end(&switcher);
    window.set_titlebar(Some(&header));

    // Build the Ui struct (widgets shared with handlers).
    let ui = Rc::new(Ui {
        window: window.clone(),
        target_list: gtk::ListBox::new(),
        name: gtk::Entry::new(),
        backend: gtk::DropDown::from_strings(&["ssh", "rclone", "ftp"]),
        host: gtk::Entry::new(),
        port: gtk::Entry::new(),
        log: gtk::TextView::new(),
        status: gtk::Label::new(Some("Ready")),
        run_btn: gtk::Button::with_label("Run backup"),
        dry_btn: gtk::Button::with_label("Dry run"),
        sched_list: gtk::ListBox::new(),
        restore_target: gtk::DropDown::from_strings(&[]),
        snap_list: gtk::ListBox::new(),
        tree_list: gtk::ListBox::new(),
        restore_dest: gtk::Entry::new(),
        crumb: gtk::Label::new(Some("/")),
        history_list: gtk::ListBox::new(),
    });

    stack.add_titled(&build_quick_tab(&state, &ui), Some("quick"), "Quick Backup");
    stack.add_titled(&build_schedule_tab(&state, &ui), Some("sched"), "Schedule");
    stack.add_titled(&build_restore_tab(&state, &ui), Some("restore"), "Restore");
    stack.add_titled(&build_history_tab(&ui), Some("history"), "History");

    // Status bar + footer.
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.append(&stack);

    ui.status.add_css_class("statusbar");
    ui.status.set_halign(gtk::Align::Start);
    let status_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    status_card.add_css_class("card");
    status_card.append(&ui.status);
    root.append(&status_card);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&spacer);
    let link = gtk::Button::with_label("by Jonaz Thern");
    link.add_css_class("linkbtn");
    link.connect_clicked(|_| {
        let _ = gio::AppInfo::launch_default_for_uri(
            "https://www.thern.io",
            gio::AppLaunchContext::NONE,
        );
    });
    footer.append(&link);
    root.append(&footer);

    window.set_child(Some(&root));

    refresh_targets(&state, &ui);
    refresh_connection(&state, &ui);
    refresh_schedules(&state, &ui);
    refresh_restore_targets(&state, &ui);
    refresh_history(&state, &ui);

    window.present();
}

fn asset(name: &str) -> String {
    // Installed location first, then the source tree (dev).
    for base in ["/usr/share/moraine/assets", "assets"] {
        let p = format!("{base}/{name}");
        if Path::new(&p).exists() {
            return p;
        }
    }
    format!("assets/{name}")
}

// ─────────────────────────── Quick Backup tab ───────────────────────────

fn build_quick_tab(state: &Shared, ui: &Rc<Ui>) -> gtk::Widget {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);

    // Targets panel
    let targets_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    targets_card.add_css_class("card");
    targets_card.set_width_request(260);
    let t_title = gtk::Label::new(Some("Targets"));
    t_title.add_css_class("section");
    t_title.set_halign(gtk::Align::Start);
    targets_card.append(&t_title);
    ui.target_list.set_selection_mode(gtk::SelectionMode::None);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&ui.target_list));
    targets_card.append(&scroll);
    let add_btn = gtk::Button::with_label("+ New target");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        add_btn.connect_clicked(move |_| {
            let mut s = st.borrow_mut();
            let mut f = TargetForm {
                port: "22".to_string(),
                ..Default::default()
            };
            f.name = format!("target-{}", s.targets.len() + 1);
            s.targets.push(f);
            s.selected = Some(s.targets.len() - 1);
            drop(s);
            refresh_targets(&st, &ui2);
            refresh_connection(&st, &ui2);
        });
    }
    targets_card.append(&add_btn);
    top.append(&targets_card);

    // Connection panel
    let conn = gtk::Box::new(gtk::Orientation::Vertical, 10);
    conn.add_css_class("card");
    conn.set_hexpand(true);
    let c_title = gtk::Label::new(Some("Connection"));
    c_title.add_css_class("section");
    c_title.set_halign(gtk::Align::Start);
    conn.append(&c_title);

    conn.append(&labeled("Name", &ui.name));
    {
        let st = state.clone();
        let ui2 = ui.clone();
        ui.name.connect_changed(move |e| {
            let sel = st.borrow().selected;
            if let Some(i) = sel {
                st.borrow_mut().targets[i].name = e.text().to_string();
            }
            refresh_target_rows(&st, &ui2);
        });
    }

    let backend_lbl = gtk::Label::new(Some("Backend"));
    backend_lbl.add_css_class("muted");
    backend_lbl.set_halign(gtk::Align::Start);
    conn.append(&backend_lbl);
    conn.append(&ui.backend);
    {
        let st = state.clone();
        ui.backend.connect_selected_notify(move |d| {
            let sel = st.borrow().selected;
            if let Some(i) = sel {
                st.borrow_mut().targets[i].backend = Backend::ALL[d.selected() as usize % 3];
            }
        });
    }

    let hp = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let host_box = labeled("Host / IP", &ui.host);
    host_box.set_hexpand(true);
    hp.append(&host_box);
    ui.port.set_width_request(80);
    hp.append(&labeled("Port", &ui.port));
    conn.append(&hp);
    bind_entry(&ui.host, state, |f, v| f.host = v);
    bind_entry(&ui.port, state, |f, v| f.port = v);

    // Settings + actions row
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let settings_btn = gtk::Button::with_label("⚙ Settings");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        settings_btn.connect_clicked(move |_| open_settings(&st, &ui2));
    }
    actions.append(&settings_btn);
    let del_btn = gtk::Button::with_label("Delete");
    del_btn.add_css_class("danger");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        del_btn.connect_clicked(move |_| confirm_delete_target(&st, &ui2));
    }
    actions.append(&del_btn);
    let test_btn = gtk::Button::with_label("Test connection");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        test_btn.connect_clicked(move |_| test_connection(&st, &ui2));
    }
    actions.append(&test_btn);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&spacer);
    {
        let st = state.clone();
        let ui2 = ui.clone();
        ui.dry_btn
            .connect_clicked(move |_| run_backup(&st, &ui2, true));
    }
    actions.append(&ui.dry_btn);
    ui.run_btn.add_css_class("accent");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        ui.run_btn
            .connect_clicked(move |_| run_backup(&st, &ui2, false));
    }
    actions.append(&ui.run_btn);
    conn.append(&actions);

    top.append(&conn);
    outer.append(&top);

    // Log
    let log_card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    log_card.add_css_class("card");
    log_card.set_vexpand(true);
    ui.log.set_editable(false);
    ui.log.set_monospace(true);
    ui.log.set_wrap_mode(gtk::WrapMode::WordChar);
    let log_scroll = gtk::ScrolledWindow::new();
    log_scroll.set_vexpand(true);
    log_scroll.set_min_content_height(180);
    log_scroll.set_child(Some(&ui.log));
    log_card.append(&log_scroll);
    outer.append(&log_card);

    outer.upcast()
}

fn labeled(label: &str, entry: &gtk::Entry) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let l = gtk::Label::new(Some(label));
    l.add_css_class("muted");
    l.set_halign(gtk::Align::Start);
    b.append(&l);
    b.append(entry);
    b
}

/// Wire an entry so editing updates the selected target's field.
fn bind_entry(entry: &gtk::Entry, state: &Shared, set: fn(&mut TargetForm, String)) {
    let st = state.clone();
    entry.connect_changed(move |e| {
        let sel = st.borrow().selected;
        if let Some(i) = sel {
            set(&mut st.borrow_mut().targets[i], e.text().to_string());
        }
    });
}

fn refresh_targets(state: &Shared, ui: &Rc<Ui>) {
    while let Some(child) = ui.target_list.first_child() {
        ui.target_list.remove(&child);
    }
    let s = state.borrow();
    for (i, t) in s.targets.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let count = s
            .counts
            .get(&t.name)
            .map(|c| format!("  ({c})"))
            .unwrap_or_default();
        let lbl = gtk::Label::new(Some(&format!("{}{}", t.label(), count)));
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);
        hb.append(&lbl);
        row.set_child(Some(&hb));
        if Some(i) == s.selected {
            row.add_css_class("selected-target");
        }
        let st = state.clone();
        let ui2 = ui.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_released(move |_, _, _, _| {
            st.borrow_mut().selected = Some(i);
            refresh_targets(&st, &ui2);
            refresh_connection(&st, &ui2);
        });
        row.add_controller(gesture);
        ui.target_list.append(&row);
    }
}

/// Cheaper refresh: just re-render the labels (used while typing the name).
fn refresh_target_rows(state: &Shared, ui: &Rc<Ui>) {
    refresh_targets(state, ui);
}

fn refresh_connection(state: &Shared, ui: &Rc<Ui>) {
    // Clone out the form and drop the borrow before touching widgets: set_text
    // re-enters the `changed` handlers, which take borrow_mut.
    let f = state
        .borrow()
        .selected_target()
        .cloned()
        .unwrap_or_default();
    set_text_silent(&ui.name, &f.name);
    set_text_silent(&ui.host, &f.host);
    set_text_silent(&ui.port, &f.port);
    let idx = Backend::ALL
        .iter()
        .position(|b| *b == f.backend)
        .unwrap_or(0);
    ui.backend.set_selected(idx as u32);
}

/// Set entry text without re-triggering loops (GTK changed fires regardless; we
/// just accept it because the value is identical).
fn set_text_silent(entry: &gtk::Entry, value: &str) {
    if entry.text() != value {
        entry.set_text(value);
    }
}

// ─────────────────────────── Settings modal ───────────────────────────

fn open_settings(state: &Shared, ui: &Rc<Ui>) {
    let Some(i) = state.borrow().selected else {
        set_status(ui, "No target selected");
        return;
    };
    let win = gtk::Window::builder()
        .transient_for(&ui.window)
        .modal(true)
        .title("Settings")
        .default_width(640)
        .default_height(620)
        .build();
    let scroll = gtk::ScrolledWindow::new();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.set_margin_top(14);
    body.set_margin_bottom(14);
    body.set_margin_start(14);
    body.set_margin_end(14);

    let f = state.borrow().targets[i].clone();

    // User
    let user = gtk::Entry::new();
    user.set_text(&f.user);
    body.append(&labeled("User", &user));
    {
        let st = state.clone();
        user.connect_changed(move |e| st.borrow_mut().targets[i].user = e.text().to_string());
    }

    // SSH key + browse
    let key = gtk::Entry::new();
    key.set_text(&f.key);
    let key_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    key.set_hexpand(true);
    key_row.append(&key);
    let key_browse = gtk::Button::with_label("Browse…");
    key_row.append(&key_browse);
    let key_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let kl = gtk::Label::new(Some("SSH key (optional)"));
    kl.add_css_class("muted");
    kl.set_halign(gtk::Align::Start);
    key_box.append(&kl);
    key_box.append(&key_row);
    body.append(&key_box);
    {
        let st = state.clone();
        key.connect_changed(move |e| st.borrow_mut().targets[i].key = e.text().to_string());
    }
    {
        let win2 = win.clone();
        let key2 = key.clone();
        key_browse.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder().title("Select SSH key").build();
            let key3 = key2.clone();
            dialog.open(Some(&win2), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(p) = file.path() {
                        key3.set_text(&p.display().to_string());
                    }
                }
            });
        });
    }

    // Secret
    let secret = gtk::PasswordEntry::new();
    secret.set_show_peek_icon(true);
    secret.set_text(&f.password);
    let sl = gtk::Label::new(Some("Key passphrase / login password (optional)"));
    sl.add_css_class("muted");
    sl.set_halign(gtk::Align::Start);
    body.append(&sl);
    body.append(&secret);
    {
        let st = state.clone();
        secret.connect_changed(move |e| st.borrow_mut().targets[i].password = e.text().to_string());
    }

    // Destination
    let dest = gtk::Entry::new();
    dest.set_text(&f.dest);
    body.append(&labeled("Destination on target", &dest));
    {
        let st = state.clone();
        dest.connect_changed(move |e| st.borrow_mut().targets[i].dest = e.text().to_string());
    }

    // Sources + exclude list editors
    body.append(&list_editor(
        state,
        i,
        "Sources (files/folders on the client)",
        true,
        &win,
    ));
    body.append(&list_editor(
        state,
        i,
        "Exclude patterns (optional)",
        false,
        &win,
    ));

    // Retention
    let rl = gtk::Label::new(Some("Retention (0 = keep all)"));
    rl.add_css_class("section");
    rl.set_halign(gtk::Align::Start);
    body.append(&rl);
    let ret = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    ret.append(&retention_field(state, i, "Last", f.keep_last, |f, v| {
        f.keep_last = v
    }));
    ret.append(&retention_field(state, i, "Daily", f.keep_daily, |f, v| {
        f.keep_daily = v
    }));
    ret.append(&retention_field(
        state,
        i,
        "Weekly",
        f.keep_weekly,
        |f, v| f.keep_weekly = v,
    ));
    ret.append(&retention_field(
        state,
        i,
        "Monthly",
        f.keep_monthly,
        |f, v| f.keep_monthly = v,
    ));
    body.append(&ret);

    // Buttons
    let btns = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let prune_btn = gtk::Button::with_label("Prune now");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        prune_btn.connect_clicked(move |_| prune_now(&st, &ui2));
    }
    btns.append(&prune_btn);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    btns.append(&spacer);
    let save_btn = gtk::Button::with_label("Save & close");
    save_btn.add_css_class("accent");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        let win2 = win.clone();
        save_btn.connect_clicked(move |_| {
            match st.borrow().save() {
                Ok(()) => set_status(&ui2, &format!("Saved {CONFIG_PATH}")),
                Err(e) => set_status(&ui2, &format!("Save error: {e}")),
            }
            refresh_targets(&st, &ui2);
            refresh_connection(&st, &ui2);
            win2.close();
        });
    }
    btns.append(&save_btn);
    body.append(&btns);

    scroll.set_child(Some(&body));
    win.set_child(Some(&scroll));
    win.present();
}

fn retention_field(
    state: &Shared,
    i: usize,
    label: &str,
    value: String,
    set: fn(&mut TargetForm, String),
) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let l = gtk::Label::new(Some(label));
    l.add_css_class("muted");
    b.append(&l);
    let e = gtk::Entry::new();
    e.set_text(&value);
    e.set_width_request(70);
    let st = state.clone();
    e.connect_changed(move |e| set(&mut st.borrow_mut().targets[i], e.text().to_string()));
    b.append(&e);
    b
}

/// A simple add/remove list editor bound to a target's sources or exclude list.
fn list_editor(
    state: &Shared,
    i: usize,
    label: &str,
    is_sources: bool,
    win: &gtk::Window,
) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let l = gtk::Label::new(Some(label));
    l.add_css_class("muted");
    l.set_halign(gtk::Align::Start);
    outer.append(&l);
    let listbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
    outer.append(&listbox);

    let rebuild = Rc::new({
        let state = state.clone();
        let listbox = listbox.clone();
        let win = win.clone();
        move || {
            while let Some(c) = listbox.first_child() {
                listbox.remove(&c);
            }
            let items = if is_sources {
                state.borrow().targets[i].sources.clone()
            } else {
                state.borrow().targets[i].exclude.clone()
            };
            for (j, item) in items.iter().enumerate() {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let e = gtk::Entry::new();
                e.set_text(item);
                e.set_hexpand(true);
                {
                    let st = state.clone();
                    e.connect_changed(move |e| {
                        let mut s = st.borrow_mut();
                        let v = if is_sources {
                            &mut s.targets[i].sources
                        } else {
                            &mut s.targets[i].exclude
                        };
                        if j < v.len() {
                            v[j] = e.text().to_string();
                        }
                    });
                }
                row.append(&e);
                if is_sources {
                    let browse = gtk::Button::with_label("Browse…");
                    let st = state.clone();
                    let e2 = e.clone();
                    let win2 = win.clone();
                    browse.connect_clicked(move |_| {
                        let dialog = gtk::FileDialog::builder().title("Select a folder").build();
                        let st2 = st.clone();
                        let e3 = e2.clone();
                        dialog.select_folder(Some(&win2), gio::Cancellable::NONE, move |res| {
                            if let Ok(file) = res {
                                if let Some(p) = file.path() {
                                    let path = p.display().to_string();
                                    e3.set_text(&path);
                                    if j < st2.borrow().targets[i].sources.len() {
                                        st2.borrow_mut().targets[i].sources[j] = path;
                                    }
                                }
                            }
                        });
                    });
                    row.append(&browse);
                }
                let rm = gtk::Button::with_label("✕");
                row.append(&rm);
                listbox.append(&row);
            }
        }
    });

    let add = gtk::Button::with_label("+ Add");
    {
        let state = state.clone();
        let rebuild = rebuild.clone();
        add.connect_clicked(move |_| {
            let mut s = state.borrow_mut();
            if is_sources {
                s.targets[i].sources.push(String::new());
            } else {
                s.targets[i].exclude.push(String::new());
            }
            drop(s);
            rebuild();
        });
    }
    outer.append(&add);
    rebuild();
    outer
}

// ─────────────────────────── Schedule tab ───────────────────────────

fn build_schedule_tab(state: &Shared, ui: &Rc<Ui>) -> gtk::Widget {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    let title = gtk::Label::new(Some("Schedules"));
    title.add_css_class("section");
    title.set_halign(gtk::Align::Start);
    card.append(&title);

    ui.sched_list.set_selection_mode(gtk::SelectionMode::None);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&ui.sched_list));
    card.append(&scroll);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let add = gtk::Button::with_label("+ New schedule");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        add.connect_clicked(move |_| {
            let target = st
                .borrow()
                .targets
                .first()
                .map(|t| t.name.clone())
                .unwrap_or_default();
            st.borrow_mut().schedules.push(ScheduleForm {
                name: format!("schedule-{}", st.borrow().schedules.len() + 1),
                target,
                frequency: Frequency::Daily,
                hour: "2".to_string(),
                minute: "0".to_string(),
                weekday: 1,
                enabled: true,
            });
            refresh_schedules(&st, &ui2);
        });
    }
    row.append(&add);
    let install = gtk::Button::with_label(&format!("Install to {}", scheduler_name()));
    install.add_css_class("accent");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        install.connect_clicked(move |_| {
            let _ = st.borrow().save();
            let scheds: Vec<Schedule> = st
                .borrow()
                .schedules
                .iter()
                .map(|f| f.to_schedule())
                .collect();
            match install_schedules(&scheds) {
                Ok(n) => set_status(
                    &ui2,
                    &format!("Installed {n} schedule(s) to {}", scheduler_name()),
                ),
                Err(e) => set_status(&ui2, &format!("{} error: {e}", scheduler_name())),
            }
        });
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);
    row.append(&install);
    card.append(&row);

    outer.append(&card);
    outer.upcast()
}

fn refresh_schedules(state: &Shared, ui: &Rc<Ui>) {
    while let Some(c) = ui.sched_list.first_child() {
        ui.sched_list.remove(&c);
    }
    let target_names: Vec<String> = state
        .borrow()
        .targets
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let len = state.borrow().schedules.len();
    for i in 0..len {
        let f = state.borrow().schedules[i].clone();
        let row = gtk::ListBoxRow::new();
        let grid = gtk::Box::new(gtk::Orientation::Vertical, 6);
        grid.add_css_class("card");

        let line1 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let name = gtk::Entry::new();
        name.set_text(&f.name);
        name.set_hexpand(true);
        {
            let st = state.clone();
            name.connect_changed(move |e| st.borrow_mut().schedules[i].name = e.text().to_string());
        }
        line1.append(&name);
        let enabled = gtk::Switch::new();
        enabled.set_active(f.enabled);
        {
            let st = state.clone();
            enabled.connect_active_notify(move |s| {
                st.borrow_mut().schedules[i].enabled = s.is_active()
            });
        }
        line1.append(&enabled);
        let del = gtk::Button::with_label("✕");
        {
            let st = state.clone();
            let ui2 = ui.clone();
            del.connect_clicked(move |_| {
                st.borrow_mut().schedules.remove(i);
                refresh_schedules(&st, &ui2);
            });
        }
        line1.append(&del);
        grid.append(&line1);

        let line2 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        // Target dropdown
        let strs: Vec<&str> = target_names.iter().map(|s| s.as_str()).collect();
        let target_dd = gtk::DropDown::from_strings(&strs);
        if let Some(pos) = target_names.iter().position(|n| *n == f.target) {
            target_dd.set_selected(pos as u32);
        }
        {
            let st = state.clone();
            let names = target_names.clone();
            target_dd.connect_selected_notify(move |d| {
                if let Some(n) = names.get(d.selected() as usize) {
                    st.borrow_mut().schedules[i].target = n.clone();
                }
            });
        }
        line2.append(&target_dd);
        // Frequency
        let freq = gtk::DropDown::from_strings(&["Hourly", "Daily", "Weekly"]);
        let fi = match f.frequency {
            Frequency::Hourly => 0,
            Frequency::Daily => 1,
            Frequency::Weekly => 2,
        };
        freq.set_selected(fi);
        {
            let st = state.clone();
            freq.connect_selected_notify(move |d| {
                st.borrow_mut().schedules[i].frequency = Frequency::ALL[d.selected() as usize % 3];
            });
        }
        line2.append(&freq);
        // Hour / minute
        let hour = gtk::Entry::new();
        hour.set_text(&f.hour);
        hour.set_width_request(60);
        hour.set_placeholder_text(Some("HH"));
        {
            let st = state.clone();
            hour.connect_changed(move |e| st.borrow_mut().schedules[i].hour = e.text().to_string());
        }
        line2.append(&hour);
        let minute = gtk::Entry::new();
        minute.set_text(&f.minute);
        minute.set_width_request(60);
        minute.set_placeholder_text(Some("MM"));
        {
            let st = state.clone();
            minute.connect_changed(move |e| {
                st.borrow_mut().schedules[i].minute = e.text().to_string()
            });
        }
        line2.append(&minute);
        grid.append(&line2);

        let cron = gtk::Label::new(Some(&format!("cron: {}", f.to_schedule().cron())));
        cron.add_css_class("muted");
        cron.set_halign(gtk::Align::Start);
        grid.append(&cron);

        row.set_child(Some(&grid));
        ui.sched_list.append(&row);
    }
}

// ─────────────────────────── Restore tab ───────────────────────────

fn build_restore_tab(state: &Shared, ui: &Rc<Ui>) -> gtk::Widget {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let tl = gtk::Label::new(Some("Target:"));
    top.append(&tl);
    top.append(&ui.restore_target);
    {
        let st = state.clone();
        let ui2 = ui.clone();
        ui.restore_target.connect_selected_notify(move |d| {
            let names: Vec<String> = st.borrow().targets.iter().map(|t| t.name.clone()).collect();
            if let Some(n) = names.get(d.selected() as usize) {
                st.borrow_mut().restore_target = Some(n.clone());
                load_snapshots(&st, &ui2);
            }
        });
    }
    let refresh = gtk::Button::with_label("Load snapshots");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        refresh.connect_clicked(move |_| load_snapshots(&st, &ui2));
    }
    top.append(&refresh);
    outer.append(&top);

    let cols = gtk::Box::new(gtk::Orientation::Horizontal, 12);

    // Snapshots
    let snap_card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    snap_card.add_css_class("card");
    snap_card.set_width_request(260);
    let sl = gtk::Label::new(Some("Snapshots"));
    sl.add_css_class("section");
    sl.set_halign(gtk::Align::Start);
    snap_card.append(&sl);
    let snap_scroll = gtk::ScrolledWindow::new();
    snap_scroll.set_vexpand(true);
    snap_scroll.set_child(Some(&ui.snap_list));
    snap_card.append(&snap_scroll);
    cols.append(&snap_card);

    // Tree
    let tree_card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    tree_card.add_css_class("card");
    tree_card.set_hexpand(true);
    let trl = gtk::Label::new(Some("Files"));
    trl.add_css_class("section");
    trl.set_halign(gtk::Align::Start);
    tree_card.append(&trl);
    ui.crumb.add_css_class("crumb");
    ui.crumb.set_halign(gtk::Align::Start);
    tree_card.append(&ui.crumb);
    let tree_scroll = gtk::ScrolledWindow::new();
    tree_scroll.set_vexpand(true);
    tree_scroll.set_child(Some(&ui.tree_list));
    tree_card.append(&tree_scroll);
    cols.append(&tree_card);

    outer.append(&cols);

    // Restore controls
    let ctl = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let dl = gtk::Label::new(Some("Restore to:"));
    ctl.append(&dl);
    ui.restore_dest.set_hexpand(true);
    ui.restore_dest
        .set_placeholder_text(Some("/path/to/restore/destination"));
    ctl.append(&ui.restore_dest);
    let browse = gtk::Button::with_label("Browse…");
    {
        let ui2 = ui.clone();
        browse.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder()
                .title("Restore destination")
                .build();
            let dest = ui2.restore_dest.clone();
            dialog.select_folder(Some(&ui2.window), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(p) = file.path() {
                        dest.set_text(&p.display().to_string());
                    }
                }
            });
        });
    }
    ctl.append(&browse);
    let dry = gtk::Button::with_label("Dry run");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        dry.connect_clicked(move |_| run_restore(&st, &ui2, true));
    }
    ctl.append(&dry);
    let restore = gtk::Button::with_label("Restore");
    restore.add_css_class("accent");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        restore.connect_clicked(move |_| run_restore(&st, &ui2, false));
    }
    ctl.append(&restore);
    outer.append(&ctl);

    outer.upcast()
}

fn refresh_restore_targets(state: &Shared, ui: &Rc<Ui>) {
    let names: Vec<String> = state
        .borrow()
        .targets
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let strs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let model = gtk::StringList::new(&strs);
    ui.restore_target.set_model(Some(&model));
    if !names.is_empty() {
        ui.restore_target.set_selected(0);
        state.borrow_mut().restore_target = Some(names[0].clone());
    }
}

fn refresh_snapshots(state: &Shared, ui: &Rc<Ui>) {
    while let Some(c) = ui.snap_list.first_child() {
        ui.snap_list.remove(&c);
    }
    let len = state.borrow().snapshots.len();
    for i in 0..len {
        let snap = state.borrow().snapshots[i].clone();
        let row = gtk::ListBoxRow::new();
        let lbl = gtk::Label::new(Some(&snap));
        lbl.set_halign(gtk::Align::Start);
        row.set_child(Some(&lbl));
        let st = state.clone();
        let ui2 = ui.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_released(move |_, _, _, _| {
            st.borrow_mut().selected_snapshot = Some(i);
            st.borrow_mut().cwd = String::new();
            load_tree(&st, &ui2);
        });
        row.add_controller(gesture);
        ui.snap_list.append(&row);
    }
}

fn refresh_tree(state: &Shared, ui: &Rc<Ui>) {
    while let Some(c) = ui.tree_list.first_child() {
        ui.tree_list.remove(&c);
    }
    let cwd = state.borrow().cwd.clone();
    ui.crumb.set_text(&format!("/{cwd}"));
    // ".." up entry
    if !cwd.is_empty() {
        let row = gtk::ListBoxRow::new();
        let lbl = gtk::Label::new(Some("📁 .."));
        lbl.set_halign(gtk::Align::Start);
        row.set_child(Some(&lbl));
        let st = state.clone();
        let ui2 = ui.clone();
        let g = gtk::GestureClick::new();
        g.connect_released(move |_, _, _, _| {
            let parent = {
                let c = st.borrow().cwd.clone();
                match c.rfind('/') {
                    Some(p) => c[..p].to_string(),
                    None => String::new(),
                }
            };
            st.borrow_mut().cwd = parent;
            refresh_tree(&st, &ui2);
        });
        row.add_controller(g);
        ui.tree_list.append(&row);
    }
    let entries = state.borrow().tree.clone();
    let prefix = if cwd.is_empty() {
        String::new()
    } else {
        format!("{cwd}/")
    };
    for e in entries.iter() {
        // Show only entries directly within cwd.
        let Some(rest) = e.path.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() || rest.contains('/') {
            continue;
        }
        let row = gtk::ListBoxRow::new();
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let check = gtk::CheckButton::new();
        hb.append(&check);
        let icon = if e.is_dir { "📁" } else { "📄" };
        let lbl = gtk::Label::new(Some(&format!("{icon} {}", e.name)));
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);
        hb.append(&lbl);
        // store the path on the row for selective restore
        unsafe {
            row.set_data("path", e.path.clone());
        }
        row.set_child(Some(&hb));
        if e.is_dir {
            let st = state.clone();
            let ui2 = ui.clone();
            let path = e.path.clone();
            let g = gtk::GestureClick::new();
            g.connect_released(move |_, n, _, _| {
                if n == 2 {
                    st.borrow_mut().cwd = path.clone();
                    refresh_tree(&st, &ui2);
                }
            });
            lbl.add_controller(g);
        }
        ui.tree_list.append(&row);
    }
}

/// Collect the checked paths in the tree (selective restore).
fn checked_paths(ui: &Rc<Ui>) -> Vec<String> {
    let mut out = Vec::new();
    let mut child = ui.tree_list.first_child();
    while let Some(row) = child {
        if let Some(r) = row.downcast_ref::<gtk::ListBoxRow>() {
            if let Some(hb) = r.child().and_downcast::<gtk::Box>() {
                if let Some(check) = hb.first_child().and_downcast::<gtk::CheckButton>() {
                    if check.is_active() {
                        let p: Option<std::ptr::NonNull<String>> = unsafe { r.data("path") };
                        if let Some(p) = p {
                            out.push(unsafe { p.as_ref().clone() });
                        }
                    }
                }
            }
        }
        child = row.next_sibling();
    }
    out
}

// ─────────────────────────── History tab ───────────────────────────

fn build_history_tab(ui: &Rc<Ui>) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("card");
    let title = gtk::Label::new(Some("Run history"));
    title.add_css_class("section");
    title.set_halign(gtk::Align::Start);
    card.append(&title);
    ui.history_list.set_selection_mode(gtk::SelectionMode::None);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&ui.history_list));
    card.append(&scroll);
    card.upcast()
}

fn refresh_history(state: &Shared, ui: &Rc<Ui>) {
    while let Some(c) = ui.history_list.first_child() {
        ui.history_list.remove(&c);
    }
    for e in state.borrow().history.iter() {
        let row = gtk::ListBoxRow::new();
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let mark = gtk::Label::new(Some(if e.ok { "✓" } else { "✗" }));
        mark.add_css_class(if e.ok { "accent" } else { "danger" });
        hb.append(&mark);
        let txt = format!("{}  {}  {}  —  {}", e.time, e.op, e.target, e.detail);
        let lbl = gtk::Label::new(Some(&txt));
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);
        hb.append(&lbl);
        row.set_child(Some(&hb));
        ui.history_list.append(&row);
    }
}

// ─────────────────────────── operations ───────────────────────────

fn set_status(ui: &Rc<Ui>, msg: &str) {
    ui.status.set_text(msg);
}

fn append_log(ui: &Rc<Ui>, line: &str) {
    let buf = ui.log.buffer();
    let mut end = buf.end_iter();
    buf.insert(&mut end, line);
    buf.insert(&mut end, "\n");
    // autoscroll
    let mark = buf.create_mark(None, &buf.end_iter(), false);
    ui.log.scroll_mark_onscreen(&mark);
}

fn set_log(ui: &Rc<Ui>, text: &str) {
    ui.log.buffer().set_text(text);
}

fn set_running(ui: &Rc<Ui>, running: bool) {
    ui.run_btn.set_sensitive(!running);
    ui.dry_btn.set_sensitive(!running);
}

fn confirm_delete_target(state: &Shared, ui: &Rc<Ui>) {
    let Some(i) = state.borrow().selected else {
        return;
    };
    let name = state.borrow().targets[i].label();
    let dialog = gtk::AlertDialog::builder()
        .message(format!("Delete target “{name}”?"))
        .detail("This removes it from the config. Snapshots on the target are not touched.")
        .buttons(["Cancel", "Delete"])
        .cancel_button(0)
        .default_button(0)
        .build();
    let st = state.clone();
    let ui2 = ui.clone();
    dialog.choose(Some(&ui.window), gio::Cancellable::NONE, move |res| {
        if res == Ok(1) {
            let mut s = st.borrow_mut();
            s.targets.remove(i);
            s.selected = if s.targets.is_empty() {
                None
            } else {
                Some(i.min(s.targets.len() - 1))
            };
            drop(s);
            let _ = st.borrow().save();
            refresh_targets(&st, &ui2);
            refresh_connection(&st, &ui2);
            set_status(&ui2, "Target deleted");
        }
    });
}

/// Spawn a worker thread that runs the (prog,args) sequence streaming stdout,
/// and drive a glib future that appends each line to the log.
fn run_stream(
    ui: &Rc<Ui>,
    state: &Shared,
    cmds: Vec<(String, Vec<String>)>,
    env: Vec<(String, String)>,
    pending: Option<(String, String, String)>,
    start_msg: &str,
) {
    state.borrow_mut().running = true;
    set_running(ui, true);
    set_status(ui, start_msg);

    let (tx, rx) = async_channel::unbounded::<Worker>();
    std::thread::spawn(move || {
        for (prog, args) in &cmds {
            let _ = tx.send_blocking(Worker::Line(format!("$ {prog} {}", rsync::render(args))));
            let child = Command::new(prog)
                .args(args)
                .envs(env.clone())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send_blocking(Worker::Done(
                        false,
                        format!("could not start {prog}: {e}"),
                        None,
                    ));
                    return;
                }
            };
            if let Some(out) = child.stdout.take() {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = tx.send_blocking(Worker::Line(line));
                }
            }
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut err);
            }
            let ok = child.wait().map(|s| s.success()).unwrap_or(false);
            if !ok {
                let _ = tx.send_blocking(Worker::Done(false, err, None));
                return;
            }
        }
        let _ = tx.send_blocking(Worker::Done(true, String::new(), pending));
    });

    let ui = ui.clone();
    let state = state.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            match msg {
                Worker::Line(l) => append_log(&ui, &l),
                Worker::Done(ok, detail, pending) => {
                    state.borrow_mut().running = false;
                    set_running(&ui, false);
                    if ok {
                        set_status(&ui, "Done");
                        if let Some((op, target, info)) = pending {
                            log_entry(&state, &op, &target, true, info);
                        }
                    } else {
                        append_log(&ui, &detail);
                        set_status(&ui, "Failed");
                    }
                    refresh_history(&state, &ui);
                }
            }
        }
    });
}

fn log_entry(state: &Shared, op: &str, target: &str, ok: bool, detail: String) {
    let entry = LogEntry::new(op, target, ok, detail);
    let _ = history::append(Path::new(CONFIG_PATH), &entry);
    state.borrow_mut().history = history::read(Path::new(CONFIG_PATH));
}

fn run_backup(state: &Shared, ui: &Rc<Ui>, dry_run: bool) {
    if state.borrow().running {
        return;
    }
    let Some(f) = state.borrow().selected_target().cloned() else {
        set_status(ui, "No target selected");
        return;
    };
    let target = f.to_target();
    if target.host.is_empty() && target.backend.is_ssh() {
        set_status(ui, "Target needs a host");
        return;
    }
    if target.sources.is_empty() {
        set_status(ui, "Target needs at least one source");
        return;
    }
    let _ = state.borrow().save();
    let ts = snapshot::timestamp();
    let cmds = if target.backend.is_ssh() {
        let dest = snapshot::snapshot_dir(&target, &ts);
        let mut args = rsync::build_args(&target, &dest, Some(rsync::LINK_DEST), dry_run);
        ensure_verbose(&mut args);
        let mut c = vec![("rsync".to_string(), args)];
        if !dry_run {
            let latest = snapshot::update_latest_cmd(&target, &ts);
            c.push((
                "ssh".to_string(),
                ssh::remote_command_args(&target, &latest),
            ));
        }
        c
    } else {
        rclone::backup_cmds(&target, &ts, None, dry_run)
    };
    let pending = if dry_run {
        None
    } else {
        Some((
            "backup".to_string(),
            target.name.clone(),
            format!("snapshot {ts}"),
        ))
    };
    set_log(ui, &format!("snapshot {ts}\n"));
    let msg = if dry_run {
        format!("Dry run against {}…", target.name)
    } else {
        format!("Backing up {}…", target.name)
    };
    run_stream(ui, state, cmds, ssh::askpass_env(&target), pending, &msg);
}

fn ensure_verbose(args: &mut Vec<String>) {
    if !args
        .iter()
        .any(|a| a == "-v" || a == "--verbose" || a.starts_with("-v"))
    {
        args.insert(0, "-v".to_string());
    }
}

fn run_restore(state: &Shared, ui: &Rc<Ui>, dry_run: bool) {
    if state.borrow().running {
        return;
    }
    let s = state.borrow();
    let Some(name) = s.restore_target.clone() else {
        drop(s);
        set_status(ui, "Pick a target");
        return;
    };
    let Some(si) = s.selected_snapshot else {
        drop(s);
        set_status(ui, "Pick a snapshot");
        return;
    };
    let ts = s.snapshots[si].clone();
    let Some(f) = s.targets.iter().find(|t| t.name == name).cloned() else {
        drop(s);
        return;
    };
    drop(s);
    let dest = ui.restore_dest.text().to_string();
    if dest.trim().is_empty() {
        set_status(ui, "Pick a restore destination");
        return;
    }
    let target = f.to_target();
    let selected = checked_paths(ui);
    let cmd = if target.backend.is_ssh() {
        let mut args = if selected.is_empty() {
            rsync::restore_args(&target, &ts, &dest, dry_run)
        } else {
            rsync::restore_selected_args(&target, &ts, &selected, &dest, dry_run)
        };
        ensure_verbose(&mut args);
        ("rsync".to_string(), args)
    } else {
        (
            "rclone".to_string(),
            rclone::restore_args(&target, &ts, &selected, &dest, dry_run),
        )
    };
    let pending = if dry_run {
        None
    } else {
        Some(("restore".to_string(), name, format!("{ts} → {dest}")))
    };
    set_log(ui, &format!("Restore {ts}\n"));
    run_stream(
        ui,
        state,
        vec![cmd],
        ssh::askpass_env(&target),
        pending,
        "Restoring…",
    );
}

/// Run a one-shot command in a thread and deliver its Result to a callback.
fn run_oneshot<F>(
    ui: &Rc<Ui>,
    state: &Shared,
    work: impl FnOnce() -> Result<String, String> + Send + 'static,
    done: F,
) where
    F: Fn(&Shared, &Rc<Ui>, Result<String, String>) + 'static,
{
    let (tx, rx) = async_channel::unbounded::<Result<String, String>>();
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    let ui = ui.clone();
    let state = state.clone();
    glib::spawn_future_local(async move {
        if let Ok(res) = rx.recv().await {
            done(&state, &ui, res);
        }
    });
}

fn test_connection(state: &Shared, ui: &Rc<Ui>) {
    let Some(f) = state.borrow().selected_target().cloned() else {
        set_status(ui, "No target selected");
        return;
    };
    let target = f.to_target();
    set_status(ui, "Testing connection…");
    run_oneshot(
        ui,
        state,
        move || verify_target(&target),
        |_st, ui, res| match res {
            Ok(msg) => {
                set_log(ui, &msg);
                set_status(ui, "Connection test done");
            }
            Err(e) => set_status(ui, &format!("Test failed: {e}")),
        },
    );
}

fn prune_now(state: &Shared, ui: &Rc<Ui>) {
    let Some(f) = state.borrow().selected_target().cloned() else {
        return;
    };
    let target = f.to_target();
    set_status(ui, "Pruning…");
    run_oneshot(
        ui,
        state,
        move || prune_target(&target),
        |st, ui, res| match res {
            Ok(msg) => {
                set_status(ui, &msg);
                log_entry(st, "prune", "", true, msg);
                refresh_history(st, ui);
            }
            Err(e) => set_status(ui, &format!("Prune failed: {e}")),
        },
    );
}

fn load_snapshots(state: &Shared, ui: &Rc<Ui>) {
    let Some(name) = state.borrow().restore_target.clone() else {
        return;
    };
    let Some(f) = state
        .borrow()
        .targets
        .iter()
        .find(|t| t.name == name)
        .cloned()
    else {
        return;
    };
    let target = f.to_target();
    set_status(ui, "Loading snapshots…");
    run_oneshot(
        ui,
        state,
        move || list_snapshots(&target),
        |st, ui, res| match res {
            Ok(joined) => {
                let snaps: Vec<String> = joined
                    .lines()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                st.borrow_mut().snapshots = snaps.clone();
                st.borrow_mut().selected_snapshot = None;
                st.borrow_mut().tree.clear();
                refresh_snapshots(st, ui);
                refresh_tree(st, ui);
                set_status(ui, &format!("{} snapshot(s)", snaps.len()));
            }
            Err(e) => set_status(ui, &format!("Error: {e}")),
        },
    );
}

fn load_tree(state: &Shared, ui: &Rc<Ui>) {
    let Some(name) = state.borrow().restore_target.clone() else {
        return;
    };
    let Some(si) = state.borrow().selected_snapshot else {
        return;
    };
    let ts = state.borrow().snapshots[si].clone();
    let Some(f) = state
        .borrow()
        .targets
        .iter()
        .find(|t| t.name == name)
        .cloned()
    else {
        return;
    };
    let target = f.to_target();
    set_status(ui, "Loading file tree…");
    run_oneshot(
        ui,
        state,
        move || list_tree(&target, &ts),
        |st, ui, res| match res {
            Ok(joined) => {
                let tree: Vec<TreeEntry> = joined
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| {
                        let is_dir = l.ends_with('/');
                        let path = l.trim_end_matches('/').to_string();
                        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                        TreeEntry { path, name, is_dir }
                    })
                    .collect();
                st.borrow_mut().tree = tree;
                refresh_tree(st, ui);
                set_status(ui, "Tree loaded");
            }
            Err(e) => set_status(ui, &format!("Error: {e}")),
        },
    );
}

// ─────────────────── sync backend helpers (run in worker threads) ───────────────────

fn ssh_probe(target: &Target, remote_cmd: &str) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args(ssh::probe_command_args(target, remote_cmd));
    cmd.envs(ssh::askpass_env(target));
    cmd
}

fn list_snapshots(target: &Target) -> Result<String, String> {
    let mut snaps = if target.backend.is_ssh() {
        let out = ssh_probe(target, &snapshot::list_cmd(target))
            .output()
            .map_err(|e| format!("could not start ssh: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l != "latest")
            .collect::<Vec<_>>()
    } else {
        rclone::list_snapshots(target).map_err(|e| format!("{e:#}"))?
    };
    snaps.sort();
    snaps.reverse();
    Ok(snaps.join("\n"))
}

fn list_tree(target: &Target, ts: &str) -> Result<String, String> {
    if target.backend.is_ssh() {
        let out = ssh_probe(target, &snapshot::tree_cmd(target, ts))
            .output()
            .map_err(|e| format!("could not start ssh: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let out = Command::new("rclone")
            .args(rclone::tree_args(target, ts))
            .output()
            .map_err(|e| format!("could not start rclone: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

fn verify_target(target: &Target) -> Result<String, String> {
    let mut out = String::new();
    if target.backend.is_ssh() {
        for src in &target.sources {
            let p = moraine::config::expand_tilde(src);
            out.push_str(&format!("{} source {}\n", check(p.exists()), p.display()));
        }
        let probe = ssh_probe(target, "echo ok")
            .output()
            .map_err(|e| format!("ssh: {e}"))?;
        out.push_str(&format!(
            "{} SSH connection\n",
            check(probe.status.success())
        ));
        if probe.status.success() {
            let dest = ssh_probe(target, &snapshot::dest_check_cmd(target))
                .output()
                .map_err(|e| format!("ssh: {e}"))?;
            let txt = String::from_utf8_lossy(&dest.stdout);
            let ok = matches!(txt.trim(), "writable" | "parent-writable");
            out.push_str(&format!("{} dest writable: {}\n", check(ok), target.dest));
        }
    } else {
        for src in &target.sources {
            let p = moraine::config::expand_tilde(src);
            out.push_str(&format!("{} source {}\n", check(p.exists()), p.display()));
        }
        let ok = Command::new("rclone")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        out.push_str(&format!("{} rclone available\n", check(ok)));
    }
    Ok(out)
}

fn check(ok: bool) -> &'static str {
    if ok {
        "✓"
    } else {
        "✗"
    }
}

fn prune_target(target: &Target) -> Result<String, String> {
    let Some(policy) = &target.retention else {
        return Ok("No retention policy — keeping all".to_string());
    };
    if policy.is_empty() {
        return Ok("No retention policy — keeping all".to_string());
    }
    let snaps: Vec<String> = list_snapshots(target)?
        .lines()
        .map(|s| s.to_string())
        .collect();
    let plan = prune::plan(&snaps, policy);
    if plan.delete.is_empty() {
        return Ok(format!("Nothing to prune ({} kept)", plan.keep.len()));
    }
    if target.backend.is_ssh() {
        let del = ssh_probe(target, &snapshot::prune_cmd(target, &plan.delete))
            .output()
            .map_err(|e| format!("ssh: {e}"))?;
        if !del.status.success() {
            return Err(String::from_utf8_lossy(&del.stderr).trim().to_string());
        }
    } else {
        for ts in &plan.delete {
            rclone::purge(target, ts).map_err(|e| format!("{e:#}"))?;
        }
    }
    Ok(format!(
        "Pruned {}, kept {}",
        plan.delete.len(),
        plan.keep.len()
    ))
}

// ─────────────────────────── scheduling (reused from CLI logic) ───────────────────────────

fn backup_cli_path() -> String {
    let exe_name = if cfg!(windows) {
        "moraine.exe"
    } else {
        "moraine"
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join(exe_name).display().to_string();
        }
    }
    exe_name.to_string()
}

fn scheduler_name() -> &'static str {
    if cfg!(windows) {
        "Task Scheduler"
    } else {
        "crontab"
    }
}

fn scheduled_config_path() -> String {
    std::fs::canonicalize(CONFIG_PATH)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| CONFIG_PATH.to_string())
}

fn install_schedules(schedules: &[Schedule]) -> Result<usize, String> {
    if cfg!(windows) {
        install_schtasks(schedules)
    } else {
        install_crontab(schedules)
    }
}

fn install_crontab(schedules: &[Schedule]) -> Result<usize, String> {
    const MARKER: &str = "# moraine";
    let existing = match Command::new("crontab").arg("-l").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    };
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.contains(MARKER))
        .map(|s| s.to_string())
        .collect();
    let exe = backup_cli_path();
    let cfg = scheduled_config_path();
    let mut count = 0;
    for s in schedules
        .iter()
        .filter(|s| s.enabled && !s.target.is_empty())
    {
        lines.push(format!(
            "{} {} -c {} run --target {} >/dev/null 2>&1 {MARKER}:{}",
            s.cron(),
            exe,
            cfg,
            s.target,
            s.name
        ));
        count += 1;
    }
    let body = format!("{}\n", lines.join("\n"));
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run crontab: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("no stdin for crontab")?
        .write_all(body.as_bytes())
        .map_err(|e| format!("could not write to crontab: {e}"))?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(count)
    } else {
        Err(format!(
            "crontab exited with {}",
            status.code().unwrap_or(-1)
        ))
    }
}

fn install_schtasks(schedules: &[Schedule]) -> Result<usize, String> {
    const FOLDER: &str = "Moraine";
    remove_moraine_tasks(FOLDER);
    let dir = schtasks_wrapper_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let exe = backup_cli_path();
    let cfg = scheduled_config_path();
    let mut count = 0;
    for s in schedules
        .iter()
        .filter(|s| s.enabled && !s.target.is_empty())
    {
        let safe = sanitize_task_name(&s.name);
        let wrapper = dir.join(format!("{safe}.cmd"));
        let body = format!(
            "@echo off\r\n\"{exe}\" -c \"{cfg}\" run --target {}\r\n",
            s.target
        );
        std::fs::write(&wrapper, body)
            .map_err(|e| format!("could not write {}: {e}", wrapper.display()))?;
        let tn = format!("{FOLDER}\\{safe}");
        let tr = format!("\"{}\"", wrapper.display());
        let mut cmd = Command::new("schtasks");
        cmd.args(["/Create", "/F", "/TN", tn.as_str(), "/TR", tr.as_str()]);
        schtasks_schedule_flags(&mut cmd, s);
        let out = cmd
            .output()
            .map_err(|e| format!("could not run schtasks: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "schtasks failed for '{}': {}",
                s.name,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        count += 1;
    }
    Ok(count)
}

fn schtasks_wrapper_dir() -> Result<std::path::PathBuf, String> {
    let base = std::env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    Ok(std::path::Path::new(&base).join("Moraine").join("tasks"))
}

fn schtasks_schedule_flags(cmd: &mut Command, s: &Schedule) {
    let time = format!("{:02}:{:02}", s.hour.min(23), s.minute.min(59));
    match s.frequency {
        Frequency::Hourly => {
            let st = format!("00:{:02}", s.minute.min(59));
            cmd.args(["/SC", "HOURLY", "/MO", "1", "/ST", st.as_str()]);
        }
        Frequency::Daily => {
            cmd.args(["/SC", "DAILY", "/ST", time.as_str()]);
        }
        Frequency::Weekly => {
            const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
            let d = DAYS[(s.weekday as usize).min(6)];
            cmd.args(["/SC", "WEEKLY", "/D", d, "/ST", time.as_str()]);
        }
    }
}

fn remove_moraine_tasks(folder: &str) {
    let Ok(out) = Command::new("schtasks")
        .args(["/Query", "/FO", "CSV", "/NH"])
        .output()
    else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let prefix = format!("\\{folder}\\");
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let rest = line.trim().trim_start_matches('"');
        let name = match rest.find('"') {
            Some(i) => &rest[..i],
            None => continue,
        };
        if name.starts_with(&prefix) {
            let _ = Command::new("schtasks")
                .args(["/Delete", "/F", "/TN", name])
                .output();
        }
    }
}

fn sanitize_task_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed.to_string()
    }
}
