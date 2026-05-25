#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use chrono::Local;
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{keyboard, Alignment, Background, Color, Element, Font, Length, Subscription, Task};
use iced_shadcn::{
    button, card, dialog, input, separator, tabs_content, tabs_contents, tabs_list,
    tabs_trigger, ButtonProps, ButtonSize, ButtonVariant, CardProps, CardVariant,
    DialogAlign, DialogProps, InputProps, SeparatorProps, TabsHover, TabsListProps,
    TabsListVariant, TabsRootProps, Theme,
};
use sqlitex::{sqlitex, Connection};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const SUBJECTS: &[&str] = &["Computing", "Math", "General Paper", "Chemistry"];

fn subject_color(sub: &str) -> Color {
    match sub {
        "Computing" => Color::from_rgb8(56, 189, 248),
        "Math" => Color::from_rgb8(248, 113, 113),
        "General Paper" => Color::from_rgb8(250, 204, 21),
        "Chemistry" => Color::from_rgb8(74, 222, 128),
        _ => Color::WHITE,
    }
}

// ── Database Setup ────────────────────────────────────────────────────────────

#[sqlitex("migrations/")]
struct Db {
    add_session: sql!("INSERT INTO study_sessions (subject, duration_seconds, date) VALUES (?, ?, ?)"),
    get_all_sessions: sql!("SELECT id, subject, duration_seconds, date FROM study_sessions ORDER BY id DESC"),
    delete_session: sql!("DELETE FROM study_sessions WHERE id = ?"),
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct StudySession {
    id: u64,
    subject: String,
    duration_seconds: u64,
    date: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ChartMode {
    Today,
    AllTime,
}

fn strong_text<'a>(content: &str) -> iced::widget::Text<'a> {
    text(content.to_string()).font(Font {
        weight: iced::font::Weight::Bold,
        ..Default::default()
    })
}

// For the main display (HH:MM:SS)
fn format_duration(seconds: u64) -> String {
    let hrs = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hrs > 0 {
        format!("{:02}:{:02}:{:02}", hrs, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

// For the labels below the chart (e.g., "1h 15m" or "45m")
fn format_short_duration(seconds: u64) -> String {
    if seconds == 0 {
        return "0s".to_string();
    }
    let hrs = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hrs > 0 {
        format!("{}h {}m", hrs, mins)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

// ── App State & Messages ──────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Message {
    TabSelected(String),
    SetChartMode(ChartMode),

    // Timer Logic
    ToggleTimer,
    Tick(Instant),
    TimerSubjectSelected(String),
    SaveTimerSession,
    DiscardTimerSession,

    // Keyboard global shortcuts
    KeyboardSave,
    KeyboardSpace,

    // Manual Log Dialog Logic
    OpenManualLog,
    CloseDialogs,
    ManualSubjectSelected(String),
    ManualDurationChanged(String),
    SubmitManualLog,

    // History Logic
    DeleteSession(u64),
}

struct StudyApp {
    db: Db,
    theme: Theme,
    active_tab: String,
    chart_mode: ChartMode,
    sessions: Vec<StudySession>,

    // Timer State
    is_timer_running: bool,
    timer_start: Option<Instant>,
    timer_accumulated: Duration,
    timer_subject: String,

    // Manual Log State
    manual_log_open: bool,
    manual_subject: String,
    manual_duration_mins: String,
}

impl StudyApp {
    fn new() -> (Self, Task<Message>) {
        let conn = Connection::open("study_tracker.db").expect("Failed to open local database");
        let mut db = Db::new(conn);
        db.migrate().expect("Failed to run database migrations");

        let mut app = Self {
            db,
            theme: Theme::dark(),
            active_tab: "timer".to_string(),
            chart_mode: ChartMode::Today,
            sessions: Vec::new(),

            is_timer_running: false,
            timer_start: None,
            timer_accumulated: Duration::ZERO,
            timer_subject: SUBJECTS[0].to_string(),

            manual_log_open: false,
            manual_subject: SUBJECTS[0].to_string(),
            manual_duration_mins: String::new(),
        };
        app.sync_db();

        (app, Task::none())
    }

    fn sync_db(&mut self) {
        if let Ok(rows) = self.db.get_all_sessions() {
            if let Ok(all) = rows.all() {
                self.sessions = all
                    .into_iter()
                    .map(|r| StudySession {
                        id: r.id as u64,
                        subject: r.subject,
                        duration_seconds: r.duration_seconds as u64,
                        date: r.date,
                    })
                    .collect();
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let timer_sub = if self.is_timer_running {
            iced::time::every(Duration::from_millis(500)).map(Message::Tick)
        } else {
            Subscription::none()
        };

        // Mac friendly keyboard shortcuts
        let keyboard_sub = iced::event::listen_with(|event, _status, _window| {
            if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
                // Spacebar toggles timer (handled conditionally in update phase)
                if key == keyboard::Key::Named(keyboard::key::Named::Space) {
                    return Some(Message::KeyboardSpace);
                }

                if modifiers.logo() {
                    match key.as_ref() {
                        keyboard::Key::Character(c) => match c.as_ref() {
                            "s" | "S" => Some(Message::KeyboardSave),
                            "l" | "L" => Some(Message::OpenManualLog),
                            // Subject toggles mapped to Cmd+1..4
                            "1" => Some(Message::TimerSubjectSelected(SUBJECTS[0].to_string())),
                            "2" => Some(Message::TimerSubjectSelected(SUBJECTS[1].to_string())),
                            "3" => Some(Message::TimerSubjectSelected(SUBJECTS[2].to_string())),
                            "4" => Some(Message::TimerSubjectSelected(SUBJECTS[3].to_string())),
                            _ => None,
                        },
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        });

        Subscription::batch(vec![timer_sub, keyboard_sub])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => self.active_tab = tab,
            Message::SetChartMode(mode) => self.chart_mode = mode,

            // ── Timer Logic ──
            Message::ToggleTimer => {
                if self.is_timer_running {
                    if let Some(start) = self.timer_start {
                        self.timer_accumulated += start.elapsed();
                    }
                    self.timer_start = None;
                    self.is_timer_running = false;
                } else {
                    self.timer_start = Some(Instant::now());
                    self.is_timer_running = true;
                }
            }
            Message::Tick(_instant) => {}
            Message::TimerSubjectSelected(val) => {
                self.timer_subject = val;
            }
            Message::SaveTimerSession => {
                let mut total_secs = self.timer_accumulated.as_secs();
                if let Some(start) = self.timer_start {
                    total_secs += start.elapsed().as_secs();
                }

                if total_secs > 0 {
                    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
                    let _ = self.db.add_session(
                        &self.timer_subject,
                        total_secs as i64,
                        &today,
                    );
                    self.sync_db();
                }

                self.is_timer_running = false;
                self.timer_start = None;
                self.timer_accumulated = Duration::ZERO;
            }
            Message::DiscardTimerSession => {
                self.is_timer_running = false;
                self.timer_start = None;
                self.timer_accumulated = Duration::ZERO;
            }

            Message::KeyboardSave => {
                if self.manual_log_open {
                    return self.update(Message::SubmitManualLog);
                } else {
                    return self.update(Message::SaveTimerSession);
                }
            }
            Message::KeyboardSpace => {
                // only if the manual log typing dialog is not open!
                if !self.manual_log_open {
                    return self.update(Message::ToggleTimer);
                }
            }

            // ── Manual Log Logic ──
            Message::OpenManualLog => {
                self.manual_log_open = true;
                self.manual_duration_mins.clear();
            }
            Message::CloseDialogs => {
                self.manual_log_open = false;
            }
            Message::ManualSubjectSelected(val) => self.manual_subject = val,
            Message::ManualDurationChanged(val) => {
                if val.is_empty() || val.chars().all(|c| c.is_ascii_digit()) {
                    self.manual_duration_mins = val;
                }
            }
            Message::SubmitManualLog => {
                if let Ok(mins) = self.manual_duration_mins.parse::<u64>() {
                    if mins > 0 {
                        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
                        let _ = self.db.add_session(
                            &self.manual_subject,
                            (mins * 60) as i64,
                            &today,
                        );
                        self.sync_db();
                    }
                }
                self.manual_log_open = false;
            }

            // ── History Logic ──
            Message::DeleteSession(id) => {
                let _ = self.db.delete_session(id as i64);
                self.sync_db();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let theme = &self.theme;

        let list = tabs_list(
            vec![
                tabs_trigger("timer", "Live Timer"),
                tabs_trigger("dashboard", "Dashboard & Stats"),
                tabs_trigger("history", "History"),
            ],
            &self.active_tab,
            Some(Message::TabSelected),
            TabsRootProps::new(),
            TabsListProps::new()
                .variant(TabsListVariant::Pill)
                .transparent_container(true)
                .hover(TabsHover::Soft),
            theme,
        );

        let content = tabs_contents(
            vec![
                tabs_content("timer", self.view_timer(theme)),
                tabs_content("dashboard", self.view_dashboard(theme)),
                tabs_content("history", self.view_history(theme)),
            ],
            &self.active_tab,
        );

        let main_view = column![
            container(list).width(Length::Fill).align_x(Alignment::Center),
            Space::new().height(Length::Fixed(12.0)),
            content,
        ]
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill);

        let mut app_ui: Element<Message> = container(main_view)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_t| iced::widget::container::Style {
                background: Some(Background::Color(Color::from_rgb8(9, 9, 11))),
                text_color: Some(Color::WHITE),
                ..Default::default()
            })
            .into();

        app_ui = dialog(
            app_ui,
            self.manual_log_open,
            self.view_manual_log_dialog(theme),
            Message::CloseDialogs,
            DialogProps::new().align(DialogAlign::Center),
            theme,
        );

        app_ui
    }

    // ── Views ────────────────────────────────────────────────────────────────

    fn view_timer<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let mut total_secs = self.timer_accumulated.as_secs();
        if let Some(start) = self.timer_start {
            total_secs += start.elapsed().as_secs();
        }

        let time_display = text(format_duration(total_secs))
            .size(80)
            .style(move |_| iced::widget::text::Style {
                color: Some(if self.is_timer_running {
                    Color::from_rgb8(34, 197, 94) // Green when running
                } else {
                    Color::WHITE
                }),
            });

        let timer_controls = row![
            button(
                if self.is_timer_running { "Pause (Space)" } else { "Start (Space)" },
                Some(Message::ToggleTimer),
                ButtonProps::new()
                    .variant(if self.is_timer_running { ButtonVariant::Outline } else { ButtonVariant::Solid })
                    .size(ButtonSize::Size3),
                theme
            )
            .width(Length::Fixed(180.0)),

            button(
                "Save (Cmd+S)",
                if total_secs > 0 { Some(Message::SaveTimerSession) } else { None },
                ButtonProps::new().variant(ButtonVariant::Solid).size(ButtonSize::Size3),
                theme
            ),

            button(
                "Discard",
                if total_secs > 0 && !self.is_timer_running { Some(Message::DiscardTimerSession) } else { None },
                ButtonProps::new().variant(ButtonVariant::Destructive).size(ButtonSize::Size3),
                theme
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let mut subject_row = row![].spacing(8);
        for (i, &sub) in SUBJECTS.iter().enumerate() {
            let is_selected = self.timer_subject == sub;
            let variant = if is_selected { ButtonVariant::Solid } else { ButtonVariant::Outline };

            subject_row = subject_row.push(
                button(
                    format!("{} (Cmd+{})", sub, i + 1), // Cmd+1, Cmd+2, etc
                    Some(Message::TimerSubjectSelected(sub.to_string())),
                    ButtonProps::new().variant(variant).size(ButtonSize::Size1),
                    theme
                )
            );
        }

        let card_content = column![
            strong_text("Study Session").size(24),
            Space::new().height(Length::Fixed(10.0)),
            text("Select your subject:").size(14).style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) }),
            Space::new().height(Length::Fixed(4.0)),
            subject_row,
            Space::new().height(Length::Fixed(20.0)),
            time_display,
            Space::new().height(Length::Fixed(30.0)),
            timer_controls,
            Space::new().height(Length::Fixed(16.0)),
            text("Press Cmd+L anywhere to log past sessions manually").size(12).style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) }),
        ]
        .align_x(Alignment::Center)
        .padding(40);

        container(
            card(card_content, CardProps::new().variant(CardVariant::Surface), theme)
                .width(Length::Fixed(700.0))
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
    }

    fn view_dashboard<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let today_str = Local::now().date_naive().format("%Y-%m-%d").to_string();

        let today_sessions: Vec<_> = self.sessions.iter().filter(|s| s.date == today_str).collect();
        let today_total_secs: u64 = today_sessions.iter().map(|s| s.duration_seconds).sum();
        let unique_subjects_today: HashSet<_> = today_sessions.iter().map(|s| &s.subject).collect();
        let subjects_studied_today_count = unique_subjects_today.len();

        let stats_row = row![
            card(
                column![
                    text("Subjects Studied Today").size(14).style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) }),
                    strong_text(&subjects_studied_today_count.to_string()).size(32)
                ].spacing(8).padding(16),
                CardProps::new().variant(CardVariant::Surface),
                theme
            ).width(Length::FillPortion(1)),
            card(
                column![
                    text("Time Studied Today").size(14).style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) }),
                    strong_text(&format_duration(today_total_secs)).size(32)
                ].spacing(8).padding(16),
                CardProps::new().variant(CardVariant::Surface),
                theme
            ).width(Length::FillPortion(1)),
        ].spacing(16).width(Length::Fill);

        let relevant_sessions: Vec<_> = if self.chart_mode == ChartMode::Today {
            self.sessions.iter().filter(|s| s.date == today_str).collect()
        } else {
            self.sessions.iter().collect()
        };

        let mut subject_totals: HashMap<String, u64> = HashMap::new();
        for session in &relevant_sessions {
            *subject_totals.entry(session.subject.clone()).or_insert(0) += session.duration_seconds;
        }

        let mode_toggle = row![
            button("Today", Some(Message::SetChartMode(ChartMode::Today)),
                ButtonProps::new().variant(if self.chart_mode == ChartMode::Today { ButtonVariant::Solid } else { ButtonVariant::Ghost }).size(ButtonSize::Size1), theme),
            button("All Time", Some(Message::SetChartMode(ChartMode::AllTime)),
                ButtonProps::new().variant(if self.chart_mode == ChartMode::AllTime { ButtonVariant::Solid } else { ButtonVariant::Ghost }).size(ButtonSize::Size1), theme),
        ].spacing(4);

        let mut breakdown_col = column![
            row![
                strong_text("Time by Subject").size(18),
                Space::new().width(Length::Fill),
                mode_toggle
            ].align_y(Alignment::Center),
            separator(SeparatorProps::new(), theme),
        ].spacing(16);

        if subject_totals.is_empty() {
            breakdown_col = breakdown_col.push(
                text("No data to show. Go track some study time!").style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) })
            );
        } else {
            // -- Custom Vertical Bar Chart logic --
            let max_duration = subject_totals.values().copied().max().unwrap_or(0).max(1) as f32;
            let mut chart_row = row![].spacing(16).align_y(Alignment::End).height(Length::Fixed(250.0));

            for &sub in SUBJECTS {
                let secs = *subject_totals.get(sub).unwrap_or(&0);
                let height_pct = if max_duration == 0.0 { 0.0 } else { secs as f32 / max_duration };

                // Cap the visual bar height at 180px
                let bar_height = 180.0 * height_pct;

                let bar = container(
                    Space::new()
                        .width(Length::Fixed(60.0))
                        .height(Length::Fixed(bar_height))
                )
                .style(move |_| iced::widget::container::Style {
                    background: Some(Background::Color(if secs > 0 { subject_color(sub) } else { Color::TRANSPARENT })),
                    border: iced::border::Border {
                        radius: iced::border::Radius {
                            top_left: 4.0,
                            top_right: 4.0,
                            bottom_right: 0.0,
                            bottom_left: 0.0,
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                });

                let column_entry = column![
                    bar,
                    Space::new().width(Length::Fixed(1.0)).height(Length::Fixed(8.0)), // gap between bar and text
                    strong_text(&format_short_duration(secs)).size(14),
                    text(sub).size(12).style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) }),
                ]
                .align_x(Alignment::Center)
                .width(Length::FillPortion(1));

                chart_row = chart_row.push(column_entry);
            }

            breakdown_col = breakdown_col.push(chart_row);
        }

        let breakdown_card = card(
            breakdown_col.padding(24),
            CardProps::new().variant(CardVariant::Surface),
            theme
        ).width(Length::Fill);

        scrollable(
            column![
                stats_row,
                breakdown_card
            ].spacing(24).max_width(900.0)
        ).into()
    }

    fn view_history<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        if self.sessions.is_empty() {
            return container(
                text("No study sessions logged yet.")
                    .style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) })
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();
        }

        let mut history_col = column![].spacing(12);

        for session in &self.sessions {
            let id = session.id;
            let dot_color = subject_color(&session.subject);

            let row_content = row![
                text("●").color(dot_color).size(18),
                Space::new().width(Length::Fixed(12.0)),
                column![
                    strong_text(&session.subject).size(16),
                    text(&session.date)
                        .size(12)
                        .style(move |_| iced::widget::text::Style { color: Some(theme.palette.muted_foreground) })
                ].spacing(4).width(Length::Fill),

                strong_text(&format_duration(session.duration_seconds))
                    .size(16)
                    .style(move |_| iced::widget::text::Style { color: Some(theme.palette.foreground) }),

                Space::new().width(Length::Fixed(24.0)),

                button(
                    "X",
                    Some(Message::DeleteSession(id)),
                    ButtonProps::new().variant(ButtonVariant::Ghost).size(ButtonSize::Size1),
                    theme
                )
            ]
            .align_y(Alignment::Center)
            .padding(16);

            history_col = history_col.push(
                card(row_content, CardProps::new().variant(CardVariant::Surface), theme)
            );
        }

        scrollable(
            container(history_col).max_width(800.0)
        ).into()
    }

    // ── Dialogs ────────────────────────────────────────────────────────────────

    fn view_manual_log_dialog<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let mut subject_row = row![].spacing(8);
        for &sub in SUBJECTS {
            let is_selected = self.manual_subject == sub;
            let variant = if is_selected { ButtonVariant::Solid } else { ButtonVariant::Outline };

            subject_row = subject_row.push(
                button(
                    sub,
                    Some(Message::ManualSubjectSelected(sub.to_string())),
                    ButtonProps::new().variant(variant).size(ButtonSize::Size1),
                    theme
                )
            );
        }

        let form = column![
            column![
                text("Subject").size(14),
                scrollable(subject_row).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new())),
            ].spacing(6),

            column![
                text("Duration (in minutes)").size(14),
                input(
                    &self.manual_duration_mins,
                    "e.g. 45",
                    Some(Message::ManualDurationChanged),
                    InputProps::new(),
                    theme
                ),
            ].spacing(6),

            row![
                Space::new().width(Length::Fill),
                button(
                    "Cancel",
                    Some(Message::CloseDialogs),
                    ButtonProps::new().variant(ButtonVariant::Outline),
                    theme
                ),
                button(
                    "Log Session (Cmd+S)",
                    if self.manual_duration_mins.is_empty() { None } else { Some(Message::SubmitManualLog) },
                    ButtonProps::new().variant(ButtonVariant::Solid),
                    theme
                ),
            ].spacing(8)
        ].spacing(24);

        container(column![strong_text("Log Past Session").size(18), form].spacing(16))
            .width(Length::Fixed(500.0))
            .padding(24)
            .style(move |_| iced::widget::container::Style {
                background: Some(Background::Color(theme.palette.card)),
                border: iced::border::Border { radius: 8.0.into(), width: 1.0, color: theme.palette.border },
                ..Default::default()
            })
            .into()
    }
}

pub fn main() -> iced::Result {
    iced::application(StudyApp::new, StudyApp::update, StudyApp::view)
        .title("Study Tracker")
        .subscription(StudyApp::subscription)
        .theme(|_app: &StudyApp| iced::Theme::Dark)
        .window(iced::window::Settings {
            decorations: false,
            ..Default::default()
        })
        .run()
}