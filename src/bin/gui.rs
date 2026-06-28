//! moraine-gui — desktop client (iced) on top of the moraine engine.
//!
//! Two tabs:
//!  * Quick Backup — edit targets and run dry-run/backup directly.
//!  * Schedule — create multiple schedules and install them in crontab.
//!
//! The look is defined by a small design system (see the `style` section):
//! a palette that follows the system's light/dark mode, cards, an accent
//! color, rounded corners and consistent spacing/typography.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use moraine::config::{Backend, Config, Frequency, Retention, Schedule, Target};
use moraine::history::{self, LogEntry};
use moraine::{prune, rclone, rsync, snapshot, ssh};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Column, Row,
    Space,
};
use iced::{Background, Border, Color, Element, Length, Shadow, Task, Theme, Vector};

const CONFIG_PATH: &str = "moraine.toml";
const AUTHOR: &str = "by Jonaz Thern";
const AUTHOR_URL: &str = "https://www.thern.io";
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn main() -> iced::Result {
    iced::application("Moraine Backup", App::update, App::view)
        .theme(|_| system_theme())
        .run_with(App::new)
}

// ───────────────────────────── theme ─────────────────────────────

fn system_theme() -> Theme {
    // Forced dark theme (the hero background is dark navy). Restore
    // system-follow by uncommenting the block below.
    Theme::Dark
    // if let Some(dark) = gsettings_prefers_dark() {
    //     return if dark { Theme::Dark } else { Theme::Light };
    // }
    // match dark_light::detect() {
    //     dark_light::Mode::Light => Theme::Light,
    //     _ => Theme::Dark,
    // }
}

#[allow(dead_code)]
fn gsettings_prefers_dark() -> Option<bool> {
    if let Some(v) = gsettings_get("color-scheme") {
        if v.contains("dark") {
            return Some(true);
        }
        if v.contains("light") {
            return Some(false);
        }
    }
    gsettings_get("gtk-theme").map(|v| v.contains("dark"))
}

#[allow(dead_code)]
fn gsettings_get(key: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_lowercase())
}

// ────────────────────────── design system ──────────────────────────

/// Color palette for a tab (light or dark).
struct Pal {
    bg: Color,
    surface: Color,
    elevated: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    accent2: Color, // blue (thern.io --blue-2) — the other end of the accent gradient
    accent_hover: Color,
    on_accent: Color,
    selected: Color,
    danger: Color,
}

/// Linear gradient between two colors (~120°, like thern.io).
fn linear(start: Color, end: Color) -> Background {
    let g = iced::gradient::Linear::new(iced::Radians(2.1))
        .add_stop(0.0, start)
        .add_stop(1.0, end);
    Background::Gradient(iced::Gradient::Linear(g))
}

/// Blue→teal accent gradient (thern.io "SÅ FUNKAR DET" section).
fn accent_gradient(p: &Pal) -> Background {
    linear(p.accent2, p.accent)
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

// Palette matched to thern.io: teal accent (#0fd4a0) on navy/off-white.
fn pal(theme: &Theme) -> Pal {
    let accent = rgb(15, 212, 160); // thern.io --accent
    if theme.extended_palette().is_dark {
        Pal {
            bg: rgb(10, 22, 38),        // fallback (the hero image covers it)
            surface: rgb(45, 66, 96),   // much lighter slate-navy card
            elevated: rgb(56, 80, 112), // input, lighter still
            border: rgb(78, 104, 142),  // clear border
            text: rgb(238, 243, 249),
            muted: rgb(176, 190, 208),
            accent,                      // teal #0fd4a0
            accent2: rgb(46, 139, 224),  // blue --blue-2
            accent_hover: rgb(46, 224, 179),
            on_accent: rgb(255, 255, 255), // white text on the blue→teal gradient
            selected: with_alpha(accent, 0.18),
            danger: rgb(226, 58, 58), // --danger
        }
    } else {
        Pal {
            bg: rgb(244, 246, 249),       // off-white background
            surface: rgb(255, 255, 255),  // white cards (slightly lighter than bg)
            elevated: rgb(255, 255, 255), // white text fields
            border: rgb(214, 222, 233),   // light border (defines white fields)
            text: rgb(20, 26, 33),        // black text
            muted: rgb(108, 120, 137),
            accent,
            accent2: rgb(46, 139, 224),
            accent_hover: rgb(13, 190, 143),
            on_accent: rgb(255, 255, 255), // white text on the blue→teal gradient
            selected: with_alpha(accent, 0.13),
            danger: rgb(214, 55, 55),
        }
    }
}

fn semibold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::DEFAULT
    }
}

fn rounded(radius: f32, width: f32, color: Color) -> Border {
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

fn soft_shadow() -> Shadow {
    Shadow {
        color: with_alpha(Color::BLACK, 0.18),
        offset: Vector::new(0.0, 2.0),
        blur_radius: 16.0,
    }
}

#[allow(dead_code)]
fn window_style(theme: &Theme) -> container::Style {
    let p = pal(theme);
    // Subtle vertical gradient (slightly lighter at the top → p.bg at the
    // bottom) for depth without flat black.
    let top = if theme.extended_palette().is_dark {
        rgb(33, 46, 65)
    } else {
        rgb(252, 253, 255)
    };
    let g = iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
        .add_stop(0.0, top)
        .add_stop(1.0, p.bg);
    container::Style {
        text_color: Some(p.text),
        background: Some(Background::Gradient(iced::Gradient::Linear(g))),
        ..Default::default()
    }
}

/// Dimmed background behind modals.
fn scrim_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            r: 0.02,
            g: 0.05,
            b: 0.09,
            a: 0.6,
        })),
        ..Default::default()
    }
}

fn card_style(theme: &Theme) -> container::Style {
    let p = pal(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.surface)),
        border: rounded(16.0, 1.0, p.border),
        shadow: soft_shadow(),
    }
}

fn panel_style(theme: &Theme) -> container::Style {
    let p = pal(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.surface)),
        border: rounded(16.0, 1.0, p.border),
        ..Default::default()
    }
}

fn segmented_style(theme: &Theme) -> container::Style {
    let p = pal(theme);
    container::Style {
        background: Some(Background::Color(p.surface)),
        border: rounded(12.0, 1.0, p.border),
        ..Default::default()
    }
}

fn input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = pal(theme);
    let focused = matches!(status, text_input::Status::Focused);
    text_input::Style {
        background: Background::Color(p.elevated),
        border: rounded(
            10.0,
            if focused { 1.5 } else { 1.0 },
            if focused { p.accent } else { p.border },
        ),
        icon: p.muted,
        placeholder: p.muted,
        value: p.text,
        selection: with_alpha(p.accent, 0.35),
    }
}

fn picklist_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let p = pal(theme);
    let active = matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened);
    pick_list::Style {
        text_color: p.text,
        placeholder_color: p.muted,
        handle_color: p.muted,
        background: Background::Color(p.elevated),
        border: rounded(10.0, 1.0, if active { p.accent } else { p.border }),
    }
}

fn checkbox_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = pal(theme);
    let checked = match status {
        checkbox::Status::Active { is_checked }
        | checkbox::Status::Hovered { is_checked }
        | checkbox::Status::Disabled { is_checked } => is_checked,
    };
    checkbox::Style {
        background: Background::Color(if checked { p.accent } else { p.elevated }),
        icon_color: p.on_accent,
        border: rounded(6.0, 1.0, if checked { p.accent } else { p.border }),
        text_color: Some(p.text),
    }
}

fn primary_btn(theme: &Theme, status: button::Status) -> button::Style {
    let p = pal(theme);
    let background = match status {
        button::Status::Disabled => Background::Color(with_alpha(p.accent, 0.4)),
        button::Status::Hovered | button::Status::Pressed => linear(p.accent2, p.accent_hover),
        _ => accent_gradient(&p),
    };
    button::Style {
        background: Some(background),
        text_color: p.on_accent,
        border: rounded(10.0, 0.0, Color::TRANSPARENT),
        shadow: soft_shadow(),
    }
}

fn ghost_btn(theme: &Theme, status: button::Status) -> button::Style {
    let p = pal(theme);
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => Some(Background::Color(p.elevated)),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: p.text,
        border: rounded(10.0, 1.0, p.border),
        shadow: Shadow::default(),
    }
}

fn danger_btn(theme: &Theme, status: button::Status) -> button::Style {
    let p = pal(theme);
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Background::Color(with_alpha(p.danger, 0.12)))
        }
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: p.danger,
        border: rounded(10.0, 1.0, with_alpha(p.danger, 0.5)),
        shadow: Shadow::default(),
    }
}

fn link_btn(theme: &Theme, status: button::Status) -> button::Style {
    let p = pal(theme);
    let color = match status {
        button::Status::Hovered | button::Status::Pressed => p.accent,
        _ => p.muted,
    };
    button::Style {
        background: None,
        text_color: color,
        border: rounded(0.0, 0.0, Color::TRANSPARENT),
        shadow: Shadow::default(),
    }
}

/// Style for a row in a side list (selected or not).
fn list_item_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = pal(theme);
        let bg = if selected {
            Some(Background::Color(p.selected))
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(Background::Color(p.elevated))
                }
                _ => None,
            }
        };
        button::Style {
            background: bg,
            text_color: if selected { p.text } else { p.muted },
            border: rounded(10.0, 0.0, Color::TRANSPARENT),
            shadow: Shadow::default(),
        }
    }
}

/// Style for a tab pill (active or not).
fn tab_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let p = pal(theme);
    let bg = if active {
        Some(accent_gradient(&p))
    } else {
        match status {
            button::Status::Hovered => Some(Background::Color(p.elevated)),
            _ => None,
        }
    };
    button::Style {
        background: bg,
        text_color: if active { p.on_accent } else { p.muted },
        border: rounded(9.0, 0.0, Color::TRANSPARENT),
        shadow: Shadow::default(),
    }
}

/// Small muted label text.
fn muted_text<'a>(s: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(s)
        .size(12)
        .style(|theme: &Theme| text::Style {
            color: Some(pal(theme).muted),
        })
}

// ───────────────────────────── models ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    QuickBackup,
    Schedule,
    Restore,
    History,
}

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

impl Default for ScheduleForm {
    fn default() -> Self {
        ScheduleForm {
            name: "new-schedule".to_string(),
            target: String::new(),
            frequency: Frequency::Daily,
            hour: "2".to_string(),
            minute: "0".to_string(),
            weekday: 1,
            enabled: true,
        }
    }
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

    fn label(&self) -> String {
        if self.name.trim().is_empty() {
            "(unnamed schedule)".to_string()
        } else {
            self.name.clone()
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

/// An entry in a snapshot's file tree.
#[derive(Clone)]
struct TreeEntry {
    path: String,
    name: String,
    is_dir: bool,
    checked: bool,
}

struct App {
    tab: Tab,
    targets: Vec<TargetForm>,
    selected: Option<usize>,
    // Target index awaiting delete confirmation in the list.
    confirm_delete: Option<usize>,
    // Whether the settings modal is open (for the selected target).
    settings_open: bool,
    schedules: Vec<ScheduleForm>,
    selected_schedule: Option<usize>,
    // Restore
    restore_target: Option<String>,
    snapshots: Vec<String>,
    selected_snapshot: Option<usize>,
    restore_dest: String,
    tree: Vec<TreeEntry>,
    cwd: String,
    // Snapshot count per target name.
    counts: HashMap<String, usize>,
    // Configured rclone remotes (for the backend guide).
    rclone_remotes: Vec<String>,
    // The hero background image (navy + grid + glow).
    hero: iced::widget::image::Handle,
    // Run log (newest first) + the pending operation to log when done.
    history: Vec<LogEntry>,
    pending_op: Option<(String, String, String)>, // (op, target, info)
    log: String,
    status: String,
    running: bool,
}

#[derive(Debug, Clone)]
enum Message {
    SwitchTab(Tab),
    Select(usize),
    AddTarget,
    RequestDeleteTarget(usize),
    ConfirmDeleteTarget(usize),
    CancelDelete,
    OpenSettings(usize),
    CloseSettings,
    NoOp,
    // Schedule editing by index (the modal's filtered view).
    ModAddSchedule(String),
    ModDeleteSchedule(usize),
    ModSchedName(usize, String),
    ModSchedEnabled(usize, bool),
    ModSchedFrequency(usize, Frequency),
    ModSchedHour(usize, String),
    ModSchedMinute(usize, String),
    ModSchedWeekday(usize, String),
    Name(String),
    BackendSelected(Backend),
    Host(String),
    User(String),
    Port(String),
    Key(String),
    PickKey,
    KeyPicked(Option<String>),
    Password(String),
    Dest(String),
    SourceChanged(usize, String),
    AddSource,
    RemoveSource(usize),
    PickSource(usize),
    SourcePicked(usize, Option<String>),
    ExcludeChanged(usize, String),
    AddExclude,
    RemoveExclude(usize),
    RetLast(String),
    RetDaily(String),
    RetWeekly(String),
    RetMonthly(String),
    PruneNow,
    PruneFinished(Result<String, String>),
    SelectSchedule(usize),
    AddSchedule,
    DeleteSchedule,
    SchedName(String),
    SchedTarget(String),
    SchedFrequency(Frequency),
    SchedHour(String),
    SchedMinute(String),
    SchedWeekday(String),
    SchedEnabled(bool),
    InstallCron,
    RefreshCounts,
    CountResult(String, Result<usize, String>),
    // Restore
    RestoreTargetSelected(String),
    ListSnapshots,
    SnapshotsListed(Result<Vec<String>, String>),
    SelectSnapshot(usize),
    RestoreDest(String),
    PickRestoreDest,
    RestoreDestPicked(Option<String>),
    BrowseFiles,
    FilesListed(Result<Vec<(bool, String)>, String>),
    ToggleFile(usize),
    EnterDir(String),
    ClearSelection,
    RunRestore(bool),
    // Common
    Save,
    Run(bool),
    ProgressLine(String),
    ProgressDone(bool, String),
    VerifyTarget,
    VerifyFinished(String),
    RefreshHistory,
    RcloneRemotes(Vec<String>),
    OpenRcloneConfig,
    RefreshRemotes,
    OpenLink,
}

impl std::fmt::Debug for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Tab::QuickBackup => "QuickBackup",
            Tab::Schedule => "Schedule",
            Tab::Restore => "Restore",
            Tab::History => "History",
        })
    }
}

impl App {
    fn new() -> (App, Task<Message>) {
        let path = PathBuf::from(CONFIG_PATH);
        let (targets, schedules, status) = match Config::load(&path) {
            Ok(cfg) => {
                let targets: Vec<TargetForm> =
                    cfg.targets.iter().map(TargetForm::from_target).collect();
                let schedules: Vec<ScheduleForm> = cfg
                    .schedules
                    .iter()
                    .map(ScheduleForm::from_schedule)
                    .collect();
                let msg = format!(
                    "Loaded {} target(s) and {} schedule(s) from {CONFIG_PATH}",
                    targets.len(),
                    schedules.len()
                );
                (targets, schedules, msg)
            }
            Err(_) => (
                Vec::new(),
                Vec::new(),
                format!("No {CONFIG_PATH} — create a new target"),
            ),
        };
        let selected = if targets.is_empty() { None } else { Some(0) };
        let selected_schedule = if schedules.is_empty() { None } else { Some(0) };
        let restore_target = targets.first().map(|t| t.name.trim().to_string());
        (
            App {
                tab: Tab::QuickBackup,
                targets,
                selected,
                confirm_delete: None,
                settings_open: false,
                schedules,
                selected_schedule,
                restore_target,
                snapshots: Vec::new(),
                selected_snapshot: None,
                restore_dest: String::new(),
                tree: Vec::new(),
                cwd: String::new(),
                counts: HashMap::new(),
                rclone_remotes: Vec::new(),
                hero: iced::widget::image::Handle::from_bytes(
                    include_bytes!("../../assets/hero-bg.png").to_vec(),
                ),
                history: history::read(Path::new(CONFIG_PATH)),
                pending_op: None,
                log: String::new(),
                status,
                running: false,
            },
            Task::perform(list_remotes(), Message::RcloneRemotes),
        )
    }

    fn current_mut(&mut self) -> Option<&mut TargetForm> {
        self.selected.and_then(|i| self.targets.get_mut(i))
    }

    /// Removes target `i` and keeps `selected` valid.
    fn remove_target(&mut self, i: usize) {
        if i >= self.targets.len() {
            return;
        }
        self.targets.remove(i);
        self.confirm_delete = None;
        self.settings_open = false;
        self.selected = match self.selected {
            _ if self.targets.is_empty() => None,
            Some(s) if s > i => Some(s - 1),
            Some(s) => Some(s.min(self.targets.len() - 1)),
            None => None,
        };
    }

    fn current_sched_mut(&mut self) -> Option<&mut ScheduleForm> {
        self.selected_schedule.and_then(|i| self.schedules.get_mut(i))
    }

    /// Looks up a target by name and converts it to a `Target`.
    fn target_by_name(&self, name: &str) -> Option<Target> {
        self.targets
            .iter()
            .find(|t| t.name.trim() == name)
            .map(TargetForm::to_target)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SwitchTab(t) => {
                self.tab = t;
                self.settings_open = false;
            }

            Message::Select(i) => {
                self.selected = Some(i);
                self.confirm_delete = None;
            }
            Message::AddTarget => {
                self.targets.push(TargetForm {
                    name: "new-target".to_string(),
                    port: "22".to_string(),
                    ..Default::default()
                });
                self.selected = Some(self.targets.len() - 1);
                self.confirm_delete = None;
            }
            Message::RequestDeleteTarget(i) => self.confirm_delete = Some(i),
            Message::ConfirmDeleteTarget(i) => self.remove_target(i),
            Message::CancelDelete => self.confirm_delete = None,
            Message::OpenSettings(i) => {
                self.selected = Some(i);
                self.confirm_delete = None;
                self.settings_open = true;
            }
            Message::CloseSettings => self.settings_open = false,
            Message::NoOp => {}
            Message::ModAddSchedule(target) => {
                self.schedules.push(ScheduleForm {
                    name: format!("{}-schedule", target.trim()),
                    target,
                    frequency: Frequency::Daily,
                    hour: "2".to_string(),
                    minute: "0".to_string(),
                    weekday: 1,
                    enabled: true,
                });
            }
            Message::ModDeleteSchedule(i) => {
                if i < self.schedules.len() {
                    self.schedules.remove(i);
                }
            }
            Message::ModSchedName(i, v) => {
                if let Some(s) = self.schedules.get_mut(i) {
                    s.name = v;
                }
            }
            Message::ModSchedEnabled(i, b) => {
                if let Some(s) = self.schedules.get_mut(i) {
                    s.enabled = b;
                }
            }
            Message::ModSchedFrequency(i, f) => {
                if let Some(s) = self.schedules.get_mut(i) {
                    s.frequency = f;
                }
            }
            Message::ModSchedHour(i, v) => {
                if let Some(s) = self.schedules.get_mut(i) {
                    s.hour = digits(&v);
                }
            }
            Message::ModSchedMinute(i, v) => {
                if let Some(s) = self.schedules.get_mut(i) {
                    s.minute = digits(&v);
                }
            }
            Message::ModSchedWeekday(i, name) => {
                if let Some(idx) = WEEKDAYS.iter().position(|&w| w == name) {
                    if let Some(s) = self.schedules.get_mut(i) {
                        s.weekday = idx as u8;
                    }
                }
            }
            Message::Name(v) => set(self.current_mut(), |t| t.name = v),
            Message::BackendSelected(b) => set(self.current_mut(), |t| {
                // Suggest a default port when the backend changes.
                if b == Backend::Ftp && (t.port.is_empty() || t.port == "22") {
                    t.port = "21".to_string();
                } else if b == Backend::Ssh && (t.port.is_empty() || t.port == "21") {
                    t.port = "22".to_string();
                }
                t.backend = b;
            }),
            Message::Host(v) => set(self.current_mut(), |t| t.host = v),
            Message::User(v) => set(self.current_mut(), |t| t.user = v),
            Message::Port(v) => set(self.current_mut(), |t| t.port = digits(&v)),
            Message::Key(v) => set(self.current_mut(), |t| t.key = v),
            Message::PickKey => return Task::perform(pick_key_file(), Message::KeyPicked),
            Message::KeyPicked(path) => {
                if let Some(p) = path {
                    set(self.current_mut(), |t| t.key = p);
                }
            }
            Message::Password(v) => set(self.current_mut(), |t| t.password = v),
            Message::Dest(v) => set(self.current_mut(), |t| t.dest = v),
            Message::SourceChanged(i, v) => set(self.current_mut(), |t| {
                if let Some(s) = t.sources.get_mut(i) {
                    *s = v;
                }
            }),
            Message::AddSource => set(self.current_mut(), |t| t.sources.push(String::new())),
            Message::RemoveSource(i) => set(self.current_mut(), |t| {
                if i < t.sources.len() {
                    t.sources.remove(i);
                }
            }),
            Message::PickSource(i) => {
                return Task::perform(pick_folder(), move |p| Message::SourcePicked(i, p));
            }
            Message::SourcePicked(i, path) => {
                if let Some(p) = path {
                    set(self.current_mut(), |t| {
                        if let Some(s) = t.sources.get_mut(i) {
                            *s = p;
                        }
                    });
                }
            }
            Message::ExcludeChanged(i, v) => set(self.current_mut(), |t| {
                if let Some(e) = t.exclude.get_mut(i) {
                    *e = v;
                }
            }),
            Message::AddExclude => set(self.current_mut(), |t| t.exclude.push(String::new())),
            Message::RemoveExclude(i) => set(self.current_mut(), |t| {
                if i < t.exclude.len() {
                    t.exclude.remove(i);
                }
            }),
            Message::RetLast(v) => set(self.current_mut(), |t| t.keep_last = digits(&v)),
            Message::RetDaily(v) => set(self.current_mut(), |t| t.keep_daily = digits(&v)),
            Message::RetWeekly(v) => set(self.current_mut(), |t| t.keep_weekly = digits(&v)),
            Message::RetMonthly(v) => set(self.current_mut(), |t| t.keep_monthly = digits(&v)),
            Message::PruneNow => {
                if self.running {
                    return Task::none();
                }
                let Some(form) = self.selected.and_then(|i| self.targets.get(i)) else {
                    self.status = "No target selected".to_string();
                    return Task::none();
                };
                let target = form.to_target();
                let policy = form.retention();
                if target.host.is_empty() {
                    self.status = "Target needs a host".to_string();
                    return Task::none();
                }
                if policy.is_empty() {
                    self.status = "Set a retention policy first".to_string();
                    return Task::none();
                }
                self.running = true;
                self.pending_op = Some(("prune".into(), target.name.clone(), String::new()));
                self.status = format!("Pruning {}…", target.name);
                self.log = format!("Prune {}\n", target.name);
                return Task::perform(prune_now(target, policy), Message::PruneFinished);
            }
            Message::PruneFinished(result) => {
                self.running = false;
                let target = self
                    .pending_op
                    .take()
                    .map(|(_, t, _)| t)
                    .unwrap_or_default();
                match result {
                    Ok(msg) => {
                        self.log.push_str(&msg);
                        self.log.push('\n');
                        self.push_history(LogEntry::new("prune", &target, true, msg.clone()));
                        self.status = msg;
                    }
                    Err(e) => {
                        self.log.push_str(&e);
                        self.log.push('\n');
                        let detail = e.lines().next().unwrap_or("failed").trim().to_string();
                        self.push_history(LogEntry::new("prune", &target, false, detail));
                        self.status = "Prune failed ✗".to_string();
                    }
                }
            }
            Message::RefreshHistory => {
                self.history = history::read(Path::new(CONFIG_PATH));
                self.status = format!("{} log entries", self.history.len());
            }
            Message::RcloneRemotes(remotes) => self.rclone_remotes = remotes,
            Message::RefreshRemotes => {
                return Task::perform(list_remotes(), Message::RcloneRemotes);
            }
            Message::OpenRcloneConfig => {
                self.status = if open_rclone_config() {
                    "Opened rclone config in a terminal — click Refresh when done".to_string()
                } else {
                    "Couldn't open a terminal — run `rclone config` manually".to_string()
                };
            }

            Message::SelectSchedule(i) => self.selected_schedule = Some(i),
            Message::AddSchedule => {
                self.schedules.push(ScheduleForm::default());
                self.selected_schedule = Some(self.schedules.len() - 1);
            }
            Message::DeleteSchedule => {
                if let Some(i) = self.selected_schedule {
                    self.schedules.remove(i);
                    self.selected_schedule = pick_after_remove(self.schedules.len(), i);
                }
            }
            Message::SchedName(v) => set(self.current_sched_mut(), |s| s.name = v),
            Message::SchedTarget(v) => set(self.current_sched_mut(), |s| s.target = v),
            Message::SchedFrequency(f) => set(self.current_sched_mut(), |s| s.frequency = f),
            Message::SchedHour(v) => set(self.current_sched_mut(), |s| s.hour = digits(&v)),
            Message::SchedMinute(v) => set(self.current_sched_mut(), |s| s.minute = digits(&v)),
            Message::SchedWeekday(name) => {
                if let Some(idx) = WEEKDAYS.iter().position(|&w| w == name) {
                    set(self.current_sched_mut(), |s| s.weekday = idx as u8);
                }
            }
            Message::SchedEnabled(b) => set(self.current_sched_mut(), |s| s.enabled = b),
            Message::InstallCron => {
                let _ = self.build_config().save(&PathBuf::from(CONFIG_PATH));
                let scheds: Vec<Schedule> =
                    self.schedules.iter().map(ScheduleForm::to_schedule).collect();
                self.status = match install_crontab(&scheds) {
                    Ok(n) => format!("Installed {n} schedule(s) to crontab"),
                    Err(e) => format!("Crontab error: {e}"),
                };
            }

            Message::RefreshCounts => {
                let tasks: Vec<Task<Message>> = self
                    .targets
                    .iter()
                    .filter(|t| !t.name.trim().is_empty())
                    .map(|t| {
                        let name = t.name.trim().to_string();
                        let target = t.to_target();
                        Task::perform(count_snapshots(target, name), |(n, r)| {
                            Message::CountResult(n, r)
                        })
                    })
                    .collect();
                if tasks.is_empty() {
                    return Task::none();
                }
                self.status = "Counting snapshots…".to_string();
                return Task::batch(tasks);
            }
            Message::CountResult(name, result) => match result {
                Ok(n) => {
                    self.counts.insert(name, n);
                    self.status = "Snapshot counts updated".to_string();
                }
                Err(_) => {
                    self.counts.remove(&name);
                }
            },

            Message::RestoreTargetSelected(name) => {
                self.restore_target = Some(name);
                self.snapshots.clear();
                self.selected_snapshot = None;
                self.tree.clear();
                self.cwd.clear();
            }
            Message::ListSnapshots => {
                if self.running {
                    return Task::none();
                }
                let Some(name) = self.restore_target.clone() else {
                    self.status = "Select a target first".to_string();
                    return Task::none();
                };
                let Some(target) = self.target_by_name(&name) else {
                    self.status = "Unknown target".to_string();
                    return Task::none();
                };
                self.running = true;
                self.status = format!("Listing snapshots on {name}…");
                return Task::perform(list_snapshots(target), Message::SnapshotsListed);
            }
            Message::SnapshotsListed(result) => {
                self.running = false;
                self.selected_snapshot = None;
                self.tree.clear();
                self.cwd.clear();
                match result {
                    Ok(list) => {
                        self.status = format!("Found {} snapshot(s)", list.len());
                        if let Some(name) = self.restore_target.clone() {
                            self.counts.insert(name, list.len());
                        }
                        self.snapshots = list;
                    }
                    Err(e) => {
                        self.snapshots.clear();
                        self.status = format!("Could not list snapshots: {e}");
                        self.log = e;
                    }
                }
            }
            Message::SelectSnapshot(i) => {
                self.selected_snapshot = Some(i);
                self.tree.clear();
                self.cwd.clear();
                // Suggest a restore folder if the field is empty.
                if self.restore_dest.trim().is_empty() {
                    if let (Some(name), Some(ts)) =
                        (self.restore_target.as_ref(), self.snapshots.get(i))
                    {
                        self.restore_dest = format!("~/restore/{name}/{ts}");
                    }
                }
            }
            Message::RestoreDest(v) => self.restore_dest = v,
            Message::PickRestoreDest => {
                return Task::perform(pick_folder(), Message::RestoreDestPicked);
            }
            Message::RestoreDestPicked(path) => {
                if let Some(p) = path {
                    self.restore_dest = p;
                }
            }
            Message::BrowseFiles => {
                if self.running {
                    return Task::none();
                }
                let Some(name) = self.restore_target.clone() else {
                    return Task::none();
                };
                let Some(target) = self.target_by_name(&name) else {
                    return Task::none();
                };
                let Some(ts) = self
                    .selected_snapshot
                    .and_then(|i| self.snapshots.get(i))
                    .cloned()
                else {
                    self.status = "Select a snapshot first".to_string();
                    return Task::none();
                };
                self.running = true;
                self.status = format!("Browsing {ts}…");
                return Task::perform(list_tree(target, ts), Message::FilesListed);
            }
            Message::FilesListed(result) => {
                self.running = false;
                match result {
                    Ok(entries) => {
                        self.tree = entries
                            .into_iter()
                            .map(|(is_dir, path)| {
                                let name = path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&path)
                                    .to_string();
                                TreeEntry {
                                    path,
                                    name,
                                    is_dir,
                                    checked: false,
                                }
                            })
                            .collect();
                        self.cwd.clear();
                        self.status = format!("{} item(s) in snapshot", self.tree.len());
                    }
                    Err(e) => {
                        self.tree.clear();
                        self.status = format!("Could not browse: {e}");
                        self.log = e;
                    }
                }
            }
            Message::ToggleFile(i) => {
                if let Some(e) = self.tree.get_mut(i) {
                    e.checked = !e.checked;
                }
            }
            Message::EnterDir(path) => self.cwd = path,
            Message::ClearSelection => {
                for e in &mut self.tree {
                    e.checked = false;
                }
            }
            Message::RunRestore(dry_run) => {
                if self.running {
                    return Task::none();
                }
                let Some(name) = self.restore_target.clone() else {
                    self.status = "Select a target first".to_string();
                    return Task::none();
                };
                let Some(target) = self.target_by_name(&name) else {
                    self.status = "Unknown target".to_string();
                    return Task::none();
                };
                let Some(ts) = self
                    .selected_snapshot
                    .and_then(|i| self.snapshots.get(i))
                    .cloned()
                else {
                    self.status = "Select a snapshot".to_string();
                    return Task::none();
                };
                let dest = self.restore_dest.trim().to_string();
                if dest.is_empty() {
                    self.status = "Enter a folder to restore into".to_string();
                    return Task::none();
                }
                let selected: Vec<String> = self
                    .tree
                    .iter()
                    .filter(|e| e.checked)
                    .map(|e| e.path.clone())
                    .collect();
                self.running = true;
                let scope = if selected.is_empty() {
                    "whole snapshot".to_string()
                } else {
                    format!("{} selected item(s)", selected.len())
                };
                self.status = if dry_run {
                    format!("Dry run: restoring {scope}…")
                } else {
                    format!("Restoring {scope} → {dest}…")
                };
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
                self.pending_op = if dry_run {
                    None
                } else {
                    Some(("restore".into(), name.clone(), format!("{ts} → {dest}")))
                };
                self.log = format!("Restore {ts}\n");
                return Task::run(run_stream(vec![cmd]), map_prog);
            }

            Message::Save => {
                self.status = match self.build_config().save(&PathBuf::from(CONFIG_PATH)) {
                    Ok(()) => format!("Saved {CONFIG_PATH}"),
                    Err(e) => format!("Error while saving: {e:#}"),
                };
            }
            Message::Run(dry_run) => {
                if self.running {
                    return Task::none();
                }
                let Some(form) = self.selected.and_then(|i| self.targets.get(i)) else {
                    self.status = "No target selected".to_string();
                    return Task::none();
                };
                let target = form.to_target();
                if target.host.is_empty() || target.sources.is_empty() {
                    self.status = "Target needs a host and at least one source".to_string();
                    return Task::none();
                }
                self.running = true;
                self.status = if dry_run {
                    format!("Dry run against {}…", target.name)
                } else {
                    format!("Running backup against {}…", target.name)
                };
                let ts = snapshot::timestamp();
                let cmds = if target.backend.is_ssh() {
                    let dest = snapshot::snapshot_dir(&target, &ts);
                    let mut args =
                        rsync::build_args(&target, &dest, Some(rsync::LINK_DEST), dry_run);
                    ensure_verbose(&mut args);
                    let mut c = vec![("rsync".to_string(), args)];
                    if !dry_run {
                        let latest = snapshot::update_latest_cmd(&target, &ts);
                        c.push(("ssh".to_string(), ssh::remote_command_args(&target, &latest)));
                    }
                    c
                } else {
                    // GUI backup without --copy-dest (scheduled CLI runs do
                    // the bandwidth-efficient variant).
                    rclone::backup_cmds(&target, &ts, None, dry_run)
                };
                self.pending_op = if dry_run {
                    None
                } else {
                    Some(("backup".into(), target.name.clone(), format!("snapshot {ts}")))
                };
                self.log = format!("snapshot {ts}\n");
                return Task::run(run_stream(cmds), map_prog);
            }
            Message::ProgressLine(line) => {
                self.log.push_str(&line);
                self.log.push('\n');
            }
            Message::ProgressDone(ok, err) => {
                self.running = false;
                if ok {
                    self.status = "Done ✓".to_string();
                } else {
                    if !err.trim().is_empty() {
                        self.log.push_str(&err);
                        if !err.ends_with('\n') {
                            self.log.push('\n');
                        }
                    }
                    self.status = "Failed ✗".to_string();
                }
                self.record(ok, &err);
            }
            Message::VerifyTarget => {
                if self.running {
                    return Task::none();
                }
                let Some(form) = self.selected.and_then(|i| self.targets.get(i)) else {
                    self.status = "No target selected".to_string();
                    return Task::none();
                };
                let target = form.to_target();
                if target.host.is_empty() {
                    self.status = "Target needs a host".to_string();
                    return Task::none();
                }
                self.running = true;
                self.status = format!("Verifying {}…", target.name);
                self.log = format!("Verify {}\n", target.name);
                return Task::perform(verify_target(target), Message::VerifyFinished);
            }
            Message::VerifyFinished(report) => {
                self.running = false;
                self.log.push_str(&report);
                self.status = if report.contains('✗') {
                    "Verify: issues found ✗".to_string()
                } else {
                    "Verify: all checks passed ✓".to_string()
                };
            }
            Message::OpenLink => {
                let _ = open::that(AUTHOR_URL);
            }
        }
        Task::none()
    }

    fn build_config(&self) -> Config {
        Config {
            targets: self.targets.iter().map(TargetForm::to_target).collect(),
            schedules: self.schedules.iter().map(ScheduleForm::to_schedule).collect(),
        }
    }

    /// Writes an entry to history.jsonl and puts it at the top in memory.
    fn push_history(&mut self, entry: LogEntry) {
        let _ = history::append(Path::new(CONFIG_PATH), &entry);
        self.history.insert(0, entry);
    }

    /// Logs the outcome of the pending operation (if any).
    /// On failure, the last non-empty line in `err` is used as the detail.
    fn record(&mut self, ok: bool, err: &str) {
        if let Some((op, target, info)) = self.pending_op.take() {
            let detail = if ok {
                info
            } else {
                err.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("failed")
                    .trim()
                    .to_string()
            };
            self.push_history(LogEntry::new(&op, &target, ok, detail));
        }
    }

    // ─────────────────────────── views ───────────────────────────

    fn view(&self) -> Element<'_, Message> {
        let header = column![
            text("Moraine").size(28).font(semibold()),
            muted_text(format!("Snapshot backups over SSH & rclone · v{}", moraine::VERSION)),
        ]
        .spacing(2);

        let content = match self.tab {
            Tab::QuickBackup => self.view_quick_backup(),
            Tab::Schedule => self.view_schedule(),
            Tab::Restore => self.view_restore(),
            Tab::History => self.view_history(),
        };

        let inner = column![
            header,
            self.view_tabs(),
            content,
            view_status_bar(&self.status),
            view_footer(),
        ]
        .spacing(18);

        let foreground = container(inner)
            .style(|theme: &Theme| container::Style {
                text_color: Some(pal(theme).text),
                ..Default::default()
            })
            .padding(28)
            .width(Length::Fill)
            .height(Length::Fill);

        // The hero background (navy + grid + glow) behind all content.
        let background = iced::widget::image(self.hero.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(iced::ContentFit::Cover);

        let mut stack = iced::widget::Stack::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .push(background)
            .push(foreground);
        if self.settings_open && self.selected.is_some() {
            stack = stack.push(self.view_settings_modal());
        }
        stack.into()
    }

    fn view_tabs(&self) -> Element<'_, Message> {
        let pill = |label: &str, t: Tab| {
            let active = self.tab == t;
            button(text(label.to_string()).size(14))
                .padding([8.0, 18.0])
                .style(move |theme, status| tab_style(theme, status, active))
                .on_press(Message::SwitchTab(t))
        };
        container(
            row![
                pill("Quick Backup", Tab::QuickBackup),
                pill("Schedule", Tab::Schedule),
                pill("Restore", Tab::Restore),
                pill("History", Tab::History),
            ]
            .spacing(4),
        )
        .padding(4)
        .style(segmented_style)
        .into()
    }

    fn view_quick_backup(&self) -> Element<'_, Message> {
        let body = row![self.view_target_list(), self.view_target_form()]
            .spacing(18)
            .height(Length::Fill);
        column![body, view_log(&self.log)]
            .spacing(18)
            .height(Length::Fill)
            .into()
    }

    fn view_target_list(&self) -> Element<'_, Message> {
        let mut list = Column::new().spacing(4);
        for (i, t) in self.targets.iter().enumerate() {
            let row_el = if self.confirm_delete == Some(i) {
                row![
                    container(muted_text("Remove?")).padding([0.0, 10.0]),
                    Space::with_width(Length::Fill),
                    button(text("✓").size(13))
                        .padding([8.0, 10.0])
                        .style(danger_btn)
                        .on_press(Message::ConfirmDeleteTarget(i)),
                    button(text("✗").size(13))
                        .padding([8.0, 10.0])
                        .style(ghost_btn)
                        .on_press(Message::CancelDelete),
                ]
                .spacing(4)
                .align_y(iced::alignment::Vertical::Center)
            } else {
                row![
                    button(text(t.label()).width(Length::Fill))
                        .padding([10.0, 12.0])
                        .width(Length::Fill)
                        .style(list_item_style(self.selected == Some(i)))
                        .on_press(Message::Select(i)),
                    button(text("⚙").size(14))
                        .padding([8.0, 9.0])
                        .style(ghost_btn)
                        .on_press(Message::OpenSettings(i)),
                    button(text("✕").size(13))
                        .padding([8.0, 10.0])
                        .style(ghost_btn)
                        .on_press(Message::RequestDeleteTarget(i)),
                ]
                .spacing(4)
                .align_y(iced::alignment::Vertical::Center)
            };
            list = list.push(row_el);
        }
        sidebar(
            "Targets",
            scrollable(list).height(Length::Fill).into(),
            button(text("+ New target").width(Length::Fill))
                .padding([10.0, 12.0])
                .width(Length::Fill)
                .style(ghost_btn)
                .on_press(Message::AddTarget)
                .into(),
        )
    }

    fn view_target_form(&self) -> Element<'_, Message> {
        let Some(form) = self.selected.and_then(|i| self.targets.get(i)) else {
            return empty_card("Select a target or create a new one.");
        };

        let mut conn = column![
            field("Name", &form.name, Message::Name),
            labeled(
                "Backend",
                pick_list(
                    Backend::ALL.to_vec(),
                    Some(form.backend),
                    Message::BackendSelected,
                )
                .padding(10)
                .width(Length::Fill)
                .style(picklist_style),
            ),
        ]
        .spacing(12);
        match form.backend {
            Backend::Ssh => {
                conn = conn.push(
                    row![
                        field("Host / IP", &form.host, Message::Host),
                        fixed_field("Port", &form.port, Message::Port, 90.0),
                    ]
                    .spacing(12),
                );
            }
            Backend::Ftp => {
                conn = conn.push(
                    row![
                        field("FTP host", &form.host, Message::Host),
                        fixed_field("Port", &form.port, Message::Port, 90.0),
                    ]
                    .spacing(12),
                );
            }
            Backend::Rclone => {
                conn = conn.push(field(
                    "Rclone remote (empty = local path)",
                    &form.host,
                    Message::Host,
                ));
            }
        }
        conn = conn.push(muted_text(
            "User, key, password, destination, sources and retention are under ⚙ Settings.",
        ));
        let connection = section("Connection", conn);

        let run_label = if self.running { "Running…" } else { "Run backup" };
        let mut run_btn = button(text(run_label)).padding([10.0, 18.0]).style(primary_btn);
        let mut dry_btn = button(text("Dry run")).padding([10.0, 16.0]).style(ghost_btn);
        let mut verify_btn = button(text("Test connection"))
            .padding([10.0, 16.0])
            .style(ghost_btn);
        if !self.running {
            run_btn = run_btn.on_press(Message::Run(false));
            dry_btn = dry_btn.on_press(Message::Run(true));
            verify_btn = verify_btn.on_press(Message::VerifyTarget);
        }

        let actions = row![
            button(text("Save"))
                .padding([10.0, 16.0])
                .style(ghost_btn)
                .on_press(Message::Save),
            button(text("⚙ Settings"))
                .padding([10.0, 16.0])
                .style(ghost_btn)
                .on_press(Message::OpenSettings(self.selected.unwrap_or(0))),
            Space::with_width(Length::Fill),
            verify_btn,
            dry_btn,
            run_btn,
        ]
        .spacing(10);

        form_card(column![connection, actions].spacing(22))
    }

    /// The settings modal for the selected target (overlay).
    fn view_settings_modal(&self) -> Element<'_, Message> {
        let Some(form) = self.selected.and_then(|i| self.targets.get(i)) else {
            return Space::new(Length::Fixed(0.0), Length::Fixed(0.0)).into();
        };

        let header = row![
            text(format!("Settings: {}", form.label()))
                .size(18)
                .font(semibold()),
            Space::with_width(Length::Fill),
            button(text("✕").size(15))
                .padding([6.0, 10.0])
                .style(ghost_btn)
                .on_press(Message::CloseSettings),
        ]
        .align_y(iced::alignment::Vertical::Center);

        let mut details = Column::new().spacing(12);
        match form.backend {
            Backend::Ssh => {
                details = details.push(field("User", &form.user, Message::User));
                details = details.push(labeled(
                    "SSH key (optional)",
                    row![
                        text_input("", &form.key)
                            .on_input(Message::Key)
                            .padding(10)
                            .style(input_style),
                        button(text("Browse…"))
                            .padding([10.0, 14.0])
                            .style(ghost_btn)
                            .on_press(Message::PickKey),
                    ]
                    .spacing(8),
                ));
                details = details.push(field("Destination on target", &form.dest, Message::Dest));
            }
            Backend::Rclone => {
                details = details.push(rclone_guide(&self.rclone_remotes));
                details = details.push(field(
                    "Destination (path within remote)",
                    &form.dest,
                    Message::Dest,
                ));
            }
            Backend::Ftp => {
                details = details.push(field("User", &form.user, Message::User));
                details =
                    details.push(password_field("Password", &form.password, Message::Password));
                details = details.push(field("Destination (path on server)", &form.dest, Message::Dest));
            }
        }
        let connection = section("Connection details", details);

        let files = section(
            "Files",
            column![
                list_editor(
                    "Sources (files/folders on the client)",
                    &form.sources,
                    Message::SourceChanged,
                    Message::RemoveSource,
                    Message::AddSource,
                    Some(Message::PickSource),
                ),
                list_editor(
                    "Exclude patterns (optional)",
                    &form.exclude,
                    Message::ExcludeChanged,
                    Message::RemoveExclude,
                    Message::AddExclude,
                    None,
                ),
            ]
            .spacing(18),
        );

        let mut prune_btn = button(text("Prune now")).padding([10.0, 16.0]).style(ghost_btn);
        if !self.running {
            prune_btn = prune_btn.on_press(Message::PruneNow);
        }
        let retention = section(
            "Retention (snapshots to keep, 0 = keep all)",
            column![
                row![
                    fixed_field("Last", &form.keep_last, Message::RetLast, 84.0),
                    fixed_field("Daily", &form.keep_daily, Message::RetDaily, 84.0),
                    fixed_field("Weekly", &form.keep_weekly, Message::RetWeekly, 84.0),
                    fixed_field("Monthly", &form.keep_monthly, Message::RetMonthly, 84.0),
                ]
                .spacing(12),
                prune_btn,
            ]
            .spacing(12),
        );

        let schedule = self.modal_schedule_section();

        let footer = row![
            Space::with_width(Length::Fill),
            button(text("Save"))
                .padding([10.0, 18.0])
                .style(ghost_btn)
                .on_press(Message::Save),
            button(text("Close"))
                .padding([10.0, 18.0])
                .style(primary_btn)
                .on_press(Message::CloseSettings),
        ]
        .spacing(10);

        let content =
            column![header, connection, files, schedule, retention, footer].spacing(20);

        // Right padding so the scrollbar doesn't clip fields/✕ buttons.
        let inner_pad = iced::Padding {
            top: 0.0,
            right: 14.0,
            bottom: 0.0,
            left: 0.0,
        };
        let card = container(
            scrollable(container(content).padding(inner_pad)).height(Length::Fill),
        )
        .style(card_style)
        .padding(24)
        .width(Length::Fixed(840.0))
        .max_height(660.0);

        let overlay = container(card)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(scrim_style);

        iced::widget::mouse_area(overlay)
            .on_press(Message::NoOp)
            .into()
    }

    /// Schedule section in the modal — filtered to the selected target.
    fn modal_schedule_section(&self) -> Element<'_, Message> {
        let name = self
            .selected
            .and_then(|i| self.targets.get(i))
            .map(|t| t.name.trim().to_string())
            .unwrap_or_default();
        let mut col = Column::new().spacing(12);
        let mut any = false;
        for (i, s) in self.schedules.iter().enumerate() {
            if s.target.trim() != name {
                continue;
            }
            any = true;
            col = col.push(self.schedule_card(i, s));
        }
        if !any {
            col = col.push(muted_text("No schedules for this target yet."));
        }
        col = col.push(
            button(text("+ Add schedule"))
                .padding([8.0, 14.0])
                .style(ghost_btn)
                .on_press(Message::ModAddSchedule(name.clone())),
        );
        col = col.push(muted_text(
            "Activate via \"Install to crontab\" in the Schedule tab.",
        ));
        section("Schedule", col)
    }

    /// An editable schedule card in the modal (for schedule index `i`).
    fn schedule_card(&self, i: usize, s: &ScheduleForm) -> Element<'_, Message> {
        let top = row![
            text_input("", &s.name)
                .on_input(move |v| Message::ModSchedName(i, v))
                .padding(8)
                .style(input_style),
            checkbox("On", s.enabled)
                .on_toggle(move |b| Message::ModSchedEnabled(i, b))
                .style(checkbox_style),
            button(text("✕"))
                .padding([8.0, 10.0])
                .style(ghost_btn)
                .on_press(Message::ModDeleteSchedule(i)),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center);

        let mut mid = Row::new()
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .push(
                pick_list(Frequency::ALL.to_vec(), Some(s.frequency), move |f| {
                    Message::ModSchedFrequency(i, f)
                })
                .padding(8)
                .style(picklist_style),
            );
        if s.frequency != Frequency::Hourly {
            mid = mid.push(muted_text("at"));
            mid = mid.push(
                text_input("HH", &s.hour)
                    .on_input(move |v| Message::ModSchedHour(i, v))
                    .padding(8)
                    .width(Length::Fixed(60.0))
                    .style(input_style),
            );
        }
        mid = mid.push(
            text_input("MM", &s.minute)
                .on_input(move |v| Message::ModSchedMinute(i, v))
                .padding(8)
                .width(Length::Fixed(60.0))
                .style(input_style),
        );
        if s.frequency == Frequency::Weekly {
            let weekdays: Vec<String> = WEEKDAYS.iter().map(|w| w.to_string()).collect();
            let selected = WEEKDAYS.get(s.weekday as usize).map(|w| w.to_string());
            mid = mid.push(
                pick_list(weekdays, selected, move |w| Message::ModSchedWeekday(i, w))
                    .padding(8)
                    .style(picklist_style),
            );
        }

        let cron = muted_text(format!("cron: {}", s.to_schedule().cron()));

        container(column![top, mid, cron].spacing(8))
            .style(|theme: &Theme| {
                let p = pal(theme);
                container::Style {
                    background: Some(Background::Color(p.elevated)),
                    border: rounded(10.0, 1.0, p.border),
                    ..Default::default()
                }
            })
            .padding(12)
            .width(Length::Fill)
            .into()
    }

    fn view_schedule(&self) -> Element<'_, Message> {
        let mut list = Column::new().spacing(4);
        for (i, s) in self.schedules.iter().enumerate() {
            let off = if s.enabled { "" } else { "  (off)" };
            let count = self
                .counts
                .get(s.target.trim())
                .map(|c| format!("  ·  {c} snaps"))
                .unwrap_or_default();
            let label = format!("{}{off}{count}", s.label());
            list = list.push(
                button(text(label).width(Length::Fill))
                    .padding([10.0, 12.0])
                    .width(Length::Fill)
                    .style(list_item_style(self.selected_schedule == Some(i)))
                    .on_press(Message::SelectSchedule(i)),
            );
        }
        let buttons = column![
            button(text("Refresh counts").width(Length::Fill))
                .padding([10.0, 12.0])
                .width(Length::Fill)
                .style(ghost_btn)
                .on_press(Message::RefreshCounts),
            button(text("+ New schedule").width(Length::Fill))
                .padding([10.0, 12.0])
                .width(Length::Fill)
                .style(ghost_btn)
                .on_press(Message::AddSchedule),
            button(text("Install to crontab").width(Length::Fill))
                .padding([10.0, 12.0])
                .width(Length::Fill)
                .style(primary_btn)
                .on_press(Message::InstallCron),
        ]
        .spacing(8);

        let sidebar = sidebar(
            "Schedules",
            scrollable(list).height(Length::Fill).into(),
            buttons.into(),
        );

        row![sidebar, self.view_schedule_form()]
            .spacing(18)
            .height(Length::Fill)
            .into()
    }

    fn view_schedule_form(&self) -> Element<'_, Message> {
        let Some(form) = self.selected_schedule.and_then(|i| self.schedules.get(i)) else {
            return empty_card("Select a schedule or create a new one.");
        };

        let target_names: Vec<String> = self
            .targets
            .iter()
            .map(|t| t.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let selected_target = if form.target.is_empty() {
            None
        } else {
            Some(form.target.clone())
        };

        let mut col = column![
            field("Name", &form.name, Message::SchedName),
            row![
                labeled(
                    "Target",
                    pick_list(target_names, selected_target, Message::SchedTarget)
                        .padding(10)
                        .width(Length::Fill)
                        .style(picklist_style),
                ),
                labeled(
                    "Frequency",
                    pick_list(
                        Frequency::ALL.to_vec(),
                        Some(form.frequency),
                        Message::SchedFrequency,
                    )
                    .padding(10)
                    .width(Length::Fill)
                    .style(picklist_style),
                ),
            ]
            .spacing(12),
        ]
        .spacing(14);

        if form.frequency == Frequency::Hourly {
            col = col.push(fixed_field("Minute (0-59)", &form.minute, Message::SchedMinute, 120.0));
        } else {
            col = col.push(
                row![
                    fixed_field("Hour (0-23)", &form.hour, Message::SchedHour, 120.0),
                    fixed_field("Minute (0-59)", &form.minute, Message::SchedMinute, 120.0),
                ]
                .spacing(12),
            );
        }
        if form.frequency == Frequency::Weekly {
            let weekdays: Vec<String> = WEEKDAYS.iter().map(|s| s.to_string()).collect();
            let selected_weekday = WEEKDAYS.get(form.weekday as usize).map(|s| s.to_string());
            col = col.push(labeled(
                "Weekday",
                pick_list(weekdays, selected_weekday, Message::SchedWeekday)
                    .padding(10)
                    .width(Length::Fill)
                    .style(picklist_style),
            ));
        }

        col = col.push(checkbox("Enabled", form.enabled).on_toggle(Message::SchedEnabled).style(checkbox_style));
        col = col.push(cron_chip(&form.to_schedule().cron()));
        col = col.push(muted_text(
            "Click \"Install to crontab\" to activate all enabled schedules.",
        ));
        col = col.push(
            row![
                button(text("Save"))
                    .padding([10.0, 16.0])
                    .style(ghost_btn)
                    .on_press(Message::Save),
                button(text("Delete"))
                    .padding([10.0, 16.0])
                    .style(danger_btn)
                    .on_press(Message::DeleteSchedule),
            ]
            .spacing(10),
        );

        form_card(col)
    }

    fn view_restore(&self) -> Element<'_, Message> {
        let target_names: Vec<String> = self
            .targets
            .iter()
            .map(|t| t.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();

        let mut list = Column::new().spacing(4);
        if self.snapshots.is_empty() {
            list = list.push(muted_text("No snapshots loaded.\nClick \"List snapshots\"."));
        } else {
            for (i, s) in self.snapshots.iter().enumerate() {
                let label = if i == 0 {
                    format!("{s}   (newest)")
                } else {
                    s.clone()
                };
                list = list.push(
                    button(text(label).width(Length::Fill))
                        .padding([10.0, 12.0])
                        .width(Length::Fill)
                        .style(list_item_style(self.selected_snapshot == Some(i)))
                        .on_press(Message::SelectSnapshot(i)),
                );
            }
        }

        let count_line: Element<'_, Message> = if self.snapshots.is_empty() {
            Space::with_height(Length::Fixed(0.0)).into()
        } else {
            muted_text(format!("{} snapshot(s)", self.snapshots.len())).into()
        };

        let panel = container(
            column![
                text("Restore").size(16).font(semibold()),
                labeled(
                    "Target",
                    pick_list(
                        target_names,
                        self.restore_target.clone(),
                        Message::RestoreTargetSelected,
                    )
                    .padding(10)
                    .width(Length::Fill)
                    .style(picklist_style),
                ),
                button(text("List snapshots").width(Length::Fill))
                    .padding([10.0, 12.0])
                    .width(Length::Fill)
                    .style(ghost_btn)
                    .on_press(Message::ListSnapshots),
                count_line,
                scrollable(list).height(Length::Fill),
            ]
            .spacing(12)
            .height(Length::Fill),
        )
        .style(panel_style)
        .padding(16)
        .width(Length::Fixed(248.0))
        .height(Length::Fill);

        let body = row![panel, self.view_restore_form()]
            .spacing(18)
            .height(Length::Fill);
        column![body, view_log(&self.log)]
            .spacing(18)
            .height(Length::Fill)
            .into()
    }

    fn view_restore_form(&self) -> Element<'_, Message> {
        let Some(ts) = self.selected_snapshot.and_then(|i| self.snapshots.get(i)) else {
            return empty_card("Pick a target, list snapshots, then select one to restore.");
        };
        let target = self.restore_target.clone().unwrap_or_default();

        let info = section(
            "Restore snapshot",
            column![
                labeled("Snapshot", text(ts.clone()).font(iced::Font::MONOSPACE)),
                labeled("From target", text(target)),
            ]
            .spacing(12),
        );

        let dest = section(
            "Destination",
            column![
                labeled(
                    "Restore into this local folder",
                    row![
                        text_input("", &self.restore_dest)
                            .on_input(Message::RestoreDest)
                            .padding(10)
                            .style(input_style),
                        button(text("Browse…"))
                            .padding([10.0, 14.0])
                            .style(ghost_btn)
                            .on_press(Message::PickRestoreDest),
                    ]
                    .spacing(8),
                ),
                muted_text(
                    "Files are copied here — nothing is deleted. Use a fresh folder to avoid \
                     overwriting live data.",
                ),
            ]
            .spacing(8),
        );

        let mut browse_btn = button(text(if self.running {
            "Browsing…"
        } else {
            "Browse files in snapshot"
        }))
        .padding([10.0, 16.0])
        .style(ghost_btn);

        let mut restore_btn = button(text(if self.running {
            "Restoring…"
        } else {
            "Restore"
        }))
        .padding([10.0, 18.0])
        .style(primary_btn);
        let mut dry_btn = button(text("Dry run")).padding([10.0, 16.0]).style(ghost_btn);
        if !self.running {
            browse_btn = browse_btn.on_press(Message::BrowseFiles);
            restore_btn = restore_btn.on_press(Message::RunRestore(false));
            dry_btn = dry_btn.on_press(Message::RunRestore(true));
        }
        let actions = row![Space::with_width(Length::Fill), dry_btn, restore_btn].spacing(10);

        let mut col = column![info, dest, browse_btn].spacing(20);
        if !self.tree.is_empty() {
            col = col.push(self.view_file_tree());
        }
        col = col.push(actions);
        form_card(col)
    }

    fn view_history(&self) -> Element<'_, Message> {
        let header = row![
            text("Run history").size(16).font(semibold()),
            Space::with_width(Length::Fill),
            button(text("Refresh").size(13))
                .padding([6.0, 12.0])
                .style(ghost_btn)
                .on_press(Message::RefreshHistory),
        ]
        .align_y(iced::alignment::Vertical::Center)
        .spacing(8);

        let mut list = Column::new().spacing(8);
        if self.history.is_empty() {
            list = list.push(muted_text(
                "No runs logged yet. Backups, restores and prunes appear here.",
            ));
        } else {
            for e in &self.history {
                list = list.push(history_row(e));
            }
        }

        let card = container(scrollable(list).height(Length::Fill))
            .style(card_style)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill);

        column![header, card]
            .spacing(12)
            .height(Length::Fill)
            .into()
    }

    fn view_file_tree(&self) -> Element<'_, Message> {
        let selected = self.tree.iter().filter(|e| e.checked).count();

        // Breadcrumbs: snapshot / seg1 / seg2 …
        let mut crumbs = Row::new()
            .spacing(2)
            .align_y(iced::alignment::Vertical::Center);
        crumbs = crumbs.push(crumb("snapshot", "", self.cwd.is_empty()));
        if !self.cwd.is_empty() {
            let mut acc = String::new();
            for seg in self.cwd.split('/') {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(seg);
                crumbs = crumbs.push(muted_text(" / "));
                crumbs = crumbs.push(crumb(seg, &acc, acc == self.cwd));
            }
        }

        // Entries in the current folder.
        let mut list = Column::new().spacing(2);
        let mut shown = 0;
        for (i, e) in self.tree.iter().enumerate() {
            let parent = e.path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            if parent != self.cwd {
                continue;
            }
            shown += 1;
            let cb = checkbox("", e.checked)
                .on_toggle(move |_| Message::ToggleFile(i))
                .style(checkbox_style);
            let name: Element<'_, Message> = if e.is_dir {
                button(
                    row![
                        text(format!("{}/", e.name)).width(Length::Fill),
                        muted_text("▸"),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .padding([6.0, 8.0])
                .width(Length::Fill)
                .style(ghost_btn)
                .on_press(Message::EnterDir(e.path.clone()))
                .into()
            } else {
                container(text(e.name.clone()))
                    .padding([6.0, 8.0])
                    .width(Length::Fill)
                    .into()
            };
            list = list.push(
                row![cb, name]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
            );
        }
        if shown == 0 {
            list = list.push(muted_text("(empty folder)"));
        }

        // Row with the Up button, selected counter and Clear.
        let mut header = Row::new()
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center);
        if !self.cwd.is_empty() {
            let parent = self.cwd.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            header = header.push(
                button(text("Up").size(12))
                    .padding([4.0, 12.0])
                    .style(ghost_btn)
                    .on_press(Message::EnterDir(parent.to_string())),
            );
        }
        header = header.push(muted_text(format!(
            "{selected} selected — none = whole snapshot"
        )));
        header = header.push(Space::with_width(Length::Fill));
        header = header.push(
            button(text("Clear").size(12))
                .padding([4.0, 10.0])
                .style(ghost_btn)
                .on_press(Message::ClearSelection),
        );

        let tree_box = container(scrollable(list).height(Length::Fixed(200.0)))
            .style(|theme: &Theme| {
                let p = pal(theme);
                container::Style {
                    background: Some(Background::Color(p.elevated)),
                    border: rounded(10.0, 1.0, p.border),
                    ..Default::default()
                }
            })
            .padding(10)
            .width(Length::Fill);

        section("Browse snapshot", column![crumbs, header, tree_box].spacing(8))
    }
}

/// Opens `rclone config` in a terminal. True if a terminal could be started.
fn open_rclone_config() -> bool {
    fn try_spawn(term: &str, args: &[&str]) -> bool {
        std::process::Command::new(term).args(args).spawn().is_ok()
    }
    try_spawn("gnome-terminal", &["--", "rclone", "config"])
        || try_spawn("x-terminal-emulator", &["-e", "rclone", "config"])
        || try_spawn("konsole", &["-e", "rclone", "config"])
        || try_spawn("mate-terminal", &["--", "rclone", "config"])
        || try_spawn("xfce4-terminal", &["--command", "rclone config"])
        || try_spawn("xterm", &["-e", "rclone", "config"])
}

/// Help box shown when the rclone backend is selected.
fn rclone_guide(remotes: &[String]) -> Element<'static, Message> {
    let remotes_line = if remotes.is_empty() {
        "No remotes configured yet — click Open rclone config.".to_string()
    } else {
        format!("Configured remotes: {}", remotes.join(", "))
    };
    let buttons = row![
        button(text("Open rclone config").size(12))
            .padding([6.0, 12.0])
            .style(primary_btn)
            .on_press(Message::OpenRcloneConfig),
        button(text("Refresh").size(12))
            .padding([6.0, 12.0])
            .style(ghost_btn)
            .on_press(Message::RefreshRemotes),
    ]
    .spacing(8);
    container(
        column![
            text("rclone backend").size(13).font(semibold()),
            muted_text("Set up a remote (ftp, sftp, smb, webdav, s3, drive, b2, …), then use its name below (empty = a local path)."),
            muted_text(remotes_line),
            buttons,
        ]
        .spacing(6),
    )
    .style(|theme: &Theme| {
        let p = pal(theme);
        container::Style {
            background: Some(Background::Color(p.elevated)),
            border: rounded(10.0, 1.0, with_alpha(p.accent, 0.5)),
            text_color: Some(p.text),
            ..Default::default()
        }
    })
    .padding([10.0, 12.0])
    .width(Length::Fill)
    .into()
}

/// A row in the run log.
fn history_row(e: &LogEntry) -> Element<'static, Message> {
    let ok = e.ok;
    let status = text(if ok { "✓" } else { "✗" }).size(14).style(move |t: &Theme| {
        text::Style {
            color: Some(if ok { pal(t).accent } else { pal(t).danger }),
        }
    });
    let top = row![
        status,
        text(e.op.clone()).size(13).font(semibold()),
        muted_text(format!("· {}", e.target)),
        Space::with_width(Length::Fill),
        muted_text(e.time.clone()),
    ]
    .align_y(iced::alignment::Vertical::Center)
    .spacing(8);

    let body = if e.detail.is_empty() {
        column![top]
    } else {
        column![top, muted_text(e.detail.clone())]
    }
    .spacing(4);

    container(body)
        .style(|theme: &Theme| {
            let p = pal(theme);
            container::Style {
                background: Some(Background::Color(p.elevated)),
                border: rounded(8.0, 1.0, p.border),
                ..Default::default()
            }
        })
        .padding([8.0, 12.0])
        .width(Length::Fill)
        .into()
}

/// A breadcrumb: clickable except for the current folder.
fn crumb(label: &str, path: &str, current: bool) -> Element<'static, Message> {
    if current {
        text(label.to_string()).size(12).into()
    } else {
        button(text(label.to_string()).size(12))
            .padding([2.0, 4.0])
            .style(link_btn)
            .on_press(Message::EnterDir(path.to_string()))
            .into()
    }
}

// ─────────────────────────── view helpers ───────────────────────────

/// A sidebar: heading, content (fills height) and button(s) at the bottom.
fn sidebar<'a>(
    title: &str,
    content: Element<'a, Message>,
    footer: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        column![
            text(title.to_string()).size(16).font(semibold()),
            content,
            footer,
        ]
        .spacing(12)
        .height(Length::Fill),
    )
    .style(panel_style)
    .padding(16)
    .width(Length::Fixed(232.0))
    .height(Length::Fill)
    .into()
}

/// The form's right panel as a card (scrollable).
fn form_card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    // Right padding (top, right, bottom, left) reserves room for the scrollbar
    // so the fields are never clipped beneath it.
    let inner_pad = iced::Padding {
        top: 4.0,
        right: 16.0,
        bottom: 4.0,
        left: 4.0,
    };
    container(scrollable(container(content.into()).padding(inner_pad)).height(Length::Fill))
        .style(card_style)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Empty card with a muted text.
fn empty_card(msg: &str) -> Element<'static, Message> {
    container(muted_text(msg.to_string()))
        .style(card_style)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A section with a small heading above its content.
fn section<'a>(title: &str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![
        text(title.to_string()).size(13).font(semibold()),
        body.into(),
    ]
    .spacing(12)
    .into()
}

/// A small "chip" that shows the cron expression.
fn cron_chip(cron: &str) -> Element<'static, Message> {
    container(
        row![
            muted_text("CRON"),
            text(cron.to_string())
                .size(13)
                .font(iced::Font::MONOSPACE),
        ]
        .spacing(10),
    )
    .style(|theme: &Theme| {
        let p = pal(theme);
        container::Style {
            background: Some(Background::Color(p.elevated)),
            border: rounded(8.0, 1.0, p.border),
            text_color: Some(p.text),
            ..Default::default()
        }
    })
    .padding([8.0, 12.0])
    .into()
}

fn set<T>(item: Option<&mut T>, f: impl FnOnce(&mut T)) {
    if let Some(t) = item {
        f(t);
    }
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn pick_after_remove(len: usize, removed: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(removed.min(len - 1))
    }
}

/// A labeled text row (fills width).
fn field<'a>(
    label: &str,
    value: &str,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    labeled(
        label,
        text_input("", value)
            .on_input(on_change)
            .padding(10)
            .style(input_style),
    )
}

/// A labeled text row with masked input (password).
fn password_field<'a>(
    label: &str,
    value: &str,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    labeled(
        label,
        text_input("", value)
            .secure(true)
            .on_input(on_change)
            .padding(10)
            .style(input_style),
    )
}

/// Like `field` but with a fixed width (for short fields like port/hour).
fn fixed_field<'a>(
    label: &str,
    value: &str,
    on_change: impl Fn(String) -> Message + 'a,
    width: f32,
) -> Element<'a, Message> {
    column![
        muted_text(label.to_string()),
        text_input("", value)
            .on_input(on_change)
            .padding(10)
            .width(Length::Fixed(width))
            .style(input_style),
    ]
    .spacing(6)
    .into()
}

fn labeled<'a>(label: &str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![muted_text(label.to_string()), control.into()]
        .spacing(6)
        .into()
}

fn list_editor<'a>(
    title: &str,
    items: &[String],
    on_change: impl Fn(usize, String) -> Message + Copy + 'static,
    on_remove: impl Fn(usize) -> Message + Copy + 'a,
    on_add: Message,
    on_pick: Option<fn(usize) -> Message>,
) -> Element<'a, Message> {
    let mut col = Column::new().spacing(8);
    col = col.push(muted_text(title.to_string()));
    for (i, item) in items.iter().enumerate() {
        let mut r = Row::new().spacing(8).push(
            text_input("", item)
                .on_input(move |v| on_change(i, v))
                .padding(10)
                .style(input_style),
        );
        if let Some(pick) = on_pick {
            r = r.push(
                button(text("Browse…"))
                    .padding([10.0, 14.0])
                    .style(ghost_btn)
                    .on_press(pick(i)),
            );
        }
        r = r.push(
            button(text("✕"))
                .padding([10.0, 14.0])
                .style(ghost_btn)
                .on_press(on_remove(i)),
        );
        col = col.push(r);
    }
    col = col.push(
        button(text("+ Add"))
            .padding([8.0, 14.0])
            .style(ghost_btn)
            .on_press(on_add),
    );
    col.into()
}

fn view_status_bar(status: &str) -> Element<'_, Message> {
    container(text(status.to_string()).size(13))
        .style(|theme: &Theme| {
            let p = pal(theme);
            container::Style {
                background: Some(Background::Color(p.surface)),
                border: rounded(10.0, 1.0, p.border),
                text_color: Some(p.muted),
                ..Default::default()
            }
        })
        .width(Length::Fill)
        .padding([8.0, 14.0])
        .into()
}

fn view_log(log: &str) -> Element<'_, Message> {
    let content: Element<Message> = if log.is_empty() {
        muted_text("The log appears here when you run a backup.").into()
    } else {
        text(log.to_string())
            .size(12)
            .font(iced::Font::MONOSPACE)
            .into()
    };
    container(scrollable(content).width(Length::Fill))
        .style(|theme: &Theme| {
            let p = pal(theme);
            container::Style {
                background: Some(Background::Color(p.surface)),
                border: rounded(12.0, 1.0, p.border),
                text_color: Some(p.text),
                ..Default::default()
            }
        })
        .width(Length::Fill)
        .height(Length::Fixed(170.0))
        .padding(12)
        .into()
}

fn view_footer() -> Element<'static, Message> {
    let link = button(text(AUTHOR).size(12))
        .on_press(Message::OpenLink)
        .style(link_btn);
    row![Space::with_width(Length::Fill), link]
        .width(Length::Fill)
        .into()
}

// ─────────────────────────── crontab/backup ───────────────────────────

fn backup_cli_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("backup").display().to_string();
        }
    }
    "backup".to_string()
}

fn install_crontab(schedules: &[Schedule]) -> Result<usize, String> {
    const MARKER: &str = "# moraine";

    let existing = match std::process::Command::new("crontab").arg("-l").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    };

    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.contains(MARKER))
        .map(|s| s.to_string())
        .collect();

    let exe = backup_cli_path();
    let cfg = std::fs::canonicalize(CONFIG_PATH)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| CONFIG_PATH.to_string());

    let mut count = 0;
    for s in schedules.iter().filter(|s| s.enabled && !s.target.is_empty()) {
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

    let mut child = std::process::Command::new("crontab")
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
        Err(format!("crontab exited with {}", status.code().unwrap_or(-1)))
    }
}

/// Counts snapshots on a target. Returns `(name, count/err)`.
async fn count_snapshots(target: Target, name: String) -> (String, Result<usize, String>) {
    match list_snapshots(target).await {
        Ok(list) => (name, Ok(list.len())),
        Err(e) => (name, Err(e)),
    }
}

/// Opens a native file dialog to pick an SSH key. None if cancelled.
async fn pick_key_file() -> Option<String> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Select SSH private key");
    if let Ok(home) = std::env::var("HOME") {
        let ssh = std::path::Path::new(&home).join(".ssh");
        if ssh.is_dir() {
            dialog = dialog.set_directory(ssh);
        }
    }
    dialog
        .pick_file()
        .await
        .map(|h| h.path().display().to_string())
}

/// Opens a native folder picker. None if cancelled.
async fn pick_folder() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Select a folder")
        .pick_folder()
        .await
        .map(|h| h.path().display().to_string())
}

/// Lists configured rclone remotes (without the trailing `:`).
async fn list_remotes() -> Vec<String> {
    match tokio::process::Command::new("rclone")
        .arg("listremotes")
        .output()
        .await
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim().trim_end_matches(':').to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Runs an rclone command and returns its output.
async fn rclone_output(args: Vec<String>) -> Result<std::process::Output, String> {
    tokio::process::Command::new("rclone")
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("could not start rclone: {e}"))
}

/// Lists snapshots on the target (ssh `ls` or `rclone lsf`). Newest first.
async fn list_snapshots(target: Target) -> Result<Vec<String>, String> {
    let mut list = if target.backend.is_ssh() {
        let args = ssh::probe_command_args(&target, &snapshot::list_cmd(&target));
        let out = tokio::process::Command::new("ssh")
            .args(&args)
            .output()
            .await
            .map_err(|e| format!("could not start ssh: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ssh failed (exit {})\n{}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l != "latest")
            .collect::<Vec<_>>()
    } else {
        let out = rclone_output(rclone::list_args(&target)).await?;
        if !out.status.success() {
            return Ok(Vec::new()); // the base probably doesn't exist yet
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().trim_end_matches('/').to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
    };
    list.sort();
    list.reverse();
    Ok(list)
}

/// Lists the contents of a snapshot via `ssh find`. Returns `(is_dir, path)`.
async fn list_tree(target: Target, ts: String) -> Result<Vec<(bool, String)>, String> {
    let mut entries: Vec<(bool, String)> = if target.backend.is_ssh() {
        let args = ssh::probe_command_args(&target, &snapshot::tree_cmd(&target, &ts));
        let out = tokio::process::Command::new("ssh")
            .args(&args)
            .output()
            .await
            .map_err(|e| format!("could not start ssh: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ssh failed (exit {})\n{}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        // ssh `find` yields "<type>\t<path>".
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let (ty, path) = l.split_once('\t')?;
                let path = path.trim();
                if path.is_empty() {
                    return None;
                }
                Some((ty == "d", path.to_string()))
            })
            .collect()
    } else {
        let out = rclone_output(rclone::tree_args(&target, &ts)).await?;
        if !out.status.success() {
            return Err(format!(
                "rclone failed (exit {})\n{}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        // rclone `lsf -R` yields one path per line; directories end with `/`.
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                if l.is_empty() {
                    return None;
                }
                Some((l.ends_with('/'), l.trim_end_matches('/').to_string()))
            })
            .collect()
    };
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(entries)
}

/// An event from a running process stream.
enum Prog {
    Line(String),
    Done(bool, String),
}

/// State for the stream that runs a sequence of commands.
enum Phase {
    Next(std::collections::VecDeque<(String, Vec<String>)>),
    Read {
        rest: std::collections::VecDeque<(String, Vec<String>)>,
        child: tokio::process::Child,
        lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    },
    End,
}

/// Ensures the rsync arguments include `--verbose` (per-file output so the
/// log ticks live).
fn ensure_verbose(args: &mut Vec<String>) {
    if !args.iter().any(|a| a == "--verbose" || a == "-v") {
        args.insert(0, "--verbose".to_string());
    }
}

fn map_prog(p: Prog) -> Message {
    match p {
        Prog::Line(l) => Message::ProgressLine(l),
        Prog::Done(ok, err) => Message::ProgressDone(ok, err),
    }
}

/// Runs a sequence of commands and streams their stdout line by line.
/// Stops (with an error) if a command fails; otherwise `Done(true)` at the end.
fn run_stream(cmds: Vec<(String, Vec<String>)>) -> impl iced::futures::Stream<Item = Prog> {
    let queue: std::collections::VecDeque<_> = cmds.into_iter().collect();
    iced::futures::stream::unfold(Phase::Next(queue), |mut phase| async move {
        loop {
            match phase {
                Phase::End => return None,
                Phase::Next(mut queue) => match queue.pop_front() {
                    None => return Some((Prog::Done(true, String::new()), Phase::End)),
                    Some((prog, args)) => {
                        let spawn = tokio::process::Command::new(&prog)
                            .args(&args)
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn();
                        match spawn {
                            Ok(mut child) => {
                                let stdout = child.stdout.take().expect("stdout piped");
                                let lines = BufReader::new(stdout).lines();
                                let echo = format!("$ {prog} {}", rsync::render(&args));
                                return Some((
                                    Prog::Line(echo),
                                    Phase::Read {
                                        rest: queue,
                                        child,
                                        lines,
                                    },
                                ));
                            }
                            Err(e) => {
                                return Some((
                                    Prog::Done(false, format!("could not start {prog}: {e}")),
                                    Phase::End,
                                ));
                            }
                        }
                    }
                },
                Phase::Read {
                    rest,
                    mut child,
                    mut lines,
                } => match lines.next_line().await {
                    Ok(Some(line)) => {
                        return Some((
                            Prog::Line(line),
                            Phase::Read { rest, child, lines },
                        ));
                    }
                    _ => {
                        // stdout exhausted → read any error and check status.
                        let mut err = String::new();
                        if let Some(mut e) = child.stderr.take() {
                            let _ = e.read_to_string(&mut err).await;
                        }
                        let ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
                        if !ok {
                            return Some((Prog::Done(false, err), Phase::End));
                        }
                        phase = Phase::Next(rest); // next command in the sequence
                    }
                },
            }
        }
    })
}

/// Lists snapshots, plans according to retention and deletes the older ones (via ssh).
async fn prune_now(target: Target, policy: Retention) -> Result<String, String> {
    let snaps = list_snapshots(target.clone()).await?;
    let plan = prune::plan(&snaps, &policy);
    if plan.delete.is_empty() {
        return Ok(format!("Nothing to prune ({} kept)", plan.keep.len()));
    }

    if target.backend.is_ssh() {
        let dargs = ssh::probe_command_args(&target, &snapshot::prune_cmd(&target, &plan.delete));
        let dout = tokio::process::Command::new("ssh")
            .args(&dargs)
            .output()
            .await
            .map_err(|e| format!("could not start ssh: {e}"))?;
        if !dout.status.success() {
            return Err(format!(
                "prune failed (exit {})\n{}",
                dout.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&dout.stderr)
            ));
        }
    } else {
        for ts in &plan.delete {
            let out = rclone_output(rclone::prune_args(&target, ts)).await?;
            if !out.status.success() {
                return Err(format!(
                    "rclone purge failed (exit {})\n{}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    Ok(format!(
        "Pruned {} snapshot(s), kept {} ✓",
        plan.delete.len(),
        plan.keep.len()
    ))
}

/// Runs the verify checks for a target and returns a report with ✓/✗.
async fn verify_target(target: Target) -> String {
    fn line(ok: bool, msg: &str) -> String {
        format!("  {} {msg}\n", if ok { "✓" } else { "✗" })
    }

    let mut out = String::new();

    // rclone backend: sources locally + remote reachability.
    if !target.backend.is_ssh() {
        for s in &target.sources {
            let p = moraine::config::expand_tilde(s);
            out.push_str(&line(p.exists(), &format!("source {}", p.display())));
        }
        match rclone_output(rclone::list_args(&target)).await {
            Ok(o) if o.status.success() => {
                out.push_str(&line(true, &format!("rclone reachable: {}", rclone::base(&target))))
            }
            Ok(_) => out.push_str(&format!(
                "  · rclone base empty or new: {}\n",
                rclone::base(&target)
            )),
            Err(e) => out.push_str(&line(false, &e)),
        }
        return out;
    }

    // SSH key (local)
    match target.key_path() {
        Some(k) if k.exists() => out.push_str(&line(true, &format!("SSH key: {}", k.display()))),
        Some(k) => out.push_str(&line(false, &format!("SSH key missing: {}", k.display()))),
        None => out.push_str("  · no key set (using ssh-agent)\n"),
    }

    // Sources (local)
    for s in &target.sources {
        let p = moraine::config::expand_tilde(s);
        out.push_str(&line(p.exists(), &format!("source {}", p.display())));
    }

    // SSH connection
    let probe = ssh::probe_command_args(&target, "echo connection-ok");
    match tokio::process::Command::new("ssh")
        .args(&probe)
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            out.push_str(&line(true, "SSH connection"));

            // Dest writable? (remote)
            let dprobe = ssh::probe_command_args(&target, &snapshot::dest_check_cmd(&target));
            match tokio::process::Command::new("ssh")
                .args(&dprobe)
                .output()
                .await
            {
                Ok(d) if d.status.success() => {
                    let (ok, msg) = match String::from_utf8_lossy(&d.stdout).trim() {
                        "writable" => (true, format!("dest writable: {}", target.dest)),
                        "parent-writable" => {
                            (true, format!("dest will be created: {}", target.dest))
                        }
                        "readonly" => (false, format!("dest not writable: {}", target.dest)),
                        other => (false, format!("dest not accessible ({other}): {}", target.dest)),
                    };
                    out.push_str(&line(ok, &msg));
                }
                _ => out.push_str(&line(false, "dest check failed")),
            }
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            out.push_str(&line(
                false,
                &format!(
                    "SSH connection: {}",
                    err.lines().next().unwrap_or("failed").trim()
                ),
            ));
        }
        Err(e) => out.push_str(&line(false, &format!("SSH connection: {e}"))),
    }

    out
}
