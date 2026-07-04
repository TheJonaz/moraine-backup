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
use std::io::{BufReader, Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;

use moraine::config::{Backend, Config, Frequency, Retention, Schedule, Target};
use moraine::history::{self, LogEntry};
use moraine::{prune, rclone, rsync, snapshot, ssh};

const CONFIG_PATH: &str = "moraine.toml";
const APP_ID: &str = "io.thern.moraine";

/// Set when launched with `--minimized` (the autostart entry does this) so the
/// window starts iconified to the taskbar instead of popping up at login.
static START_MINIMIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    strict_host_key: bool,
    dest: String,
    sources: Vec<String>,
    exclude: Vec<String>,
    vpn: String,
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
            strict_host_key: t.strict_host_key,
            dest: t.dest.clone(),
            sources: t.sources.clone(),
            exclude: t.exclude.clone(),
            vpn: t.vpn.clone(),
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
            strict_host_key: self.strict_host_key,
            dest: self.dest.trim().to_string(),
            sources: clean(&self.sources),
            exclude: clean(&self.exclude),
            vpn: self.vpn.trim().to_string(),
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

/// NetworkManager connections that look like VPNs (type `vpn` or `wireguard`),
/// for the target VPN dropdown. Empty if nmcli is unavailable.
fn list_vpn_connections() -> Vec<String> {
    let out = match Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE", "connection", "show"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| {
            // `-t` output is `NAME:TYPE`; literal ':' inside NAME is escaped `\:`,
            // and TYPE never contains ':', so the last ':' is the field separator.
            let (name, kind) = line.rsplit_once(':')?;
            let kind = kind.to_ascii_lowercase();
            (kind.contains("vpn") || kind.contains("wireguard")).then(|| name.replace("\\:", ":"))
        })
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
    /// Snapshot-relative paths ticked for a selective restore. Tracked in state
    /// (not read back off the widgets) so a selection survives folder
    /// navigation and doesn't depend on which rows are currently visible.
    checked: std::collections::HashSet<String>,
    cwd: String,
    running: bool,
    /// True while widget models are being rebuilt — selection handlers that
    /// would otherwise fire network calls (e.g. loading snapshots over SSH at
    /// startup, before any user action) check this and stay quiet.
    refreshing_ui: bool,
    /// Set if the config file existed but failed to load/validate, so the UI
    /// can warn instead of silently showing an empty (and un-saveable) state.
    load_error: Option<String>,
}

impl State {
    fn load() -> State {
        let mut st = State::default();
        let path = Path::new(CONFIG_PATH);
        match Config::load(path) {
            Ok(cfg) => {
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
            // Only surface an error if the file actually exists — a missing
            // config on first run is normal, not an error.
            Err(e) if path.exists() => {
                st.load_error = Some(format!("Could not load {CONFIG_PATH}: {e:#}"));
            }
            Err(_) => {}
        }
        st.history = history::read(path);
        st
    }

    fn build_config(&self) -> Config {
        Config {
            targets: self.targets.iter().map(|f| f.to_target()).collect(),
            schedules: self.schedules.iter().map(|f| f.to_schedule()).collect(),
        }
    }

    /// Flags form fields that would otherwise be silently defaulted/clamped
    /// (a typo'd port would quietly become 22, a bad hour 0).
    fn check_forms(&self) -> Result<(), String> {
        for f in &self.targets {
            let p = f.port.trim();
            if !p.is_empty() && p.parse::<u16>().is_err() {
                return Err(format!("target '{}': '{p}' is not a valid port", f.label()));
            }
        }
        for s in &self.schedules {
            let h = s.hour.trim();
            let m = s.minute.trim();
            if h.parse::<u8>().map(|v| v > 23).unwrap_or(true) {
                return Err(format!("schedule '{}': invalid hour '{h}' (0–23)", s.name));
            }
            if m.parse::<u8>().map(|v| v > 59).unwrap_or(true) {
                return Err(format!(
                    "schedule '{}': invalid minute '{m}' (0–59)",
                    s.name
                ));
            }
        }
        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        self.check_forms()?;
        let cfg = self.build_config();
        // Never write a config the engine can't load back (traversal names,
        // bad key path, …): the GUI would then start empty on next launch.
        cfg.validate().map_err(|e| format!("{e:#}"))?;
        // Keep one rolling backup of the previous config. fs::copy preserves
        // the 0600 permission bits.
        let path = Path::new(CONFIG_PATH);
        if path.exists() {
            let _ = std::fs::copy(path, format!("{CONFIG_PATH}.bak"));
        }
        cfg.save(path).map_err(|e| format!("{e:#}"))
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
    // Log + progress + status
    log: gtk::TextView,
    progress: gtk::ProgressBar,
    progress_lbl: gtk::Label,
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
    Progress(f64, String), // fraction 0..1, "transferred · rate · ETA"
    Done(bool, String, Option<(String, String, String)>), // ok, detail, (op,target,info)
}

// ─────────────────────────── entry point ───────────────────────────

fn main() -> glib::ExitCode {
    // Consume our own flags before GTK sees argv — a plain GtkApplication rejects
    // command-line options it wasn't told to handle, so `--minimized` would abort
    // startup if passed through to `run()`.
    let minimized = std::env::args().any(|a| a == "--minimized" || a == "--minimised");
    START_MINIMIZED.store(minimized, std::sync::atomic::Ordering::Relaxed);

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        // Don't require the D-Bus session bus for single-instance handling.
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);
    // Pass only argv[0]: our flags are handled above and GTK needs none of them.
    let argv0 = std::env::args().next().unwrap_or_default();
    app.run_with_args(&[argv0])
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    // Inject the hero background image (navy + grid + glow), resolving its path
    // at runtime (installed vs source tree).
    // GTK's CSS url() needs a real URI — a bare absolute path silently fails to
    // load (with no warning), which left the grid background invisible.
    // Windows paths use '\', which is invalid in a URI — normalize to '/'.
    let hero = asset("hero-bg.png").replace('\\', "/");
    let hero_uri = format!("file://{hero}");
    let css = format!(
        "{CSS}\nwindow {{ background-image: url(\"{hero_uri}\"); background-size: cover; background-position: top center; }}\n"
    );
    provider.load_from_data(&css);
    if let Some(display) = gtk::gdk::Display::default() {
        // USER priority (above the desktop theme) so our field/button colours
        // win over themes that style widgets more aggressively.
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

const CSS: &str = r#"
window { background-color: #0b1a2c; color: #e8eef6; }
.card { background-color: rgba(20, 39, 63, 0.94); border: 1px solid #213a59; border-radius: 14px; padding: 14px; }
.appname { font-size: 26px; font-weight: 800; color: #eaf1fa; }
.appsub { color: #8aa0bd; font-size: 12px; }
.muted { color: #8aa0bd; font-size: 12px; }
.section { font-weight: 700; }
.accent { background-image: none; background-color: #0fd4a0; color: #06231b; font-weight: 700; border: 1px solid transparent; }
.accent:hover { background-color: #1fe3b3; }
.danger { color: #ff6b6b; }
/* Inputs + buttons: dark navy-blue (matches the screenshot), not the theme's
   light fields. This theme paints the field on the inner `text` node, so we
   override both the outer widget and its `text`/child nodes. */
entry, spinbutton, passwordentry, dropdown, dropdown > button, button {
    background-image: none;
    background-color: #16304f;
    color: #e8eef6;
    border: 1px solid #2c4f78;
    border-radius: 8px;
}
entry > text, spinbutton > text, passwordentry > text {
    background-image: none;
    background-color: #16304f;
    color: #e8eef6;
}
entry:focus, spinbutton:focus, passwordentry:focus,
entry:focus-within, passwordentry:focus-within,
entry:focus > text, passwordentry:focus-within > text,
dropdown > button:focus {
    border-color: #2e8be0;
}
entry image, spinbutton button { color: #cfe0f2; }
entry placeholder, entry > text placeholder { color: #7f96b4; }
button:hover { background-color: #1d3d61; }
button:disabled, entry:disabled, entry:disabled > text {
    color: #6b809b;
    background-color: #12263f;
}
selection { background-color: #2e8be0; color: #ffffff; }
/* Lists (Targets, schedules, snapshots) sit inside dark cards — keep them
   transparent instead of the theme's white. */
list, listbox, scrolledwindow, list > row, listbox > row {
    background-color: transparent;
    color: #e8eef6;
}
/* Pill tab bar (StackSwitcher styled like the old iced tabs). */
.tabs { padding: 2px 0; }
.tabs button {
    border-radius: 10px;
    padding: 7px 18px;
    margin-right: 6px;
    background: #14273f;
    border: 1px solid #213a59;
    color: #9fb3cc;
    font-weight: 700;
}
.tabs button:hover { background: #1b3552; }
.tabs button:checked {
    background: linear-gradient(to right, #2e8be0, #0fd4a0);
    color: #06231b;
    border-color: transparent;
}
row.selected-target { background-color: #0fd4a0; color: #06231b; border-radius: 8px; }
.crumb { color: #8aa0bd; font-family: monospace; font-size: 12px; }
textview, textview text { background-color: #0a1626; color: #cfe6dd; font-family: monospace; font-size: 12px; }
.statusbar { color: #8aa0bd; }
.linkbtn { background: none; border: none; color: #2e8be0; padding: 2px; }
progressbar trough { background-color: #0a1626; border-radius: 6px; min-height: 10px; }
progressbar progress { background-color: #0fd4a0; border-radius: 6px; }
"#;

fn build_ui(app: &gtk::Application) {
    let state: Shared = Rc::new(RefCell::new(State::load()));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Moraine Backup")
        .default_width(1040)
        .default_height(720)
        .build();

    // The Stack holds the four tab pages. The switcher that drives it is placed
    // in the content as a pill bar (not in the window titlebar), to match the
    // original layout.
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_vexpand(true);

    // Build the Ui struct (widgets shared with handlers).
    let ui = Rc::new(Ui {
        window: window.clone(),
        target_list: gtk::ListBox::new(),
        name: gtk::Entry::new(),
        backend: gtk::DropDown::from_strings(&["ssh", "rclone", "ftp"]),
        host: gtk::Entry::new(),
        port: gtk::Entry::new(),
        log: gtk::TextView::new(),
        progress: gtk::ProgressBar::new(),
        progress_lbl: gtk::Label::new(None),
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
    stack.add_titled(
        &build_settings_tab(&state, &ui),
        Some("settings"),
        "Settings",
    );

    // Opening the Restore tab auto-loads snapshots for the selected target
    // (only when none are loaded yet, so it doesn't clobber a live selection).
    {
        let st = state.clone();
        let ui2 = ui.clone();
        stack.connect_visible_child_name_notify(move |s| {
            if s.visible_child_name().as_deref() == Some("restore") {
                let need = st.borrow().snapshots.is_empty()
                    && st.borrow().restore_target.is_some()
                    && !st.borrow().running;
                if need {
                    load_snapshots(&st, &ui2);
                }
            }
        });
    }

    // In-content header: logo + name + subtitle.
    let logo = gtk::Image::from_file(asset("moraine-64.png"));
    logo.set_pixel_size(44);
    logo.set_valign(gtk::Align::Center);
    let titlecol = gtk::Box::new(gtk::Orientation::Vertical, 0);
    titlecol.set_valign(gtk::Align::Center);
    let appname = gtk::Label::new(Some("Moraine"));
    appname.add_css_class("appname");
    appname.set_halign(gtk::Align::Start);
    let appsub = gtk::Label::new(Some(&format!(
        "Snapshot backups over SSH & rclone · v{}",
        moraine::VERSION
    )));
    appsub.add_css_class("appsub");
    appsub.set_halign(gtk::Align::Start);
    titlecol.append(&appname);
    titlecol.append(&appsub);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    header.append(&logo);
    header.append(&titlecol);

    // Pill tab bar (drives the stack).
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.add_css_class("tabs");
    switcher.set_halign(gtk::Align::Start);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(14);
    root.set_margin_bottom(14);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.append(&header);
    root.append(&switcher);
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

    // Warn (don't silently start empty) if an existing config failed to load.
    if let Some(err) = state.borrow().load_error.clone() {
        set_status(
            &ui,
            &format!("⚠ {err} — the previous config is at {CONFIG_PATH}.bak"),
        );
    }

    window.present();

    // Launched from the autostart entry (`--minimized`): iconify to the taskbar
    // rather than grabbing focus at login. Must run after present() — minimizing
    // an unmapped window is a no-op on most compositors.
    if START_MINIMIZED.load(std::sync::atomic::Ordering::Relaxed) {
        window.minimize();
    }
}

fn asset(name: &str) -> String {
    // 1) $XDG_DATA_DIRS/moraine/assets — the portable path that works for a
    //    distro install (/usr/share), Flatpak (/app/share), Snap ($SNAP/... via
    //    XDG_DATA_DIRS), AppImage (AppRun exports it) and Nix (wrapGAppsHook).
    if let Some(dirs) = std::env::var_os("XDG_DATA_DIRS") {
        for d in std::env::split_paths(&dirs) {
            let p = d.join("moraine/assets").join(name);
            if p.exists() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    // 2) Common install prefixes, then the source tree (dev).
    for base in [
        "/app/share/moraine/assets",
        "/usr/share/moraine/assets",
        "assets",
    ] {
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

    // Log (+ live progress)
    let log_card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    log_card.add_css_class("card");
    log_card.set_vexpand(true);

    // Progress row (hidden until a run streams progress).
    ui.progress_lbl.add_css_class("muted");
    ui.progress_lbl.set_halign(gtk::Align::Start);
    ui.progress_lbl.set_visible(false);
    ui.progress.set_visible(false);
    log_card.append(&ui.progress_lbl);
    log_card.append(&ui.progress);

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

    // Strict host key (SSH)
    let strict =
        gtk::CheckButton::with_label("Require known SSH host key (strict — no trust-on-first-use)");
    strict.set_active(f.strict_host_key);
    body.append(&strict);
    {
        let st = state.clone();
        strict.connect_toggled(move |c| {
            st.borrow_mut().targets[i].strict_host_key = c.is_active();
        });
    }

    // Destination
    let dest = gtk::Entry::new();
    dest.set_text(&f.dest);
    body.append(&labeled("Destination on target", &dest));
    {
        let st = state.clone();
        dest.connect_changed(move |e| st.borrow_mut().targets[i].dest = e.text().to_string());
    }

    // VPN — pick one of the machine's NetworkManager connections to bring up
    // before the backup (and down after). Covers any VPN configured in the DE.
    {
        let mut items: Vec<String> = vec!["None (no VPN)".to_string()];
        let mut conns = list_vpn_connections();
        // Keep a saved value even if nmcli is missing or the VPN was renamed.
        if !f.vpn.is_empty() && !conns.contains(&f.vpn) {
            conns.insert(0, f.vpn.clone());
        }
        items.extend(conns);
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        let vpn_dd = gtk::DropDown::from_strings(&refs);
        vpn_dd.set_selected(if f.vpn.is_empty() {
            0
        } else {
            items.iter().position(|s| *s == f.vpn).unwrap_or(0) as u32
        });
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let vl = gtk::Label::new(Some(
            "VPN (NetworkManager connection, up before / down after)",
        ));
        vl.add_css_class("muted");
        vl.set_halign(gtk::Align::Start);
        vbox.append(&vl);
        vbox.append(&vpn_dd);
        body.append(&vbox);
        let st = state.clone();
        vpn_dd.connect_selected_notify(move |dd| {
            let idx = dd.selected() as usize;
            st.borrow_mut().targets[i].vpn = if idx == 0 {
                String::new()
            } else {
                items.get(idx).cloned().unwrap_or_default()
            };
        });
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
/// A rebuild closure, stored in a cell so the row's ✕ buttons can call it.
type RebuildCell = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Adds one or more picked paths to a target's sources: the first replaces the
/// row `j` that launched the picker, the rest are appended as new rows. Used by
/// both the multi-file and multi-folder pickers.
fn add_picked_sources(st: &Shared, i: usize, j: usize, list: &gio::ListModel) {
    let mut paths: Vec<String> = Vec::new();
    for k in 0..list.n_items() {
        if let Some(file) = list.item(k).and_downcast::<gio::File>() {
            if let Some(p) = file.path() {
                paths.push(p.display().to_string());
            }
        }
    }
    if paths.is_empty() {
        return;
    }
    let mut s = st.borrow_mut();
    let sources = &mut s.targets[i].sources;
    let mut it = paths.into_iter();
    if let Some(first) = it.next() {
        if j < sources.len() {
            sources[j] = first;
        } else {
            sources.push(first);
        }
    }
    sources.extend(it);
}

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

    // `rebuild` needs to call itself (the ✕ buttons re-render the list after
    // removing a row), so store it in a cell and hand the row handlers a Weak
    // ref back to it. No strong cycle: the closure only holds a Weak.
    let rebuild_cell: RebuildCell = Rc::new(RefCell::new(None));
    let rebuild: Rc<dyn Fn()> = {
        let state = state.clone();
        let listbox = listbox.clone();
        let win = win.clone();
        let cell_weak = Rc::downgrade(&rebuild_cell);
        Rc::new(move || {
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
                    // One "Browse…" button holding both pickers. Each is
                    // multi-select, so you can grab many files (or many folders)
                    // in one sweep — GTK can't select files AND folders in the
                    // same dialog, so they're two menu entries under one button.
                    let browse = gtk::MenuButton::new();
                    browse.set_label("Browse…");
                    let pop = gtk::Popover::new();
                    let pbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    pbox.set_margin_top(4);
                    pbox.set_margin_bottom(4);
                    pbox.set_margin_start(4);
                    pbox.set_margin_end(4);
                    let files_item = gtk::Button::with_label("Files…");
                    files_item.add_css_class("flat");
                    let folders_item = gtk::Button::with_label("Folders…");
                    folders_item.add_css_class("flat");
                    pbox.append(&files_item);
                    pbox.append(&folders_item);
                    pop.set_child(Some(&pbox));
                    browse.set_popover(Some(&pop));

                    {
                        let st = state.clone();
                        let win2 = win.clone();
                        let cw = cell_weak.clone();
                        let pop2 = pop.clone();
                        files_item.connect_clicked(move |_| {
                            pop2.popdown();
                            let dialog = gtk::FileDialog::builder().title("Select files").build();
                            let st2 = st.clone();
                            let cw2 = cw.clone();
                            dialog.open_multiple(Some(&win2), gio::Cancellable::NONE, move |res| {
                                if let Ok(list) = res {
                                    add_picked_sources(&st2, i, j, &list);
                                    if let Some(f) = cw2.upgrade().and_then(|c| c.borrow().clone())
                                    {
                                        f();
                                    }
                                }
                            });
                        });
                    }
                    {
                        let st = state.clone();
                        let win2 = win.clone();
                        let cw = cell_weak.clone();
                        let pop2 = pop.clone();
                        folders_item.connect_clicked(move |_| {
                            pop2.popdown();
                            let dialog = gtk::FileDialog::builder().title("Select folders").build();
                            let st2 = st.clone();
                            let cw2 = cw.clone();
                            dialog.select_multiple_folders(
                                Some(&win2),
                                gio::Cancellable::NONE,
                                move |res| {
                                    if let Ok(list) = res {
                                        add_picked_sources(&st2, i, j, &list);
                                        if let Some(f) =
                                            cw2.upgrade().and_then(|c| c.borrow().clone())
                                        {
                                            f();
                                        }
                                    }
                                },
                            );
                        });
                    }
                    row.append(&browse);
                }
                let rm = gtk::Button::with_label("✕");
                {
                    let st = state.clone();
                    let cw = cell_weak.clone();
                    rm.connect_clicked(move |_| {
                        {
                            let mut s = st.borrow_mut();
                            let v = if is_sources {
                                &mut s.targets[i].sources
                            } else {
                                &mut s.targets[i].exclude
                            };
                            if j < v.len() {
                                v.remove(j);
                            }
                        }
                        // Re-render so the remaining rows get correct indices.
                        if let Some(f) = cw.upgrade().and_then(|c| c.borrow().clone()) {
                            f();
                        }
                    });
                }
                row.append(&rm);
                listbox.append(&row);
            }
        })
    };
    *rebuild_cell.borrow_mut() = Some(rebuild.clone());

    let add = gtk::Button::with_label("+ Add");
    {
        let state = state.clone();
        // Hold a strong ref to the cell so it (and the closure) outlives this
        // function, for as long as the modal's buttons exist.
        let cell = rebuild_cell.clone();
        add.connect_clicked(move |_| {
            {
                let mut s = state.borrow_mut();
                if is_sources {
                    s.targets[i].sources.push(String::new());
                } else {
                    s.targets[i].exclude.push(String::new());
                }
            }
            if let Some(f) = cell.borrow().clone() {
                f();
            }
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
            // Compute the number BEFORE borrow_mut: the receiver of push() is
            // evaluated first, so an inner borrow() here would panic.
            let n = st.borrow().schedules.len() + 1;
            st.borrow_mut().schedules.push(ScheduleForm {
                name: format!("schedule-{n}"),
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
            if let Err(e) = st.borrow().save() {
                set_status(&ui2, &e);
                return;
            }
            // Only install schedules whose target actually exists — a stale
            // one (target renamed/deleted) would just fail silently in cron.
            let names: Vec<String> = st
                .borrow()
                .targets
                .iter()
                .map(|t| t.name.trim().to_string())
                .collect();
            let all: Vec<Schedule> = st
                .borrow()
                .schedules
                .iter()
                .map(|f| f.to_schedule())
                .collect();
            let (scheds, skipped): (Vec<Schedule>, Vec<Schedule>) = all
                .into_iter()
                .partition(|s| names.iter().any(|n| n == s.target.trim()));
            match install_schedules(&scheds) {
                Ok(n) => {
                    let mut msg = format!("Installed {n} schedule(s) to {}", scheduler_name());
                    if !skipped.is_empty() {
                        msg.push_str(&format!(
                            " — skipped {} with a missing target",
                            skipped.len()
                        ));
                    }
                    set_status(&ui2, &msg);
                }
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
            let refreshing = st.borrow().refreshing_ui;
            if let Some(n) = names.get(d.selected() as usize) {
                st.borrow_mut().restore_target = Some(n.clone());
                // Only a real user selection triggers a network call — not a
                // programmatic model rebuild (startup, import, save).
                if !refreshing {
                    // Re-default the destination to the new target's source
                    // location (load_snapshots fills it when empty).
                    ui2.restore_dest.set_text("");
                    load_snapshots(&st, &ui2);
                }
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
    // set_model/set_selected fire selected_notify; the flag keeps the handler
    // from probing the server (SSH) without any user action.
    state.borrow_mut().refreshing_ui = true;
    ui.restore_target.set_model(Some(&model));
    if !names.is_empty() {
        ui.restore_target.set_selected(0);
        state.borrow_mut().restore_target = Some(names[0].clone());
    }
    state.borrow_mut().refreshing_ui = false;
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
            let mut s = st.borrow_mut();
            s.selected_snapshot = Some(i);
            s.cwd = String::new();
            s.checked.clear(); // a new snapshot starts with nothing selected
            drop(s);
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
    let checked = state.borrow().checked.clone();
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
        check.set_active(checked.contains(&e.path));
        {
            // Toggling updates the tracked set directly.
            let st = state.clone();
            let path = e.path.clone();
            check.connect_toggled(move |c| {
                let mut s = st.borrow_mut();
                if c.is_active() {
                    s.checked.insert(path.clone());
                } else {
                    s.checked.remove(&path);
                }
            });
        }
        hb.append(&check);
        let icon = if e.is_dir { "📁" } else { "📄" };
        let lbl = gtk::Label::new(Some(&format!("{icon} {}", e.name)));
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);
        hb.append(&lbl);
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

// ─────────────────────────── Settings tab ───────────────────────────

fn build_settings_tab(state: &Shared, ui: &Rc<Ui>) -> gtk::Widget {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);

    // ── Configuration ──
    let cfg = gtk::Box::new(gtk::Orientation::Vertical, 8);
    cfg.add_css_class("card");
    let ct = gtk::Label::new(Some("Configuration"));
    ct.add_css_class("section");
    ct.set_halign(gtk::Align::Start);
    cfg.append(&ct);

    let path = std::fs::canonicalize(CONFIG_PATH)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| CONFIG_PATH.to_string());
    let path_lbl = gtk::Label::new(Some(&format!("Config file: {path}")));
    path_lbl.add_css_class("muted");
    path_lbl.set_halign(gtk::Align::Start);
    path_lbl.set_selectable(true);
    path_lbl.set_wrap(true);
    cfg.append(&path_lbl);

    let sub = gtk::Label::new(Some("Encrypted config backup"));
    sub.add_css_class("section");
    sub.set_halign(gtk::Align::Start);
    sub.set_margin_top(6);
    cfg.append(&sub);
    let desc = gtk::Label::new(Some(
        "The config holds secrets (SSH keys, passwords, key passphrases). Export it as \
         an encrypted, password-protected file to move it between machines or keep a \
         safe backup. Import replaces the current config.",
    ));
    desc.add_css_class("muted");
    desc.set_halign(gtk::Align::Start);
    desc.set_wrap(true);
    cfg.append(&desc);

    let btns = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let export_btn = gtk::Button::with_label("Export config…");
    export_btn.add_css_class("accent");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        export_btn.connect_clicked(move |_| {
            // Persist current edits first; if they're invalid, export the
            // on-disk config anyway but tell the user it isn't the latest.
            if let Err(e) = st.borrow().save() {
                set_status(&ui2, &format!("Exporting last saved config — {e}"));
            }
            let ui3 = ui2.clone();
            let win = ui2.window.clone();
            ask_password(
                &ui2.window,
                "Export config",
                "Set a password to encrypt the exported config. You'll need it to import it again.",
                true,
                move |pw| {
                    let dialog = gtk::FileDialog::builder()
                        .title("Export encrypted config")
                        .initial_name("moraine-config.toml.gpg")
                        .build();
                    let ui4 = ui3.clone();
                    dialog.save(Some(&win), gio::Cancellable::NONE, move |res| {
                        let Ok(file) = res else { return };
                        let Some(path) = file.path() else { return };
                        match export_config(&pw, &path) {
                            Ok(()) => set_status(
                                &ui4,
                                &format!("Config exported (encrypted) → {}", path.display()),
                            ),
                            Err(e) => set_status(&ui4, &format!("Export failed: {e}")),
                        }
                    });
                },
            );
        });
    }
    btns.append(&export_btn);
    let import_btn = gtk::Button::with_label("Import config…");
    {
        let st = state.clone();
        let ui2 = ui.clone();
        import_btn.connect_clicked(move |_| {
            // Default to encrypted-config files so the right file is picked
            // (importing a plaintext file gives a confusing gpg error).
            let gpg_filter = gtk::FileFilter::new();
            gpg_filter.set_name(Some("Encrypted config (*.gpg)"));
            gpg_filter.add_pattern("*.gpg");
            let all_filter = gtk::FileFilter::new();
            all_filter.set_name(Some("All files"));
            all_filter.add_pattern("*");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&gpg_filter);
            filters.append(&all_filter);
            let dialog = gtk::FileDialog::builder()
                .title("Import encrypted config")
                .filters(&filters)
                .default_filter(&gpg_filter)
                .build();
            let st2 = st.clone();
            let ui3 = ui2.clone();
            let win = ui2.window.clone();
            dialog.open(Some(&ui2.window), gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                let Some(path) = file.path() else { return };
                let st3 = st2.clone();
                let ui4 = ui3.clone();
                ask_password(
                    &win,
                    "Import config",
                    "Enter the password for the encrypted config.",
                    false,
                    move |pw| match import_config(&pw, &path) {
                        Ok(()) => {
                            reload_all(&st3, &ui4);
                            set_status(&ui4, "Config imported");
                        }
                        Err(e) => set_status(&ui4, &format!("Import failed: {e}")),
                    },
                );
            });
        });
    }
    btns.append(&import_btn);
    cfg.append(&btns);
    outer.append(&cfg);

    // ── Startup ── (desktop-autostart entries are a freedesktop/Linux thing)
    if !cfg!(windows) {
        build_startup_card(ui, &outer);
    }

    // ── About ──
    build_about_and_finish(state, ui, &outer, scroll)
}

fn build_startup_card(ui: &Rc<Ui>, outer: &gtk::Box) {
    let startup = gtk::Box::new(gtk::Orientation::Vertical, 8);
    startup.add_css_class("card");
    let st_title = gtk::Label::new(Some("Startup"));
    st_title.add_css_class("section");
    st_title.set_halign(gtk::Align::Start);
    startup.append(&st_title);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let toggle = gtk::Switch::new();
    toggle.set_active(autostart_enabled());
    toggle.set_valign(gtk::Align::Center);
    let tl = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let tl_main = gtk::Label::new(Some("Start Moraine when I log in"));
    tl_main.set_halign(gtk::Align::Start);
    let tl_sub = gtk::Label::new(Some(
        "Adds a desktop autostart entry so the app launches automatically at login, \
         minimized to the taskbar.",
    ));
    tl_sub.add_css_class("muted");
    tl_sub.set_halign(gtk::Align::Start);
    tl_sub.set_wrap(true);
    tl.append(&tl_main);
    tl.append(&tl_sub);
    tl.set_hexpand(true);
    row.append(&tl);
    row.append(&toggle);
    startup.append(&row);
    {
        let ui2 = ui.clone();
        toggle.connect_state_set(move |sw, want| {
            match set_autostart(want) {
                Ok(()) => set_status(
                    &ui2,
                    if want {
                        "Autostart enabled — Moraine will start at login"
                    } else {
                        "Autostart disabled"
                    },
                ),
                Err(e) => {
                    set_status(&ui2, &format!("Could not change autostart: {e}"));
                    // Revert the visual state so it reflects reality.
                    sw.set_state(!want);
                    return glib::Propagation::Stop;
                }
            }
            sw.set_state(want);
            glib::Propagation::Stop
        });
    }
    outer.append(&startup);
}

fn build_about_and_finish(
    state: &Shared,
    ui: &Rc<Ui>,
    outer: &gtk::Box,
    scroll: gtk::ScrolledWindow,
) -> gtk::Widget {
    let _ = (state, ui); // parity with the other tab builders
    let about = gtk::Box::new(gtk::Orientation::Vertical, 6);
    about.add_css_class("card");
    let at = gtk::Label::new(Some("About"));
    at.add_css_class("section");
    at.set_halign(gtk::Align::Start);
    about.append(&at);
    let ver = gtk::Label::new(Some(&format!("Moraine {}", moraine::VERSION)));
    ver.add_css_class("muted");
    ver.set_halign(gtk::Align::Start);
    about.append(&ver);
    let links = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for (label, url) in [
        ("GitHub", "https://github.com/TheJonaz/moraine-backup"),
        ("moraine.thern.io", "https://moraine.thern.io"),
        ("Website", "https://www.thern.io"),
    ] {
        let btn = gtk::Button::with_label(label);
        btn.add_css_class("linkbtn");
        btn.connect_clicked(move |_| {
            let _ = gio::AppInfo::launch_default_for_uri(url, gio::AppLaunchContext::NONE);
        });
        links.append(&btn);
    }
    about.append(&links);
    outer.append(&about);

    scroll.set_child(Some(outer));
    scroll.upcast()
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
    // Cap the buffer: a huge file list must not grow it unbounded (which makes
    // every insert/scroll O(n) and can freeze the main loop). Keep recent lines.
    const MAX_LINES: i32 = 4000;
    let lines = buf.line_count();
    if lines > MAX_LINES {
        let mut start = buf.start_iter();
        if let Some(mut cut) = buf.iter_at_line(lines - MAX_LINES) {
            buf.delete(&mut start, &mut cut);
        }
    }
    let mut end = buf.end_iter();
    buf.insert(&mut end, line);
    buf.insert(&mut end, "\n");
    // Autoscroll via the buffer's built-in insert mark — do NOT create a new
    // mark each call (that leaks and slows down over a long run).
    buf.place_cursor(&buf.end_iter());
    ui.log.scroll_mark_onscreen(&buf.get_insert());
}

fn set_log(ui: &Rc<Ui>, text: &str) {
    ui.log.buffer().set_text(text);
}

/// The full current log text (for post-mortem diagnosis).
fn log_text(ui: &Rc<Ui>) -> String {
    let buf = ui.log.buffer();
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string()
}

/// Recognises common backup/restore failures and returns a clear explanation
/// plus how to fix it, ready to append to the log.
fn diagnose_failure(out: &str) -> Option<String> {
    let l = out.to_lowercase();
    let hint = |problem: &str, fix: &str| {
        Some(format!(
            "──────────── WHAT WENT WRONG ────────────\nProblem: {problem}\nFix:     {fix}"
        ))
    };

    // rsync/rclone missing (remote or local).
    if l.contains("rsync: command not found")
        || (l.contains("rsync") && l.contains("command not found"))
    {
        return hint(
            "rsync is not installed on the destination server.",
            "Log in to the server and install it — e.g.  apt install rsync  (or your \
             server's package manager). rsync must be present on BOTH this computer and \
             the server. Alternatively, switch this target to the rclone backend (SFTP), \
             which needs nothing installed on the server.",
        );
    }
    if l.contains("could not start rsync") || l.contains("rsync — is it installed") {
        return hint(
            "rsync is not installed on this computer.",
            "Install it:  sudo apt install rsync",
        );
    }
    if l.contains("could not start rclone") || l.contains("rclone: command not found") {
        return hint(
            "rclone is not installed on this computer.",
            "Install it:  sudo apt install rclone  (or from rclone.org).",
        );
    }

    // SSH problems.
    if l.contains("permission denied (publickey") || l.contains("authentication failed") {
        return hint(
            "SSH authentication was rejected by the server.",
            "Check the SSH key path and that the key is authorized on the server. If the \
             key has a passphrase, set it under the target's ⚙ Settings.",
        );
    }
    if l.contains("host key verification failed") {
        return hint(
            "The server's SSH host key is unknown or has changed.",
            "If this change is expected, remove the old line from ~/.ssh/known_hosts and \
             retry; otherwise verify you are connecting to the right host.",
        );
    }
    if l.contains("connection timed out")
        || l.contains("connection refused")
        || l.contains("could not resolve")
        || l.contains("no route to host")
        || l.contains("network is unreachable")
    {
        return hint(
            "Could not reach the server.",
            "Check the host/IP and port, that the server is online, and (if it's on a \
             private network) that your VPN is connected.",
        );
    }

    // Destination / sources.
    if l.contains("permission denied")
        && (l.contains("mkpath") || l.contains("mkdir") || l.contains("failed to"))
    {
        return hint(
            "The destination directory is not writable by the login user.",
            "Check the destination path in the target's ⚙ Settings and that the SSH user \
             may write there.",
        );
    }
    if l.contains("no such file or directory") && l.contains("rsync") {
        return hint(
            "A source path does not exist on this computer.",
            "Check the Sources list in the target's ⚙ Settings.",
        );
    }
    if l.contains("opendir") && l.contains("permission denied") {
        return hint(
            "A source folder can't be read (permission denied), so it was skipped \
             and --delete was disabled for safety.",
            "Fix that folder's permissions (chown/chmod), or add it to the target's \
             Exclude patterns. The '--link-dest ../latest does not exist' line is \
             normal on the first run.",
        );
    }

    // Generic rsync transport failure (often remote-side).
    if l.contains("connection unexpectedly closed") || l.contains("protocol data stream") {
        return hint(
            "The SSH connection closed unexpectedly during transfer.",
            "This usually means rsync is missing on the server, or the remote shell \
             printed unexpected output. Confirm rsync is installed on the server.",
        );
    }
    None
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
            let saved = st.borrow().save();
            refresh_targets(&st, &ui2);
            refresh_connection(&st, &ui2);
            match saved {
                Ok(()) => set_status(&ui2, "Target deleted"),
                Err(e) => set_status(
                    &ui2,
                    &format!("Target removed from the list, but couldn't save: {e}"),
                ),
            }
        }
    });
}

/// Turns one progress line — rsync `--info=progress2` or rclone
/// `--stats-one-line` — into (fraction 0..1, display text). None otherwise.
fn parse_progress(line: &str) -> Option<(f64, String)> {
    parse_rclone_progress(line).or_else(|| parse_rsync_progress(line))
}

/// rsync `--info=progress2`: "  512.00K  48%  2.34MB/s  0:00:03".
fn parse_rsync_progress(line: &str) -> Option<(f64, String)> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let pct_idx = toks
        .iter()
        .position(|t| t.ends_with('%') && t.trim_end_matches('%').parse::<f64>().is_ok())?;
    let pct: f64 = toks[pct_idx]
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())?;
    let transferred = toks
        .get(pct_idx.wrapping_sub(1))
        .filter(|_| pct_idx > 0)
        .filter(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .copied()
        .unwrap_or("");
    let rate = toks
        .iter()
        .find(|t| t.ends_with("/s"))
        .copied()
        .unwrap_or("");
    let eta = toks.iter().find(|t| is_time(t)).copied().unwrap_or("");
    Some((pct / 100.0, progress_text(transferred, pct, rate, eta)))
}

/// rclone `-v --stats-one-line`, e.g.
/// "2026/... INFO  :  20.09 MiB / 286.10 MiB, 7%, 12 MiB/s, ETA 1m2s".
/// The " / " (done/total) distinguishes it from rsync's progress2 line.
fn parse_rclone_progress(line: &str) -> Option<(f64, String)> {
    if !(line.contains('%') && line.contains(" / ") && line.contains("/s")) {
        return None;
    }
    let segs: Vec<&str> = line.split(',').map(str::trim).collect();
    let pct_seg = segs
        .iter()
        .find(|s| s.ends_with('%') && s.trim_end_matches('%').parse::<f64>().is_ok())?;
    let pct: f64 = pct_seg
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())?;
    // done/total is the segment with " / "; drop any "timestamp INFO :" prefix
    // by keeping only what's after the last colon.
    let transferred = segs
        .iter()
        .find(|s| s.contains(" / "))
        .map(|s| s.rsplit(':').next().unwrap_or(s).trim())
        .unwrap_or("");
    let rate = segs
        .iter()
        .find(|s| s.contains("/s"))
        .copied()
        .unwrap_or("");
    let eta = segs
        .iter()
        .find(|s| s.contains("ETA"))
        .and_then(|s| s.rsplit("ETA").next()?.split_whitespace().next())
        .unwrap_or("");
    Some((pct / 100.0, progress_text(transferred, pct, rate, eta)))
}

fn progress_text(transferred: &str, pct: f64, rate: &str, eta: &str) -> String {
    let mut text = String::new();
    if !transferred.is_empty() {
        text.push_str(&format!("{transferred} · "));
    }
    text.push_str(&format!("{}%", pct as i64));
    if !rate.is_empty() {
        text.push_str(&format!(" · {rate}"));
    }
    if !eta.is_empty() {
        text.push_str(&format!(" · ETA {eta}"));
    }
    text
}

/// True for time tokens like "0:03", "1:23:45".
fn is_time(t: &str) -> bool {
    let parts: Vec<&str> = t.split(':').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Reads a child stream in a thread, splitting on `\n` and `\r`, sending each
/// line as a progress update or a log line. `collect` gathers non-progress
/// lines (used to build the stderr error detail).
fn pump<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: async_channel::Sender<Worker>,
    collect: Option<std::sync::Arc<std::sync::Mutex<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut byte = [0u8; 1];
        let mut buf: Vec<u8> = Vec::new();
        while let Ok(1) = reader.read(&mut byte) {
            if byte[0] == b'\n' || byte[0] == b'\r' {
                let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                buf.clear();
                if line.is_empty() {
                    continue;
                }
                match parse_progress(&line) {
                    Some((frac, text)) => {
                        let _ = tx.send_blocking(Worker::Progress(frac, text));
                    }
                    None => {
                        if let Some(c) = &collect {
                            if let Ok(mut s) = c.lock() {
                                s.push_str(&line);
                                s.push('\n');
                            }
                        }
                        let _ = tx.send_blocking(Worker::Line(line));
                    }
                }
            } else {
                buf.push(byte[0]);
            }
        }
    })
}

/// A closure that builds the command list inside the worker thread, *after*
/// the VPN is up — for backends whose commands depend on a network lookup that
/// itself needs the VPN (e.g. rclone's previous-snapshot check for --copy-dest).
type Prepare = Box<dyn FnOnce() -> Vec<(String, Vec<String>)> + Send>;

// Internal orchestration helper; the parameters are each distinct run inputs
// and bundling them into a struct would only add indirection.
#[allow(clippy::too_many_arguments)]
fn run_stream(
    ui: &Rc<Ui>,
    state: &Shared,
    cmds: Vec<(String, Vec<String>)>,
    env: Vec<(String, String)>,
    pending: Option<(String, String, String)>,
    start_msg: &str,
    vpn: String,
    prepare: Option<Prepare>,
) {
    state.borrow_mut().running = true;
    set_running(ui, true);
    set_status(ui, start_msg);
    ui.progress.set_fraction(0.0);
    ui.progress.set_visible(false);
    ui.progress_lbl.set_visible(false);

    // Bounded so a very chatty (or malicious) child can't grow memory without
    // limit: when full, the pump thread's send_blocking parks, the child's pipe
    // fills, and it throttles naturally. The main loop drains continuously.
    let (tx, rx) = async_channel::bounded::<Worker>(8192);
    std::thread::spawn(move || {
        // Bring the chosen VPN up first; if it fails, abort before touching
        // data. If the VPN is already connected (user brought it up manually),
        // leave it alone — and leave it up afterwards.
        let mut vpn_was_ours = false;
        if !vpn.is_empty() {
            if moraine::vpn::is_active(&vpn) {
                let _ = tx.send_blocking(Worker::Line(format!(
                    "VPN \"{vpn}\" already connected — leaving it up afterwards"
                )));
            } else {
                let _ = tx.send_blocking(Worker::Line(format!("$ nmcli connection up {vpn}")));
                match moraine::vpn::up(&vpn) {
                    Ok(()) => {
                        vpn_was_ours = true;
                        let _ = tx.send_blocking(Worker::Line(format!("VPN \"{vpn}\" connected")));
                    }
                    Err(e) => {
                        let _ = tx.send_blocking(Worker::Done(
                            false,
                            format!("{e:#}"),
                            pending.clone(),
                        ));
                        return;
                    }
                }
            }
        }

        // Build the commands now that the VPN is up (rclone --copy-dest lookup).
        let cmds = match prepare {
            Some(build) => build(),
            None => cmds,
        };

        let mut failed: Option<String> = None;
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
                    failed = Some(format!("could not start {prog}: {e}"));
                    break;
                }
            };
            // Read stdout and stderr concurrently so progress streams live from
            // either (rsync writes progress to stdout, rclone to stderr).
            let err_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let h_out = child.stdout.take().map(|o| pump(o, tx.clone(), None));
            let h_err = child
                .stderr
                .take()
                .map(|e| pump(e, tx.clone(), Some(err_buf.clone())));
            if let Some(h) = h_out {
                let _ = h.join();
            }
            if let Some(h) = h_err {
                let _ = h.join();
            }
            let status = child.wait();
            let code = status.as_ref().ok().and_then(|s| s.code());
            let ok = status.map(|s| s.success()).unwrap_or(false);
            // rsync 23/24 = partial transfer (some files skipped/vanished) but a
            // valid snapshot — keep going so `latest` is still repointed.
            let partial = prog == "rsync" && matches!(code, Some(23) | Some(24));
            if !ok && !partial {
                failed = Some(err_buf.lock().map(|s| s.clone()).unwrap_or_default());
                break;
            }
            if partial {
                let _ = tx.send_blocking(Worker::Line(format!(
                    "⚠ rsync partial transfer (exit {}) — some files were skipped; \
                     snapshot still created.",
                    code.unwrap_or(-1)
                )));
            }
        }

        // Tear the VPN down afterwards (best effort) — but only if WE brought
        // it up; a pre-existing connection stays.
        if vpn_was_ours {
            let _ = tx.send_blocking(Worker::Line(format!("$ nmcli connection down {vpn}")));
            moraine::vpn::down(&vpn);
        }

        match failed {
            Some(detail) => {
                let _ = tx.send_blocking(Worker::Done(false, detail, pending));
            }
            None => {
                let _ = tx.send_blocking(Worker::Done(true, String::new(), pending));
            }
        }
    });

    let ui = ui.clone();
    let state = state.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            match msg {
                Worker::Line(l) => append_log(&ui, &l),
                Worker::Progress(frac, text) => {
                    ui.progress.set_visible(true);
                    ui.progress_lbl.set_visible(true);
                    ui.progress.set_fraction(frac.clamp(0.0, 1.0));
                    ui.progress_lbl.set_text(&text);
                }
                Worker::Done(ok, detail, pending) => {
                    state.borrow_mut().running = false;
                    set_running(&ui, false);
                    ui.progress.set_visible(false);
                    ui.progress_lbl.set_visible(false);
                    if ok {
                        set_status(&ui, "Done");
                        if let Some((op, target, info)) = pending {
                            log_entry(&state, &op, &target, true, info);
                        }
                    } else {
                        append_log(&ui, &detail);
                        // Scan the whole run output for a known problem and
                        // print a clear explanation + fix.
                        let full = format!("{}\n{detail}", log_text(&ui));
                        if let Some(hint) = diagnose_failure(&full) {
                            append_log(&ui, "");
                            append_log(&ui, &hint);
                        }
                        set_status(&ui, "Failed — see the log for details");
                        // Record the failure in history too (first line only —
                        // the full detail lives in the log).
                        if let Some((op, target, _)) = pending {
                            let short = detail.lines().next().unwrap_or("failed").to_string();
                            log_entry(&state, &op, &target, false, short);
                        }
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
    if target.dest.trim().is_empty() {
        set_status(ui, "Target needs a destination (dest)");
        return;
    }
    if target.sources.is_empty() {
        set_status(ui, "Target needs at least one source");
        return;
    }
    // Surface form errors (bad port etc.) instead of running with defaults.
    if let Err(e) = state.borrow().save() {
        set_status(ui, &e);
        return;
    }
    let ts = snapshot::timestamp();
    let pending = if dry_run {
        None
    } else {
        Some((
            "backup".to_string(),
            target.name.clone(),
            format!("snapshot {ts}"),
        ))
    };
    let msg = if dry_run {
        format!("Dry run against {}…", target.name)
    } else {
        format!("Backing up {}…", target.name)
    };

    if target.backend.is_ssh() {
        let dest = snapshot::snapshot_dir(&target, &ts);
        let mut args = rsync::build_args(&target, &dest, Some(rsync::LINK_DEST), dry_run);
        add_rsync_progress(&mut args);
        let mut cmds = vec![("rsync".to_string(), args)];
        if !dry_run {
            let latest = snapshot::update_latest_cmd(&target, &ts);
            cmds.push((
                "ssh".to_string(),
                ssh::remote_command_args(&target, &latest),
            ));
        }
        set_log(ui, &format!("snapshot {ts}\n"));
        run_stream(
            ui,
            state,
            cmds,
            ssh::askpass_env(&target),
            pending,
            &msg,
            target.vpn.clone(),
            None,
        );
    } else {
        if let Err(e) = rclone::preflight(&target) {
            set_status(ui, &format!("{e:#}"));
            return;
        }
        set_log(ui, &format!("snapshot {ts}\n"));
        // The --copy-dest lookup is a network call that must run *after* the
        // VPN is up (some rclone remotes are only reachable over it), so build
        // the commands inside the worker via `prepare` rather than up front.
        let env = rclone::env_for(&target);
        let t = target.clone();
        let prepare: Prepare = Box::new(move || {
            let prev = rclone::list_snapshots(&t)
                .unwrap_or_default()
                .into_iter()
                .filter(|s| snapshot::is_timestamp(s))
                .max()
                .filter(|_| rclone::supports_server_side_copy(&t));
            let mut cmds = rclone::backup_cmds(&t, &ts, prev.as_deref(), dry_run);
            for (prog, args) in &mut cmds {
                add_rclone_progress(prog, args);
            }
            cmds
        });
        run_stream(
            ui,
            state,
            Vec::new(),
            env,
            pending,
            &msg,
            target.vpn.clone(),
            Some(prepare),
        );
    }
}

/// Adds live aggregate progress to rsync. Deliberately NOT `-v` for real runs:
/// on a large source, listing every file floods the log. --info=progress2
/// gives one updating progress line (parsed by parse_progress); --stats (already
/// in build_args) prints the final summary. Dry runs KEEP the file listing —
/// seeing what would transfer is their whole point (the log buffer is capped).
fn add_rsync_progress(args: &mut Vec<String>) {
    let dry = args.iter().any(|a| a == "--dry-run");
    if !dry {
        // Drop the per-file listing (added by the shared lib for the CLI).
        args.retain(|a| a != "-v" && a != "--verbose");
    }
    if !args.iter().any(|a| a.starts_with("--info")) {
        args.insert(0, "--info=progress2".to_string());
    }
}

/// Adds live one-line stats to an rclone command (no-op for others). Uses
/// --stats-log-level NOTICE (not `-v`) so the summary shows without logging
/// every transferred file — which would flood the log like `-v` does.
fn add_rclone_progress(prog: &str, args: &mut Vec<String>) {
    if prog == "rclone" && !args.iter().any(|a| a.starts_with("--stats")) {
        // Drop -v (per-file logging) that the shared lib adds for the CLI.
        args.retain(|a| a != "-v" && a != "--verbose");
        let at = args.len().min(1); // after the subcommand (e.g. "copy")
        args.insert(at, "--stats-log-level".to_string());
        args.insert(at + 1, "NOTICE".to_string());
        args.insert(at, "--stats-one-line".to_string());
        args.insert(at, "--stats=1s".to_string());
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
    let Some(ts) = s.snapshots.get(si).cloned() else {
        drop(s);
        set_status(ui, "Pick a snapshot");
        return;
    };
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
    // Only paths that still exist in the loaded tree (a selection could linger
    // from a snapshot with a different layout).
    let selected: Vec<String> = {
        let s = state.borrow();
        let present: std::collections::HashSet<&str> =
            s.tree.iter().map(|e| e.path.as_str()).collect();
        s.checked
            .iter()
            .filter(|p| present.contains(p.as_str()))
            .cloned()
            .collect()
    };
    let mut cmd = if target.backend.is_ssh() {
        let mut args = if selected.is_empty() {
            rsync::restore_args(&target, &ts, &dest, dry_run)
        } else {
            rsync::restore_selected_args(&target, &ts, &selected, &dest, dry_run)
        };
        add_rsync_progress(&mut args);
        ("rsync".to_string(), args)
    } else {
        if let Err(e) = rclone::preflight(&target) {
            set_status(ui, &format!("{e:#}"));
            return;
        }
        (
            "rclone".to_string(),
            rclone::restore_args(&target, &ts, &selected, &dest, dry_run),
        )
    };
    add_rclone_progress(&cmd.0, &mut cmd.1);
    let pending = if dry_run {
        None
    } else {
        Some(("restore".to_string(), name, format!("{ts} → {dest}")))
    };
    set_log(ui, &format!("Restore {ts}\n"));
    // askpass env is empty for rclone targets and rclone env for ssh targets,
    // so combining both is always correct for whichever backend runs.
    let mut env = ssh::askpass_env(&target);
    env.extend(rclone::env_for(&target));
    run_stream(
        ui,
        state,
        vec![cmd],
        env,
        pending,
        "Restoring…",
        target.vpn.clone(),
        None,
    );
}

/// Run a one-shot job in a thread and deliver its result to a callback on the
/// main loop. Generic over the result type so callers can return whatever they
/// need (e.g. `Result<String, String>` or `(bool, String)`).
fn run_oneshot<T, F>(
    ui: &Rc<Ui>,
    state: &Shared,
    work: impl FnOnce() -> T + Send + 'static,
    done: F,
) where
    T: Send + 'static,
    F: Fn(&Shared, &Rc<Ui>, T) + 'static,
{
    let (tx, rx) = async_channel::unbounded::<T>();
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
    if state.borrow().running {
        set_status(ui, "Busy — wait for the current run to finish");
        return;
    }
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
            Ok((ok, msg)) => {
                // The message's RESULT line distinguishes a connection failure
                // from a merely-missing source, so keep the status neutral.
                set_log(ui, &msg);
                set_status(
                    ui,
                    if ok {
                        "All checks passed ✓"
                    } else {
                        "Some checks did not pass — see the log"
                    },
                );
            }
            Err(e) => set_status(ui, &format!("Test error: {e}")),
        },
    );
}

fn prune_now(state: &Shared, ui: &Rc<Ui>) {
    if state.borrow().running {
        set_status(ui, "Busy — wait for the current run to finish");
        return;
    }
    let Some(f) = state.borrow().selected_target().cloned() else {
        return;
    };
    let target = f.to_target();
    let name = target.name.clone();
    set_status(ui, "Pruning…");
    run_oneshot(
        ui,
        state,
        move || prune_target(&target),
        move |st, ui, res| match res {
            Ok(msg) => {
                set_status(ui, &msg);
                log_entry(st, "prune", &name, true, msg);
                refresh_history(st, ui);
            }
            Err(e) => set_status(ui, &format!("Prune failed: {e}")),
        },
    );
}

/// The directory a restore should default to: the common parent of the
/// target's sources, so restoring recreates the original paths (the snapshot
/// stores each source under its base name). `None` if the sources live in
/// different parents (no single destination reconstructs them all).
fn source_parent_default(f: &TargetForm) -> Option<String> {
    let mut parent: Option<std::path::PathBuf> = None;
    for s in &f.sources {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let par = moraine::config::expand_tilde(s).parent()?.to_path_buf();
        match &parent {
            None => parent = Some(par),
            Some(p) if *p == par => {}
            Some(_) => return None,
        }
    }
    parent.map(|p| p.display().to_string())
}

fn load_snapshots(state: &Shared, ui: &Rc<Ui>) {
    if state.borrow().running {
        return;
    }
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
    // Default the restore destination to where the sources live, so a restore
    // reconstructs the original paths. Only when the field is empty, so a
    // manual edit (or a value set for another target) is preserved.
    if ui.restore_dest.text().trim().is_empty() {
        if let Some(d) = source_parent_default(&f) {
            ui.restore_dest.set_text(&d);
        }
    }
    let target = f.to_target();
    set_status(ui, "Loading snapshots…");
    let name2 = name.clone();
    run_oneshot(
        ui,
        state,
        move || list_snapshots(&target),
        move |st, ui, res| match res {
            Ok(joined) => {
                // Ignore a stale result: the user may have switched targets
                // while this listing was in flight (jobs can overlap).
                if st.borrow().restore_target.as_deref() != Some(name2.as_str()) {
                    return;
                }
                let snaps: Vec<String> = joined
                    .lines()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                st.borrow_mut().snapshots = snaps.clone();
                st.borrow_mut().selected_snapshot = None;
                st.borrow_mut().tree.clear();
                st.borrow_mut().checked.clear();
                // Remember the count so the Targets list can show it.
                st.borrow_mut().counts.insert(name2.clone(), snaps.len());
                refresh_snapshots(st, ui);
                refresh_tree(st, ui);
                refresh_targets(st, ui);
                set_status(ui, &format!("{} snapshot(s)", snaps.len()));
            }
            Err(e) => set_status(ui, &format!("Error: {e}")),
        },
    );
}

fn load_tree(state: &Shared, ui: &Rc<Ui>) {
    if state.borrow().running {
        return;
    }
    let Some(name) = state.borrow().restore_target.clone() else {
        return;
    };
    let Some(si) = state.borrow().selected_snapshot else {
        return;
    };
    let Some(ts) = state.borrow().snapshots.get(si).cloned() else {
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
    let name_c = name.clone();
    let ts_c = ts.clone();
    set_status(ui, "Loading file tree…");
    run_oneshot(
        ui,
        state,
        move || list_tree(&target, &ts),
        move |st, ui, res| match res {
            Ok(joined) => {
                // Ignore a stale result: the selection may have changed while
                // this listing was in flight.
                let name_ok = st.borrow().restore_target.as_deref() == Some(name_c.as_str());
                let ts_ok = {
                    let s = st.borrow();
                    s.selected_snapshot
                        .and_then(|i| s.snapshots.get(i))
                        .map(String::as_str)
                        == Some(ts_c.as_str())
                };
                if !(name_ok && ts_ok) {
                    return;
                }
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

/// Lists a snapshot's contents, normalized to one entry per line where
/// directories end with `/` (rclone `lsf` format, which `load_tree` parses).
/// The SSH backend's `find -printf '%y\t%P'` output is converted here.
/// Entries are server-supplied: anything absolute or containing `..` is
/// dropped so a malicious server can't traverse outside the restore dir.
fn list_tree(target: &Target, ts: &str) -> Result<String, String> {
    let raw = if target.backend.is_ssh() {
        let out = ssh_probe(target, &snapshot::tree_cmd(target, ts))
            .output()
            .map_err(|e| format!("could not start ssh: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        // `<type>\t<relative path>` → lsf-style (`dir/` with trailing slash).
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let (ty, path) = l.split_once('\t')?;
                Some(match ty {
                    "d" => format!("{path}/"),
                    _ => path.to_string(),
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let out = Command::new("rclone")
            .args(rclone::tree_args(target, ts))
            .envs(rclone::env_for(target))
            .output()
            .map_err(|e| format!("could not start rclone: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    Ok(raw
        .lines()
        .filter(|l| {
            !l.starts_with('/')
                && !Path::new(l.trim_end_matches('/'))
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Runs the connection checks. Returns (overall_ok, human-readable report).
fn verify_target(target: &Target) -> Result<(bool, String), String> {
    let mut out = format!("Testing target \"{}\"\n\n", target.name);
    // Track a missing *source* separately from a *connection* problem: a source
    // that doesn't exist locally shouldn't read as "connection FAILED".
    let mut missing_sources = 0;
    let mut conn_ok = true;

    // Local sources exist?
    for src in &target.sources {
        let p = moraine::config::expand_tilde(src);
        let ok = p.exists();
        if !ok {
            missing_sources += 1;
        }
        out.push_str(&format!(
            "{} source {}{}\n",
            mark(ok),
            p.display(),
            if ok {
                ""
            } else {
                " — does not exist on this computer"
            }
        ));
    }

    if target.backend.is_ssh() {
        let probe = ssh_probe(target, "echo ok")
            .output()
            .map_err(|e| format!("could not start ssh: {e}"))?;
        let cok = probe.status.success();
        conn_ok &= cok;
        if cok {
            out.push_str(&format!(
                "{} SSH connection to {}\n",
                mark(true),
                target.host
            ));
            let dest = ssh_probe(target, &snapshot::dest_check_cmd(target))
                .output()
                .map_err(|e| format!("could not start ssh: {e}"))?;
            let dok = matches!(
                String::from_utf8_lossy(&dest.stdout).trim(),
                "writable" | "parent-writable"
            );
            conn_ok &= dok;
            out.push_str(&format!(
                "{} destination writable: {}\n",
                mark(dok),
                target.dest
            ));
        } else {
            // Show why it failed (first stderr line: timeout, auth, host key, ...).
            let err = String::from_utf8_lossy(&probe.stderr);
            let reason = err.lines().next().unwrap_or("connection failed").trim();
            out.push_str(&format!(
                "{} SSH connection to {} — {}\n",
                mark(false),
                target.host,
                reason
            ));
        }
    } else {
        let ok = Command::new("rclone")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        conn_ok &= ok;
        out.push_str(&format!("{} rclone available\n", mark(ok)));
    }

    let ok_all = conn_ok && missing_sources == 0;
    out.push('\n');
    if !conn_ok {
        out.push_str("==> RESULT: connection FAILED (see the lines marked [FAIL] above)");
    } else if missing_sources > 0 {
        out.push_str(&format!(
            "==> RESULT: connection OK — but {missing_sources} source(s) are missing. \
             Fix the paths in the target's ⚙ Settings (or create the folders)."
        ));
    } else {
        out.push_str("==> RESULT: all checks passed ✓");
    }
    out.push('\n');
    Ok((ok_all, out))
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "[OK]  "
    } else {
        "[FAIL]"
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

// ─────────────────────── encrypted config export / import (gpg) ───────────────────────

/// Runs gpg with the passphrase fed on stdin (never on the command line).
fn gpg_with_passphrase(args: &[&str], passphrase: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new("gpg")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start gpg — is gnupg installed? ({e})"))?;
    child
        .stdin
        .take()
        .ok_or("no stdin for gpg")?
        .write_all(format!("{passphrase}\n").as_bytes())
        .map_err(|e| format!("gpg stdin: {e}"))?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        // Return the full stderr so callers can translate known failures.
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

/// Turns raw gpg stderr into a short, actionable message. gpg's own last line
/// for these is cryptic (e.g. "decrypt_message failed: Unknown system error"
/// when the file simply isn't encrypted).
fn friendly_gpg_error(stderr: &str) -> String {
    let s = stderr.to_lowercase();
    if s.contains("no valid openpgp data") {
        "the selected file is not an encrypted Moraine config — pick the \
         .gpg file you created with Export config"
            .to_string()
    } else if s.contains("bad session key") || s.contains("gcry_kdf_derive") {
        "wrong password".to_string()
    } else if s.contains("invalid packet") || s.contains("premature eof") {
        "the file looks corrupted or incomplete".to_string()
    } else {
        stderr
            .lines()
            .last()
            .unwrap_or("gpg failed")
            .trim()
            .trim_start_matches("gpg: ")
            .to_string()
    }
}

/// Path to the desktop-autostart entry that launches the GUI at login.
fn autostart_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join(".config/autostart/moraine-gui.desktop"))
}

/// True if the autostart entry currently exists.
fn autostart_enabled() -> bool {
    autostart_path().map(|p| p.exists()).unwrap_or(false)
}

/// Create or remove the autostart entry. On enable, `Exec` points at the running
/// binary so it works for both an installed copy and a locally-built one.
fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let path = autostart_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set"))?;
    if enabled {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let exec = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "moraine-gui".to_string());
        // Pin the working directory: the config path is cwd-relative, so
        // without Path= a login-started instance would look for (and create)
        // a different moraine.toml in $HOME.
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .map(|d| format!("Path={d}\n"))
            .unwrap_or_default();
        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Moraine\n\
             Comment=Snapshot backup over SSH/rsync and rclone\n\
             Exec={exec} --minimized\n\
             {cwd}\
             Icon=moraine\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
        );
        std::fs::write(&path, entry)?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Symmetrically encrypts the current config to `dest` (AES-256, password-based).
/// gpg writes to stdout and we save via write_private, so the export is 0600
/// (gpg's own --output would create it with the default umask).
fn export_config(passphrase: &str, dest: &Path) -> Result<(), String> {
    let encrypted = gpg_with_passphrase(
        &[
            "--batch",
            "--yes",
            "--pinentry-mode",
            "loopback",
            "--passphrase-fd",
            "0",
            "--symmetric",
            "--cipher-algo",
            "AES256",
            "--output",
            "-",
            CONFIG_PATH,
        ],
        passphrase,
    )
    .map_err(|e| friendly_gpg_error(&e))?;
    moraine::config::write_private(dest, &encrypted)
        .map_err(|e| format!("could not write {}: {e}", dest.display()))
}

/// Decrypts `src`, validates it as a config, and replaces the current config.
fn import_config(passphrase: &str, src: &Path) -> Result<(), String> {
    let src = src.display().to_string();
    let plaintext = gpg_with_passphrase(
        &[
            "--batch",
            "--yes",
            "--pinentry-mode",
            "loopback",
            "--passphrase-fd",
            "0",
            "--decrypt",
            &src,
        ],
        passphrase,
    )
    .map_err(|e| friendly_gpg_error(&e))?;
    let text = String::from_utf8_lossy(&plaintext);
    // Refuse to overwrite unless it parses AND validates as a Moraine config
    // (validate() rejects e.g. traversal characters in target names).
    let cfg = toml::from_str::<Config>(&text).map_err(|e| format!("not a valid config: {e}"))?;
    cfg.validate()
        .map_err(|e| format!("invalid config: {e:#}"))?;
    // Owner-only: the config holds plaintext secrets.
    moraine::config::write_private(Path::new(CONFIG_PATH), text.as_bytes())
        .map_err(|e| format!("could not write {CONFIG_PATH}: {e}"))?;
    Ok(())
}

/// Reloads all state from disk (after an import) and refreshes every view.
fn reload_all(state: &Shared, ui: &Rc<Ui>) {
    *state.borrow_mut() = State::load();
    refresh_targets(state, ui);
    refresh_connection(state, ui);
    refresh_schedules(state, ui);
    refresh_restore_targets(state, ui);
    refresh_history(state, ui);
}

/// A small modal that asks for a password (optionally with confirmation) and
/// calls `on_ok` with it. The password is never logged or shown.
fn ask_password(
    parent: &gtk::ApplicationWindow,
    title: &str,
    prompt: &str,
    confirm: bool,
    on_ok: impl Fn(String) + 'static,
) {
    let win = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title(title)
        .default_width(400)
        .build();
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    b.set_margin_top(16);
    b.set_margin_bottom(16);
    b.set_margin_start(16);
    b.set_margin_end(16);
    let lbl = gtk::Label::new(Some(prompt));
    lbl.add_css_class("muted");
    lbl.set_halign(gtk::Align::Start);
    lbl.set_wrap(true);
    b.append(&lbl);
    let pw = gtk::PasswordEntry::new();
    pw.set_show_peek_icon(true);
    b.append(&pw);
    let pw2 = if confirm {
        let l2 = gtk::Label::new(Some("Confirm password"));
        l2.add_css_class("muted");
        l2.set_halign(gtk::Align::Start);
        b.append(&l2);
        let e = gtk::PasswordEntry::new();
        e.set_show_peek_icon(true);
        b.append(&e);
        Some(e)
    } else {
        None
    };
    let err = gtk::Label::new(None);
    err.add_css_class("danger");
    err.set_halign(gtk::Align::Start);
    b.append(&err);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);
    let cancel = gtk::Button::with_label("Cancel");
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    row.append(&cancel);
    let ok = gtk::Button::with_label("OK");
    ok.add_css_class("accent");
    {
        let win = win.clone();
        let pw = pw.clone();
        let err = err.clone();
        ok.connect_clicked(move |_| {
            let p = pw.text().to_string();
            if p.is_empty() {
                err.set_text("Enter a password.");
                return;
            }
            if let Some(pw2) = &pw2 {
                if p != *pw2.text() {
                    err.set_text("Passwords don't match.");
                    return;
                }
            }
            win.close();
            on_ok(p);
        });
    }
    row.append(&ok);
    b.append(&row);
    win.set_child(Some(&b));
    win.present();
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

/// Rejects control characters (newlines especially) in a scheduled field, so a
/// crafted target/name can't break out of the crontab line / .cmd wrapper and
/// inject extra commands.
fn check_schedule_field(kind: &str, name: &str, v: &str) -> Result<(), String> {
    if v.chars().any(|c| c.is_control()) {
        return Err(format!(
            "schedule '{name}': {kind} contains a control character — refusing to install"
        ));
    }
    Ok(())
}

fn install_crontab(schedules: &[Schedule]) -> Result<usize, String> {
    // Include the colon so we only ever remove our own generated lines
    // (`… # moraine:<name>`), never a user's unrelated `# moraine` comment.
    const MARKER: &str = "# moraine:";
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
        check_schedule_field("target", &s.name, &s.target)?;
        check_schedule_field("name", &s.name, &s.name)?;
        // Shell-quote every interpolated value so spaces/metacharacters in the
        // path or target can't be interpreted by the shell that runs the job.
        lines.push(format!(
            "{} {} -c {} run --target {} >/dev/null 2>&1 {MARKER}{}",
            s.cron(),
            snapshot::shell_quote(&exe),
            snapshot::shell_quote(&cfg),
            snapshot::shell_quote(&s.target),
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
        check_schedule_field("target", &s.name, &s.target)?;
        if s.target.contains(['"', '%']) {
            return Err(format!(
                "schedule '{}': target contains a character not allowed on Windows (\" or %)",
                s.name
            ));
        }
        let safe = sanitize_task_name(&s.name);
        let wrapper = dir.join(format!("{safe}.cmd"));
        let body = format!(
            "@echo off\r\n\"{exe}\" -c \"{cfg}\" run --target \"{}\"\r\n",
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
