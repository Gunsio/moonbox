use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

use crate::{
    app::{
        App, CommandPaletteEntry, DATA_SPACE_CONFIG_FIELD_COUNT, EnterRouteKind, Focus,
        HandoffTrailFrame, HookWaitingItem, SessionFilter, SettingsField,
    },
    core::image_preview::{ImagePreviewStatus, PreviewCell, PreviewRgb, TimelineImagePreview},
    core::model::{
        AnatomyMetric, CliTool, CompilerPresetInfo, CompilerPresetKind, CompilerPresetStatus,
        LaunchValidationState, SessionAnatomyStatus, SessionRuntimeStatus, SessionStatus,
        SourceAdapterReport, SourceFidelityStatus, SourceProvenance, TimelineAttachment,
        TimelineEvent, TimelineKind, VerificationReport, VerificationStatus, WorkCapsule,
    },
    core::{compiler, handoff, hooks},
};

use super::{
    i18n::{self, Text},
    theme,
};

pub fn render(frame: &mut Frame, app: &App) {
    let _theme = theme::use_current(app.effective_theme());
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme::text())),
        area,
    );

    let root = centered(area, 98, 96);
    let header_height = if root.width < 120 { 4 } else { 3 };
    let command_height = if root.width < 120 { 5 } else { 3 };
    let zoomed_action_path = app.zoomed_focus == Some(Focus::Branches);
    let branch_height = if zoomed_action_path {
        root.height
            .saturating_sub(header_height + command_height + 8)
            .max(8)
    } else if root.height < 32 {
        3
    } else {
        4
    };
    let body_min = if zoomed_action_path {
        6
    } else if root.height < 32 {
        8
    } else {
        18
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(body_min),
            Constraint::Length(branch_height),
            Constraint::Length(command_height),
        ])
        .split(root);

    render_header(frame, chunks[0], app);
    render_body(frame, chunks[1], app);
    render_branch_tree(frame, chunks[2], app);
    render_command_bar(frame, chunks[3], app);

    if app.show_help {
        render_help(frame, root, app);
    }
    if app.show_launch {
        render_launch(frame, root, app);
    }
    if app.show_open_original {
        render_open_original(frame, root, app);
    }
    if app.show_doctor {
        render_doctor(frame, root, app);
    }
    if app.show_capsules {
        render_capsules(frame, root, app);
    }
    if app.show_settings {
        render_settings(frame, root, app);
    }
    if app.show_data_space_config {
        render_data_space_config(frame, root, app);
    } else if app.show_data_spaces {
        render_data_spaces(frame, root, app);
    }
    if app.show_timeline_detail {
        render_timeline_detail(frame, root, app);
    }
    if app.show_skill_picker {
        render_skill_picker(frame, root, app);
    }
    if app.command_mode && !app.command_input.starts_with('/') {
        render_command_palette(frame, root, app);
    }
}

pub fn render_loading(frame: &mut Frame, tick: usize) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme::text())),
        area,
    );
    let root = centered(area, 52, 32);
    let spinner = ["|", "/", "-", "\\"][tick % 4];
    let lines = vec![
        Line::from(vec![Span::styled(
            " MOONBOX ",
            Style::default()
                .fg(theme::text())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(spinner, Style::default().fg(theme::gold())),
            Span::raw(" indexing source sessions"),
        ]),
        Line::from(vec![
            Span::raw("   bounded scan "),
            Span::styled("active", Style::default().fg(theme::green())),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("q", Style::default().fg(theme::blue())),
            Span::raw(" quit   "),
            Span::styled("ctrl-c", Style::default().fg(theme::blue())),
            Span::raw(" quit"),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Loading ", true))
            .alignment(Alignment::Left),
        root,
    );
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let language = app.effective_language();
    let preflight = preflight_summary(app);

    let title = Line::from(header_title_spans(area.width, language));
    let state = Line::from(vec![
        Span::raw(i18n::text(language, Text::Filter)),
        Span::raw(" "),
        Span::styled("[ ]", Style::default().fg(theme::muted())),
        Span::raw(": "),
        Span::styled(
            filter_label(app),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("   {}: ", i18n::text(language, Text::Data))),
        Span::styled(data_space_header_label(app), data_space_header_style(app)),
        Span::raw(format!("   {}: ", i18n::text(language, Text::Skill))),
        Span::styled(
            selected_skill_label(app),
            Style::default().fg(theme::cyan()),
        ),
    ]);
    let token_budget = app
        .current_session()
        .map(|session| format_token_count(session.token_count))
        .unwrap_or_else(|| "-".into());
    let budget = Line::from(vec![
        Span::raw(format!("{}: ", i18n::text(language, Text::Tokens))),
        Span::styled(
            token_budget,
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("   {}: ", i18n::text(language, Text::Preflight))),
        Span::styled(
            preflight.status.label(),
            Style::default()
                .fg(preflight.status.color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            preflight.confidence.label(),
            Style::default()
                .fg(preflight.confidence.color())
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let lines = if area.width < 120 {
        vec![title, state, budget]
    } else {
        vec![Line::from(
            title
                .spans
                .into_iter()
                .chain([Span::raw("   ")])
                .chain(state.spans)
                .chain([Span::raw("   ")])
                .chain(budget.spans)
                .collect::<Vec<_>>(),
        )]
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(dynamic_panel_block(
                format!(" {} ", i18n::text(language, Text::MoonboxCli)),
                app.focus == Focus::Branches,
            ))
            .alignment(Alignment::Left),
        area,
    );
}

fn selected_skill_label(app: &App) -> String {
    app.data
        .compilers
        .get(app.selected_compiler)
        .and_then(|compiler_id| handoff::parse_compiler_id(compiler_id))
        .map(|spec| spec.skill_id)
        .or_else(|| app.data.compilers.get(app.selected_compiler).cloned())
        .unwrap_or_else(|| app.data.capsule.compiler.clone())
}

fn data_space_header_label(app: &App) -> String {
    let space = app.current_data_space();
    if space.is_local() {
        i18n::text(app.effective_language(), Text::LocalDataSpace).to_string()
    } else {
        format!("SSH: {}", space.label)
    }
}

fn data_space_header_style(app: &App) -> Style {
    let color = if app.current_data_space().is_local() {
        theme::cyan()
    } else {
        theme::orange()
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn header_title_spans(width: u16, language: crate::core::config::UiLanguage) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        " MOONBOX ",
        Style::default()
            .fg(theme::text())
            .add_modifier(Modifier::BOLD),
    )];
    if width >= 120 && language == crate::core::config::UiLanguage::ZhHans {
        spans.push(Span::styled(
            "月光宝盒",
            Style::default().fg(theme::muted()),
        ));
    }
    spans
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreflightStatus {
    Pass,
    Warn,
    Blocked,
}

impl PreflightStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Blocked => "BLOCKED",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Pass => theme::green(),
            Self::Warn => theme::gold(),
            Self::Blocked => theme::red(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreflightConfidence {
    Strong,
    Medium,
    Weak,
}

impl PreflightConfidence {
    fn label(self) -> &'static str {
        match self {
            Self::Strong => "Strong",
            Self::Medium => "Medium",
            Self::Weak => "Weak",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Strong => theme::confidence_strong(),
            Self::Medium => theme::confidence_medium(),
            Self::Weak => theme::confidence_weak(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreflightSummary {
    status: PreflightStatus,
    confidence: PreflightConfidence,
    compiler_status: VerificationStatus,
    verify_status: VerificationStatus,
    verify_reviewed: bool,
}

fn preflight_summary(app: &App) -> PreflightSummary {
    let verification = app.launch_verification_for_target(app.data.target);
    let compiler_status = compiler_preflight_status(app.compile_status);
    let verify_status = if app.verify_passed {
        verification
            .as_ref()
            .map(|report| report.status)
            .unwrap_or(VerificationStatus::Fail)
    } else {
        VerificationStatus::Fail
    };
    let status = if compiler_status == VerificationStatus::Fail
        || app.doctor_report.status == VerificationStatus::Fail
        || verify_status == VerificationStatus::Fail
    {
        PreflightStatus::Blocked
    } else if compiler_status == VerificationStatus::Warn
        || app.doctor_report.status == VerificationStatus::Warn
        || verify_status == VerificationStatus::Warn
    {
        PreflightStatus::Warn
    } else {
        PreflightStatus::Pass
    };
    let confidence =
        if verification.is_none() || !app.verify_passed || app.compile_status == "LOADING" {
            PreflightConfidence::Weak
        } else if status == PreflightStatus::Pass {
            PreflightConfidence::Strong
        } else {
            PreflightConfidence::Medium
        };
    PreflightSummary {
        status,
        confidence,
        compiler_status,
        verify_status,
        verify_reviewed: app.verify_passed,
    }
}

fn compiler_preflight_status(status: &str) -> VerificationStatus {
    match status {
        "FAILED" => VerificationStatus::Fail,
        "LOADING" => VerificationStatus::Warn,
        _ => VerificationStatus::Pass,
    }
}

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
    if let Some(focus) = app.zoomed_focus {
        match focus {
            Focus::Sessions => render_sessions(frame, area, app),
            Focus::Timeline => render_timeline(frame, area, app),
            Focus::Capsule => render_capsule(frame, area, app),
            Focus::Branches => {
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(25),
                        Constraint::Percentage(45),
                        Constraint::Percentage(30),
                    ])
                    .split(area);
                render_sessions(frame, cols[0], app);
                render_timeline(frame, cols[1], app);
                render_capsule(frame, cols[2], app);
            }
        }
        return;
    }
    let waiting = app.hook_waiting_items();
    let area = if waiting.is_empty() || area.height < 14 {
        area
    } else {
        let queue_height = (waiting.len() as u16).saturating_add(2).clamp(3, 5);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(queue_height), Constraint::Min(8)])
            .split(area);
        render_waiting_queue(frame, rows[0], &waiting);
        rows[1]
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(45),
            Constraint::Percentage(30),
        ])
        .split(area);

    render_sessions(frame, cols[0], app);
    render_timeline(frame, cols[1], app);
    render_capsule(frame, cols[2], app);
}

fn render_waiting_queue(frame: &mut Frame, area: Rect, waiting: &[HookWaitingItem]) {
    let capacity = usize::from(area.height.saturating_sub(2)).max(1);
    let lines = waiting
        .iter()
        .take(capacity)
        .map(|item| {
            let mut spans = vec![
                Span::styled(source_pill(item.cli), source_tool_style(item.cli)),
                Span::raw("  "),
                Span::styled(
                    hooks::age_label_ms(item.waiting_for_ms),
                    Style::default()
                        .fg(theme::gold())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    review_snippet(&item.reason, 44),
                    Style::default().fg(theme::text()),
                ),
                Span::styled("  ·  ", Style::default().fg(theme::border())),
                Span::styled(
                    review_snippet(&item.title, 36),
                    Style::default().fg(theme::muted()),
                ),
            ];
            if let Some(pane) = &item.tmux_pane {
                spans.push(Span::styled(
                    "  pane ",
                    Style::default().fg(theme::border()),
                ));
                spans.push(Span::styled(
                    pane.clone(),
                    Style::default().fg(theme::cyan()),
                ));
            }
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(lines).block(dynamic_panel_block(" WAITING ON YOU ".into(), false)),
        area,
    );
}

fn render_sessions(frame: &mut Frame, area: Rect, app: &App) {
    let language = app.effective_language();
    let visible = app.visible_session_indices();
    let selected = visible
        .iter()
        .position(|index| *index == app.selected_session)
        .unwrap_or(0);
    let items: Vec<ListItem> = if visible.is_empty() {
        let mut lines = vec![
            Line::from(Span::styled(
                i18n::text(language, Text::NoSessionsMatch),
                Style::default().fg(theme::muted()),
            )),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Filter)),
                    Style::default().fg(theme::muted()),
                ),
                Span::styled(
                    session_filter_label(language, app.session_filter),
                    Style::default().fg(theme::cyan()),
                ),
            ]),
        ];
        if !app.search_query.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Query)),
                    Style::default().fg(theme::muted()),
                ),
                Span::styled(
                    format!("/{}", app.search_query),
                    Style::default().fg(theme::cyan()),
                ),
            ]));
        }
        lines.push(Line::from(Span::styled(
            i18n::text(language, Text::PressAToClear),
            Style::default().fg(theme::muted()),
        )));
        vec![ListItem::new(lines)]
    } else {
        let (start, end) = session_list_window(visible.len(), selected, area.height);
        visible[start..end]
            .iter()
            .map(|index| {
                let session = &app.data.sessions[*index];
                let selected_row = *index == app.selected_session;
                let selector = if selected_row {
                    Span::styled(
                        "▸",
                        Style::default()
                            .fg(theme::text())
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw(" ")
                };
                let mut title_spans = vec![selector, Span::raw(" ")];
                for marker in session_row_markers(session, app.is_session_starred(session)) {
                    title_spans.push(marker);
                    title_spans.push(Span::raw(" "));
                }
                title_spans.extend([
                    Span::styled(source_pill(session.cli), source_tool_style(session.cli)),
                    Span::raw("  "),
                ]);
                if let Some(live) = app.hook_live_for_session(session) {
                    title_spans.push(session_live_badge(live));
                    title_spans.push(Span::raw("  "));
                }
                title_spans.push(Span::styled(
                    &session.title,
                    session_title_style(selected_row),
                ));
                ListItem::new(vec![
                    Line::from(title_spans),
                    session_list_secondary_line(app, session, selected_row, area.width),
                ])
            })
            .collect()
    };

    let mut state = ListState::default();
    if !visible.is_empty() {
        let (start, _) = session_list_window(visible.len(), selected, area.height);
        state.select(Some(selected.saturating_sub(start)));
    }

    let title = if app.search_query.is_empty() {
        stable_panel_title(
            format!(
                "{} · {} {}",
                i18n::text(language, Text::SessionsTitle),
                session_filter_label(language, app.session_filter),
                session_position_label(visible.len(), selected)
            ),
            area,
        )
    } else if area.width < 28 {
        stable_panel_title(
            format!(
                "{} /{}",
                i18n::text(language, Text::SessionsTitle),
                app.search_query
            ),
            area,
        )
    } else {
        stable_panel_title(
            format!(
                "{} · {} {}",
                i18n::text(language, Text::SessionsTitle),
                filter_label(app),
                session_position_label(visible.len(), selected)
            ),
            area,
        )
    };

    let list = List::new(items)
        .block(dynamic_panel_block(title, app.focus == Focus::Sessions))
        .highlight_symbol("")
        .highlight_style(
            Style::default()
                .fg(theme::text())
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

fn compile_status_label(status: &str) -> String {
    format!("{status:<8}")
}

fn session_title_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme::text())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    }
}

fn session_row_markers(
    session: &crate::core::model::SessionSummary,
    starred: bool,
) -> Vec<Span<'static>> {
    let mut markers = Vec::with_capacity(2);
    if starred {
        markers.push(Span::styled(
            "*",
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        ));
    }
    match session.status {
        SessionStatus::Warning => {
            markers.push(Span::styled("▲", Style::default().fg(theme::gold())))
        }
        SessionStatus::Failed => markers.push(Span::styled(
            "!",
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        )),
        SessionStatus::Healthy => {}
    }
    markers
}

fn session_list_window(total: usize, selected: usize, area_height: u16) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let inner_height = usize::from(area_height.saturating_sub(2)).max(1);
    let visible_items = (inner_height / 2).max(1);
    let capacity = (visible_items + 4).min(total);
    let mut start = selected.saturating_sub(capacity / 2);
    let end = (start + capacity).min(total);
    start = end.saturating_sub(capacity);
    (start, end)
}

fn session_list_secondary_line(
    app: &App,
    session: &crate::core::model::SessionSummary,
    selected: bool,
    width: u16,
) -> Line<'static> {
    let updated = relative_time_label(&session.updated_at, current_unix_timestamp())
        .unwrap_or_else(|| session.updated.clone());
    let mut spans = vec![Span::raw("    ")];
    let metric_style = if selected {
        Style::default().fg(theme::cyan())
    } else {
        Style::default().fg(theme::muted())
    };
    spans.push(Span::styled(
        session_inventory_metric(session),
        metric_style,
    ));
    spans.push(Span::styled(" · ", Style::default().fg(theme::border())));
    spans.push(Span::styled(updated, Style::default().fg(theme::muted())));
    if width >= 34 && (app.hooks_enabled() || app.smart_enter_tmux_enabled()) {
        let route = app.enter_route_preview(session);
        spans.push(Span::styled(" · ", Style::default().fg(theme::border())));
        spans.push(Span::styled("Enter ", Style::default().fg(theme::border())));
        spans.push(Span::styled(route.label, enter_route_style(route.kind)));
    }
    if width >= 52
        && let Some(live) = app.hook_live_for_session(session)
    {
        let max_live = usize::from(width.saturating_sub(28)).clamp(10, 34);
        spans.push(Span::styled(" · ", Style::default().fg(theme::border())));
        spans.push(Span::styled(
            review_snippet(&live.summary, max_live),
            session_live_text_style(live.status),
        ));
    }
    if width >= 60
        && let Some(branch) = session
            .branch
            .as_deref()
            .filter(|branch| !branch.is_empty())
    {
        let max_branch = usize::from(width.saturating_sub(34)).clamp(8, 28);
        spans.push(Span::styled(" · ", Style::default().fg(theme::border())));
        spans.push(Span::styled(
            review_snippet(branch, max_branch),
            Style::default().fg(theme::muted()),
        ));
    }
    Line::from(spans)
}

fn session_live_badge(live: &hooks::HookSessionLiveInfo) -> Span<'static> {
    Span::styled(
        format!(" {} ", live.status.label()),
        session_live_badge_style(live.status),
    )
}

fn session_live_badge_style(status: hooks::HookSessionStatus) -> Style {
    match status {
        hooks::HookSessionStatus::Running => Style::default().fg(theme::green()),
        hooks::HookSessionStatus::Waiting => Style::default()
            .fg(theme::gold())
            .add_modifier(Modifier::BOLD),
        hooks::HookSessionStatus::Idle => Style::default().fg(theme::muted()),
        hooks::HookSessionStatus::Dead => Style::default().fg(theme::red()),
    }
}

fn session_live_text_style(status: hooks::HookSessionStatus) -> Style {
    match status {
        hooks::HookSessionStatus::Running => Style::default().fg(theme::green()),
        hooks::HookSessionStatus::Waiting => Style::default().fg(theme::gold()),
        hooks::HookSessionStatus::Idle => Style::default().fg(theme::muted()),
        hooks::HookSessionStatus::Dead => Style::default().fg(theme::red()),
    }
}

#[cfg(test)]
fn session_list_secondary_at(
    session: &crate::core::model::SessionSummary,
    now_unix_seconds: i64,
) -> String {
    let updated = relative_time_label(&session.updated_at, now_unix_seconds)
        .unwrap_or_else(|| session.updated.clone());
    let metric = session_inventory_metric(session);
    match session
        .branch
        .as_deref()
        .filter(|branch| !branch.is_empty())
    {
        Some(branch) => format!("    {metric}  ·  {updated}  ·  {branch}"),
        None => format!("    {metric}  ·  {updated}"),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SessionShapeCounts {
    user: usize,
    assistant: usize,
    tool: usize,
    rewind: usize,
}

fn hydrated_session_shape(
    app: &App,
    session: &crate::core::model::SessionSummary,
) -> Option<SessionShapeCounts> {
    if app.data.capsule.source_session != session.id || app.data.capsule.source_cli != session.cli {
        return None;
    }
    (!app.data.timeline.is_empty()).then(|| timeline_shape_counts(&app.data.timeline))
}

fn timeline_shape_counts(events: &[TimelineEvent]) -> SessionShapeCounts {
    let mut counts = SessionShapeCounts::default();
    for event in events {
        match event.kind {
            TimelineKind::User => counts.user += 1,
            TimelineKind::Assistant => counts.assistant += 1,
            TimelineKind::Tool
            | TimelineKind::Compact
            | TimelineKind::Error
            | TimelineKind::GitDiff => counts.tool += 1,
            TimelineKind::RewindPoint => counts.rewind += 1,
        }
    }
    counts
}

fn session_shape_count_text(counts: SessionShapeCounts) -> String {
    format!(
        "user {} / assistant {} / tool {} / rewind {}",
        counts.user, counts.assistant, counts.tool, counts.rewind
    )
}

fn session_inventory_metric(session: &crate::core::model::SessionSummary) -> String {
    let mut parts = Vec::new();
    if let Some(tokens) = session.token_count {
        parts.push(format!("{} tokens", format_token_count(Some(tokens))));
    }
    if let Some(bytes) = session.source_size_bytes {
        parts.push(format!("{} source", format_source_size(bytes)));
    }
    if parts.is_empty() {
        parts.push(if session.event_count > 0 {
            "timeline indexed".into()
        } else {
            "size unknown".into()
        });
    }
    parts.join(" · ")
}

fn session_portrait_detail(app: &App, session: &crate::core::model::SessionSummary) -> String {
    if let Some(counts) = hydrated_session_shape(app, session) {
        format!("{} · cached timeline", session_shape_count_text(counts),)
    } else {
        format!(
            "{} · indexed summary only",
            session_inventory_metric(session)
        )
    }
}

fn session_portrait_summary(app: &App, session: &crate::core::model::SessionSummary) -> String {
    if let Some(counts) = hydrated_session_shape(app, session) {
        session_shape_count_text(counts)
    } else {
        session_inventory_metric(session)
    }
}

fn current_unix_timestamp() -> i64 {
    if let Ok(value) = std::env::var("MOONBOX_TUI_NOW_UNIX")
        && let Ok(timestamp) = value.parse()
    {
        return timestamp;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn relative_time_label(timestamp: &str, now_unix_seconds: i64) -> Option<String> {
    let timestamp_unix_seconds = parse_session_timestamp(timestamp)?;
    let elapsed = now_unix_seconds
        .saturating_sub(timestamp_unix_seconds)
        .max(0);
    Some(match elapsed {
        0..=59 => format!("{elapsed}s ago"),
        60..=3_599 => format!("{}m ago", elapsed / 60),
        3_600..=86_399 => format!("{}h ago", elapsed / 3_600),
        86_400..=604_799 => format!("{}d ago", elapsed / 86_400),
        604_800..=2_591_999 => format!("{}w ago", elapsed / 604_800),
        2_592_000..=31_535_999 => format!("{}mo ago", elapsed / 2_592_000),
        _ => format!("{}y ago", elapsed / 31_536_000),
    })
}

fn parse_session_timestamp(timestamp: &str) -> Option<i64> {
    let timestamp = crate::core::local_jsonl::replace_time_dashes(timestamp);
    let date_time = timestamp.get(..19)?;
    let year = parse_i32(date_time.get(0..4)?)?;
    let month = parse_u32(date_time.get(5..7)?)?;
    let day = parse_u32(date_time.get(8..10)?)?;
    let hour = parse_u32(date_time.get(11..13)?)?;
    let minute = parse_u32(date_time.get(14..16)?)?;
    let second = parse_u32(date_time.get(17..19)?)?;
    if !valid_timestamp_parts(month, day, hour, minute, second) {
        return None;
    }
    let offset = parse_timezone_offset(timestamp.get(19..).unwrap_or_default())?;
    let local_seconds = days_from_civil(year, month, day)
        .saturating_mul(86_400)
        .saturating_add(i64::from(hour) * 3_600)
        .saturating_add(i64::from(minute) * 60)
        .saturating_add(i64::from(second));
    Some(local_seconds.saturating_sub(offset))
}

fn parse_timezone_offset(mut suffix: &str) -> Option<i64> {
    if let Some(rest) = suffix.strip_prefix('.') {
        let fraction_len = rest
            .char_indices()
            .find(|(_, ch)| !ch.is_ascii_digit())
            .map(|(index, _)| index)
            .unwrap_or(rest.len());
        suffix = &rest[fraction_len..];
    }
    if suffix.is_empty() || suffix.starts_with('Z') {
        return Some(0);
    }
    let sign = match suffix.as_bytes().first().copied()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hour = parse_u32(suffix.get(1..3)?)?;
    let minute = if suffix.get(3..4) == Some(":") {
        parse_u32(suffix.get(4..6)?)?
    } else {
        parse_u32(suffix.get(3..5)?)?
    };
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(i64::from(sign) * (i64::from(hour) * 3_600 + i64::from(minute) * 60))
}

fn valid_timestamp_parts(month: u32, day: u32, hour: u32, minute: u32, second: u32) -> bool {
    (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour <= 23
        && minute <= 59
        && second <= 60
}

fn parse_i32(value: &str) -> Option<i32> {
    value.parse().ok()
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month = i32::try_from(month).unwrap_or_default();
    let day = i32::try_from(day).unwrap_or_default();
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era) * 146_097 + i64::from(day_of_era) - 719_468
}

fn source_pill(tool: CliTool) -> &'static str {
    match tool {
        CliTool::Codex => "Cdx",
        CliTool::Claude => "Clu",
        CliTool::Hermes => "Hms",
    }
}

fn source_tool_style(tool: CliTool) -> Style {
    Style::default()
        .fg(source_tool_color(tool))
        .add_modifier(Modifier::BOLD)
}

fn enter_route_style(kind: EnterRouteKind) -> Style {
    let color = match kind {
        EnterRouteKind::Jump => theme::green(),
        EnterRouteKind::Handoff => theme::gold(),
        EnterRouteKind::Unavailable => theme::red(),
        EnterRouteKind::Disabled | EnterRouteKind::Resume => theme::muted(),
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn source_tool_color(tool: CliTool) -> Color {
    match tool {
        CliTool::Codex => theme::blue(),
        CliTool::Claude => theme::purple(),
        CliTool::Hermes => theme::orange(),
    }
}

fn session_position_label(total: usize, selected: usize) -> String {
    if total == 0 {
        "(0)".into()
    } else {
        format!("({}/{total})", selected + 1)
    }
}

fn filter_label(app: &App) -> String {
    let language = app.effective_language();
    let filter = session_filter_label(language, app.session_filter);
    if app.search_query.is_empty() {
        filter.to_string()
    } else {
        format!("{filter} · /{}", app.search_query)
    }
}

fn session_filter_label(
    language: crate::core::config::UiLanguage,
    filter: SessionFilter,
) -> &'static str {
    if language == crate::core::config::UiLanguage::English {
        return filter.label();
    }
    match filter {
        SessionFilter::All => "全部",
        SessionFilter::Starred => i18n::text(language, Text::Star),
        SessionFilter::Tool(CliTool::Codex) => "Codex",
        SessionFilter::Tool(CliTool::Claude) => "Claude",
        SessionFilter::Tool(CliTool::Hermes) => "Hermes",
    }
}

fn format_token_count(token_count: Option<usize>) -> String {
    match token_count {
        Some(count) if count >= 1_000 => format!("{}K", count / 1_000),
        Some(count) => count.to_string(),
        None => "-".into(),
    }
}

fn format_source_size_opt(bytes: Option<u64>) -> String {
    bytes.map(format_source_size).unwrap_or_else(|| "-".into())
}

fn format_source_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f64 = bytes as f64;
    if bytes_f64 >= GIB {
        format!("{:.1}GB", bytes_f64 / GIB)
    } else if bytes_f64 >= MIB {
        format!("{:.1}MB", bytes_f64 / MIB)
    } else if bytes_f64 >= KIB {
        format!("{:.1}KB", bytes_f64 / KIB)
    } else {
        format!("{bytes}B")
    }
}

fn session_health_style(status: SessionStatus) -> Style {
    match status {
        SessionStatus::Healthy => Style::default().fg(theme::green()),
        SessionStatus::Warning => Style::default().fg(theme::gold()),
        SessionStatus::Failed => Style::default()
            .fg(theme::red())
            .add_modifier(Modifier::BOLD),
    }
}

fn source_provenance_style(provenance: SourceProvenance) -> Style {
    match provenance {
        SourceProvenance::Real => Style::default()
            .fg(theme::green())
            .add_modifier(Modifier::BOLD),
        SourceProvenance::Fixture => Style::default().fg(theme::blue()),
        SourceProvenance::Missing => Style::default()
            .fg(theme::red())
            .add_modifier(Modifier::BOLD),
    }
}

fn session_health_detail(session: &crate::core::model::SessionSummary) -> String {
    let reason = session.health_reason.as_deref().unwrap_or("ready");
    let status = match session.status {
        SessionStatus::Healthy => "healthy",
        SessionStatus::Warning => "warning",
        SessionStatus::Failed => "failed",
    };
    if session.parse_skip_count == 0 {
        format!("{status} · {reason}")
    } else {
        format!("{status} · skipped {} · {reason}", session.parse_skip_count)
    }
}

fn render_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let language = app.effective_language();
    let visible_groups = visible_timeline_groups(app);
    let selected_group = selected_timeline_group_position(&visible_groups, app.selected_event);
    let mut lines = Vec::new();
    for (group_idx, group) in visible_groups.iter().enumerate() {
        let selected = group_idx == selected_group;
        let active = selected && app.focus == Focus::Timeline;
        let is_rewind = group.is_rewind(&app.rewind_event_id);
        let (label, color) = timeline_group_label(group, is_rewind, app.data.source);
        let accent = timeline_group_accent(color, is_rewind);
        let marker_style = timeline_marker_style(active, selected, is_rewind);
        let marker = if active && is_rewind {
            "▶◆"
        } else if active {
            "▶ "
        } else if is_rewind {
            "◆ "
        } else if selected {
            "● "
        } else {
            "│ "
        };
        let time_style = if active {
            Style::default().fg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        let label_style = if active {
            Style::default()
                .fg(Color::Black)
                .bg(accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        };
        let title_style = if active {
            Style::default()
                .fg(theme::text())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::text())
        };
        let time = timeline_group_time(group);
        lines.push(timeline_header_line(
            TimelineHeader {
                title: timeline_group_title(group),
                time: &time,
                marker,
                label: &label,
                marker_style,
                time_style,
                label_style,
                title_style,
            },
            area.width,
        ));

        let detail_style = timeline_detail_style(active, is_rewind, group.kind());
        for (event_offset, (_, event)) in group.events().enumerate() {
            for (line_index, detail) in timeline_event_detail_lines(event, area.width)
                .into_iter()
                .enumerate()
            {
                let prefix =
                    timeline_detail_prefix(active, group.is_ai_group(), event_offset, line_index);
                lines.push(Line::from(vec![
                    Span::styled(prefix, timeline_prefix_style(active, accent)),
                    Span::styled(detail, detail_style),
                ]));
            }
        }
        lines.push(Line::raw(""));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            i18n::text(language, Text::NoTimelineLoaded),
            Style::default().fg(theme::muted()),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(dynamic_panel_block(
                format!(" {} ", i18n::text(language, Text::TimelineTitle)),
                app.focus == Focus::Timeline,
            ))
            .scroll((timeline_scroll(app, area), 0)),
        area,
    );
}

fn timeline_scroll(app: &App, area: Rect) -> u16 {
    let viewport = usize::from(area.height.saturating_sub(2).max(1));
    let visible_groups = visible_timeline_groups(app);
    let selected_group = selected_timeline_group_position(&visible_groups, app.selected_event);
    let selected_top = visible_groups
        .iter()
        .take(selected_group)
        .map(|group| timeline_group_line_count(group, area.width))
        .sum::<usize>();
    let selected_height = visible_groups
        .get(selected_group)
        .map(|group| timeline_group_line_count(group, area.width))
        .unwrap_or(1);
    let center_padding = viewport.saturating_sub(selected_height) / 2;
    usize_to_u16(selected_top.saturating_sub(center_padding))
}

struct TimelineHeader<'a> {
    title: Option<&'a str>,
    time: &'a str,
    marker: &'a str,
    label: &'a str,
    marker_style: Style,
    time_style: Style,
    label_style: Style,
    title_style: Style,
}

fn timeline_header_line(header: TimelineHeader<'_>, area_width: u16) -> Line<'static> {
    let title = header.title;
    let time = header.time;
    let marker = header.marker;
    let label = header.label;
    let marker_style = header.marker_style;
    let time_style = header.time_style;
    let label_style = header.label_style;
    let title_style = header.title_style;
    let inner_width = usize::from(area_width.saturating_sub(4)).max(20);
    let left_width = display_width(marker)
        + 1
        + display_width(label)
        + 2
        + title.map(|title| 1 + display_width(title)).unwrap_or(0);
    let padding = inner_width
        .saturating_sub(left_width + display_width(time))
        .max(1);
    let mut spans = vec![
        Span::styled(format!("{marker} "), marker_style),
        Span::styled(format!(" {label} "), label_style),
    ];
    if let Some(title) = title {
        spans.push(Span::styled(format!(" {title}"), title_style));
    }
    spans.push(Span::raw(" ".repeat(padding)));
    spans.push(Span::styled(time.to_owned(), time_style));
    Line::from(spans)
}

fn timeline_group_title<'a>(group: &TimelineGroup<'a>) -> Option<&'a str> {
    let event = group.primary_event();
    match group.kind() {
        TimelineKind::User if event.title == "User" => None,
        TimelineKind::Assistant if event.title == "Assistant" => None,
        _ => Some(event.title.as_str()).filter(|title| !title.trim().is_empty()),
    }
}

fn timeline_group_accent(color: Color, is_rewind: bool) -> Color {
    if is_rewind {
        theme::role_rewind()
    } else {
        color
    }
}

fn timeline_prefix_style(active: bool, accent: Color) -> Style {
    if active {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    }
}

fn timeline_marker_style(active: bool, selected: bool, is_rewind: bool) -> Style {
    if active {
        Style::default()
            .fg(theme::cyan())
            .add_modifier(Modifier::BOLD)
    } else if is_rewind {
        Style::default()
            .fg(theme::role_rewind())
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default()
            .fg(theme::text())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    }
}

fn timeline_detail_style(active: bool, is_rewind: bool, kind: TimelineKind) -> Style {
    if active || is_rewind || kind == TimelineKind::RewindPoint {
        Style::default().fg(theme::text())
    } else {
        Style::default().fg(theme::muted())
    }
}

fn timeline_group_line_count(group: &TimelineGroup<'_>, area_width: u16) -> usize {
    1 + group
        .events()
        .map(|(_, event)| timeline_event_detail_lines(event, area_width).len())
        .sum::<usize>()
        + 1
}

fn timeline_detail_prefix(
    active: bool,
    ai_group: bool,
    event_offset: usize,
    line_index: usize,
) -> &'static str {
    if active && event_offset == 0 && line_index == 0 {
        return "   └ ";
    }
    if active && ai_group && line_index == 0 {
        return "   • ";
    }
    if active {
        return "     ";
    }
    if ai_group && event_offset > 0 && line_index == 0 {
        return "   · ";
    }
    "     "
}

fn timeline_event_detail_lines(event: &TimelineEvent, area_width: u16) -> Vec<String> {
    let mut lines = timeline_attachment_lines(&event.metadata.attachments, area_width);
    if !event.detail.trim().is_empty() {
        lines.extend(timeline_detail_lines(&event.detail, area_width));
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn timeline_attachment_lines(attachments: &[TimelineAttachment], area_width: u16) -> Vec<String> {
    attachments
        .iter()
        .flat_map(|attachment| {
            wrap_timeline_text(
                &timeline_attachment_label(attachment),
                timeline_detail_width(area_width),
            )
        })
        .collect()
}

fn timeline_attachment_label(attachment: &TimelineAttachment) -> String {
    let label = attachment
        .name
        .as_deref()
        .or(attachment.path.as_deref())
        .or(attachment.id.as_deref())
        .unwrap_or("unnamed");
    let kind = attachment
        .mime_type
        .as_deref()
        .filter(|mime_type| mime_type.starts_with("image/"))
        .map(|_| "image")
        .unwrap_or("attachment");
    format!("[{kind}] {label}")
}

fn timeline_detail_lines(detail: &str, area_width: u16) -> Vec<String> {
    wrap_timeline_text(detail, timeline_detail_width(area_width))
}

fn timeline_detail_width(area_width: u16) -> usize {
    usize::from(area_width.saturating_sub(10)).max(12)
}

fn wrap_timeline_text(text: &str, width: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        wrap_timeline_line(raw_line, width, &mut lines);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn wrap_timeline_line(line: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    for word in line.split_whitespace() {
        let word_width = display_width(word);
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.extend(split_display_width(word, width));
            continue;
        }

        if current.is_empty() {
            current.push_str(word);
        } else if display_width(&current) + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
}

fn split_display_width(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = character_display_width(character);
        if used + character_width > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(character);
        used += character_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn display_width(text: &str) -> usize {
    text.chars().map(character_display_width).sum()
}

fn character_display_width(character: char) -> usize {
    if character.is_ascii() { 1 } else { 2 }
}

fn usize_to_u16(value: usize) -> u16 {
    value.min(usize::from(u16::MAX)) as u16
}

struct TimelineGroup<'a> {
    first: (usize, &'a TimelineEvent),
    rest: Vec<(usize, &'a TimelineEvent)>,
}

impl<'a> TimelineGroup<'a> {
    fn new(event: (usize, &'a TimelineEvent)) -> Self {
        Self {
            first: event,
            rest: Vec::new(),
        }
    }

    fn push(&mut self, event: (usize, &'a TimelineEvent)) {
        self.rest.push(event);
    }

    fn events(&self) -> impl Iterator<Item = (usize, &'a TimelineEvent)> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().copied())
    }

    fn len(&self) -> usize {
        1 + self.rest.len()
    }

    fn primary_event(&self) -> &'a TimelineEvent {
        self.first.1
    }

    fn last_event(&self) -> &'a TimelineEvent {
        self.rest
            .last()
            .map(|(_, event)| *event)
            .unwrap_or(self.first.1)
    }

    fn last_event_index(&self) -> usize {
        self.rest
            .last()
            .map(|(index, _)| *index)
            .unwrap_or(self.first.0)
    }

    fn kind(&self) -> TimelineKind {
        self.primary_event().kind
    }

    fn is_ai_group(&self) -> bool {
        self.kind() == TimelineKind::Assistant
    }

    fn is_rewind(&self, rewind_event_id: &str) -> bool {
        self.events().any(|(_, event)| event.id == rewind_event_id)
    }
}

fn visible_timeline_groups(app: &App) -> Vec<TimelineGroup<'_>> {
    let mut groups: Vec<TimelineGroup<'_>> = Vec::new();
    for event in visible_timeline_events(app) {
        if event.1.kind == TimelineKind::Assistant
            && let Some(group) = groups.last_mut()
            && group.is_ai_group()
        {
            group.push(event);
            continue;
        }
        groups.push(TimelineGroup::new(event));
    }
    groups
}

fn selected_timeline_group_position(
    visible_groups: &[TimelineGroup<'_>],
    selected_event: usize,
) -> usize {
    visible_groups
        .iter()
        .position(|group| group.events().any(|(index, _)| index == selected_event))
        .or_else(|| {
            visible_groups
                .iter()
                .enumerate()
                .rev()
                .find(|(_, group)| group.last_event_index() <= selected_event)
                .map(|(position, _)| position)
        })
        .unwrap_or(0)
}

fn timeline_group_label(
    group: &TimelineGroup<'_>,
    is_rewind: bool,
    source: CliTool,
) -> (String, Color) {
    if is_rewind {
        return ("REWIND".into(), theme::gold());
    }
    match group.kind() {
        TimelineKind::User => ("USER".into(), theme::blue()),
        TimelineKind::Assistant if group.len() > 1 => (
            format!("{} x{}", assistant_source_label(source), group.len()),
            theme::gold(),
        ),
        TimelineKind::Assistant => (assistant_source_label(source).into(), theme::gold()),
        TimelineKind::Tool => ("TOOL".into(), theme::muted()),
        TimelineKind::Compact => ("COMPACT".into(), theme::cyan()),
        TimelineKind::Error => ("ERROR".into(), theme::red()),
        TimelineKind::GitDiff => ("GIT DIFF".into(), theme::green()),
        TimelineKind::RewindPoint => ("REWIND".into(), theme::gold()),
    }
}

fn assistant_source_label(source: CliTool) -> &'static str {
    match source {
        CliTool::Codex => "Codex",
        CliTool::Claude => "Claude Code",
        CliTool::Hermes => "Hermes",
    }
}

fn timeline_group_time(group: &TimelineGroup<'_>) -> String {
    let first = &group.primary_event().time;
    let last = &group.last_event().time;
    if group.len() == 1 {
        first.clone()
    } else if first == last {
        format!("{first} x{}", group.len())
    } else {
        format!("{first}-{last}")
    }
}

fn visible_timeline_events(app: &App) -> Vec<(usize, &TimelineEvent)> {
    app.data
        .timeline
        .iter()
        .enumerate()
        .filter(|(_, event)| event.id == app.rewind_event_id || event.kind != TimelineKind::Tool)
        .collect()
}

fn render_capsule(frame: &mut Frame, area: Rect, app: &App) {
    let language = app.effective_language();
    let capsule = &app.data.capsule;
    let mut lines = session_detail_lines(app);

    if app.is_session_load_pending() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Loading",
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Hydrating timeline and capsule preview for the selected session.",
            Style::default().fg(theme::text()),
        )));
        lines.push(Line::from(Span::styled(
            "  Launch, verify, compile, and rewind wait until this completes.",
            Style::default().fg(theme::muted()),
        )));
        frame.render_widget(
            Paragraph::new(lines)
                .block(dynamic_panel_block(
                    format!(" {} ", i18n::text(language, Text::SessionDetailsTitle)),
                    app.focus == Focus::Capsule,
                ))
                .scroll((app.capsule_scroll, 0))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    lines.extend(compact_capsule_lines(capsule));

    frame.render_widget(
        Paragraph::new(lines)
            .block(dynamic_panel_block(
                format!(" {} ", i18n::text(language, Text::SessionDetailsTitle)),
                app.focus == Focus::Capsule,
            ))
            .scroll((app.capsule_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn compact_capsule_lines(capsule: &WorkCapsule) -> Vec<Line<'static>> {
    vec![
        Line::raw(""),
        Line::from(Span::styled(
            "Handoff Snapshot",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
        review_label_line("State", capsule.state.clone(), theme::gold()),
        review_label_line(
            "Rewind",
            review_snippet(&capsule.rewind_point, 96),
            theme::gold(),
        ),
        review_label_line("Goal", review_snippet(&capsule.goal, 96), theme::blue()),
        review_label_line(
            "Risk",
            capsule
                .risks
                .first()
                .map(|risk| review_snippet(risk, 96))
                .unwrap_or_else(|| "none".into()),
            theme::red(),
        ),
        Line::from(Span::styled(
            "Press c to refresh and review the full handoff.",
            Style::default().fg(theme::muted()),
        )),
    ]
}

fn session_detail_lines(app: &App) -> Vec<Line<'static>> {
    let language = app.effective_language();
    let Some(session) = app.current_session() else {
        return vec![
            Line::from(Span::styled(
                i18n::text(language, Text::RealSessionMetadata),
                Style::default()
                    .fg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("  {}", i18n::text(language, Text::NoSelectedSession)),
                Style::default().fg(theme::muted()),
            )),
        ];
    };

    let zoomed = app.zoomed_focus == Some(Focus::Capsule);
    let mut lines = vec![
        Line::from(Span::styled(
            i18n::text(language, Text::RealSessionMetadata),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
        metadata_line(
            i18n::text(language, Text::RawTitle),
            &session.title,
            Style::default().fg(theme::text()),
        ),
        metadata_line(
            i18n::text(language, Text::Source),
            &format!("{} · {}", session.cli, session.source_provenance),
            source_provenance_style(session.source_provenance),
        ),
        source_fidelity_line(app, session.cli),
        metadata_line(
            i18n::text(language, Text::Portrait),
            &session_portrait_summary(app, session),
            Style::default().fg(theme::cyan()),
        ),
    ];

    lines.extend(session_anatomy_summary_lines(session));

    lines.extend([
        metadata_line(
            i18n::text(language, Text::Updated),
            &session.updated,
            Style::default().fg(theme::blue()),
        ),
        metadata_line(
            i18n::text(language, Text::Runtime),
            &session_runtime_detail(session),
            session_runtime_style(session.runtime_status),
        ),
        metadata_line(
            i18n::text(language, Text::Cwd),
            &session.cwd,
            Style::default().fg(theme::text()),
        ),
        metadata_line(
            i18n::text(language, Text::Branch),
            session.branch.as_deref().unwrap_or("-"),
            Style::default().fg(theme::cyan()),
        ),
        metadata_line(
            i18n::text(language, Text::TimelineItems),
            &session.event_count.to_string(),
            Style::default().fg(theme::muted()),
        ),
        metadata_line(
            i18n::text(language, Text::Tokens),
            &format_token_count(session.token_count),
            Style::default().fg(theme::gold()),
        ),
        metadata_line(
            i18n::text(language, Text::RawSize),
            &format_source_size_opt(session.source_size_bytes),
            Style::default().fg(theme::muted()),
        ),
        metadata_line(
            i18n::text(language, Text::SourceHealth),
            &session_health_detail(session),
            session_health_style(session.status),
        ),
    ]);
    if let Some(path) = &session.source_path {
        lines.push(metadata_line(
            i18n::text(language, Text::Path),
            path,
            Style::default().fg(theme::muted()),
        ));
    }
    if zoomed {
        lines.extend(session_anatomy_zoom_lines(session));
    }
    lines
}

fn session_anatomy_summary_lines(
    session: &crate::core::model::SessionSummary,
) -> Vec<Line<'static>> {
    let Some(anatomy) = session.anatomy.as_ref() else {
        return Vec::new();
    };

    let mut lines = vec![metadata_line(
        "Anatomy",
        &anatomy_status_text(anatomy.status, anatomy.sampled),
        anatomy_status_style(anatomy.status),
    )];
    if let Some(compact) = &anatomy.compact {
        lines.push(metadata_line(
            "Context Window",
            &format!(
                "{} · {} after compact",
                format_source_size(compact.tail_bytes),
                compact.tail_lines
            ),
            Style::default().fg(theme::gold()),
        ));
    } else if anatomy.status != SessionAnatomyStatus::Missing {
        lines.push(metadata_line(
            "Context Window",
            "no compact boundary in analyzed scope",
            Style::default().fg(theme::muted()),
        ));
    }
    if let Some(signal) = anatomy
        .value_signals
        .iter()
        .find(|signal| signal.group == "Trust")
    {
        lines.push(metadata_line(
            "Trust",
            &format!("{} · {}", signal.value, review_snippet(&signal.detail, 72)),
            anatomy_status_style(anatomy.status),
        ));
    }
    lines
}

fn session_anatomy_zoom_lines(session: &crate::core::model::SessionSummary) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw("")];
    let Some(anatomy) = session.anatomy.as_ref() else {
        lines.push(Line::from(Span::styled(
            "Session Anatomy",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "No anatomy has been loaded for this session yet.",
            Style::default().fg(theme::muted()),
        )));
        return lines;
    };

    lines.push(Line::from(Span::styled(
        "Session Anatomy",
        Style::default()
            .fg(theme::blue())
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(review_label_line(
        "Scope",
        format!(
            "{} · {} analyzed{}",
            anatomy.scan_scope,
            format_source_size(anatomy.analyzed_bytes),
            if anatomy.sampled { " · sampled" } else { "" }
        ),
        theme::cyan(),
    ));
    if let Some(lines_count) = anatomy.total_lines {
        lines.push(review_label_line(
            "Rows",
            format!(
                "{lines_count} parsed · {} malformed",
                anatomy.malformed_lines
            ),
            theme::muted(),
        ));
    } else {
        lines.push(review_label_line(
            "Rows",
            format!("sample parsed · {} malformed", anatomy.malformed_lines),
            theme::muted(),
        ));
    }

    lines.push(Line::raw(""));
    lines.push(section_header("Value Signals"));
    for signal in &anatomy.value_signals {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}. {} / {}: ", signal.rank, signal.group, signal.label),
                Style::default()
                    .fg(value_signal_color(signal.rank))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(signal.value.clone(), Style::default().fg(theme::text())),
        ]));
        if !signal.detail.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("   {}", review_snippet(&signal.detail, 128)),
                Style::default().fg(theme::muted()),
            )));
        }
    }

    if let Some(compact) = &anatomy.compact {
        lines.push(Line::raw(""));
        lines.push(section_header("Compact Frontier"));
        lines.push(review_label_line(
            "Boundary",
            match compact.line_number {
                Some(line) => format!("{} at line {line}", compact.label),
                None => format!("{} in analyzed sample", compact.label),
            },
            theme::gold(),
        ));
        lines.push(review_label_line(
            "Active Tail",
            format!(
                "{} · {}",
                format_source_size(compact.tail_bytes),
                plural_rows(compact.tail_lines)
            ),
            theme::gold(),
        ));
    }

    append_metric_section(&mut lines, "Size Profile", &anatomy.size_profile);
    append_metric_section(&mut lines, "Event Profile", &anatomy.event_profile);
    append_metric_section(&mut lines, "Content Profile", &anatomy.content_profile);

    if !anatomy.sidecars.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section_header("Sidecars"));
        for sidecar in &anatomy.sidecars {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", sidecar.kind),
                    Style::default()
                        .fg(theme::purple())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "{} · {}",
                        format_source_size(sidecar.bytes),
                        plural_files(sidecar.file_count)
                    ),
                    Style::default().fg(theme::text()),
                ),
            ]));
        }
    }

    if !anatomy.notes.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section_header("Notes"));
        for note in &anatomy.notes {
            lines.push(Line::from(Span::styled(
                format!("- {}", review_snippet(note, 128)),
                Style::default().fg(theme::muted()),
            )));
        }
    }

    lines
}

fn append_metric_section(
    lines: &mut Vec<Line<'static>>,
    title: &'static str,
    metrics: &[AnatomyMetric],
) {
    lines.push(Line::raw(""));
    lines.push(section_header(title));
    if metrics.is_empty() {
        lines.push(Line::from(Span::styled(
            "No rows in analyzed scope.",
            Style::default().fg(theme::muted()),
        )));
        return;
    }
    for metric in metrics {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}: ", metric.label),
                Style::default()
                    .fg(theme::cyan())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{} · {}",
                    format_source_size(metric.bytes),
                    plural_rows(metric.count)
                ),
                Style::default().fg(theme::text()),
            ),
        ]));
    }
}

fn section_header(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::default()
            .fg(theme::blue())
            .add_modifier(Modifier::BOLD),
    ))
}

fn anatomy_status_text(status: SessionAnatomyStatus, sampled: bool) -> String {
    let label = match status {
        SessionAnatomyStatus::Missing => "missing",
        SessionAnatomyStatus::Ready => "ready",
        SessionAnatomyStatus::Partial => "partial",
        SessionAnatomyStatus::Failed => "failed",
    };
    if sampled {
        format!("{label} · tail sampled")
    } else {
        label.into()
    }
}

fn anatomy_status_style(status: SessionAnatomyStatus) -> Style {
    match status {
        SessionAnatomyStatus::Ready => Style::default().fg(theme::green()),
        SessionAnatomyStatus::Partial => Style::default().fg(theme::gold()),
        SessionAnatomyStatus::Missing => Style::default().fg(theme::muted()),
        SessionAnatomyStatus::Failed => Style::default()
            .fg(theme::red())
            .add_modifier(Modifier::BOLD),
    }
}

fn value_signal_color(rank: u8) -> Color {
    match rank {
        1 => theme::gold(),
        2 => theme::green(),
        3 => theme::cyan(),
        _ => theme::muted(),
    }
}

fn plural_rows(count: usize) -> String {
    if count == 1 {
        "1 row".into()
    } else {
        format!("{count} rows")
    }
}

fn plural_files(count: usize) -> String {
    if count == 1 {
        "1 file".into()
    } else {
        format!("{count} files")
    }
}

fn source_fidelity_line(app: &App, cli: CliTool) -> Line<'static> {
    let language = app.effective_language();
    let Some(report) = source_adapter_report(app, cli) else {
        return metadata_line(
            i18n::text(language, Text::Fidelity),
            "missing · none",
            source_fidelity_style(SourceFidelityStatus::Missing),
        );
    };
    let value = source_fidelity_detail(report);
    metadata_line(
        i18n::text(language, Text::Fidelity),
        &value,
        source_fidelity_style(report.fidelity.status),
    )
}

fn source_adapter_report(app: &App, cli: CliTool) -> Option<&SourceAdapterReport> {
    app.data
        .source_adapters
        .iter()
        .find(|report| report.cli == cli)
}

fn source_fidelity_detail(report: &SourceAdapterReport) -> String {
    match report.fidelity.fallback_surface.as_deref() {
        Some(fallback) => format!(
            "{} · {} · fallback {}",
            report.fidelity.status, report.fidelity.primary_surface, fallback
        ),
        None => format!(
            "{} · {}",
            report.fidelity.status, report.fidelity.primary_surface
        ),
    }
}

fn source_fidelity_style(status: SourceFidelityStatus) -> Style {
    match status {
        SourceFidelityStatus::FullFidelity => Style::default().fg(theme::green()),
        SourceFidelityStatus::Partial => Style::default().fg(theme::gold()),
        SourceFidelityStatus::Fallback => Style::default().fg(theme::orange()),
        SourceFidelityStatus::Missing => Style::default().fg(theme::red()),
    }
}

fn metadata_line(label: &'static str, value: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(theme::muted())),
        Span::styled(value.to_owned(), style),
    ])
}

fn session_runtime_detail(session: &crate::core::model::SessionSummary) -> String {
    if session.runtime_status == SessionRuntimeStatus::Unknown {
        return session.runtime_status.to_string();
    }
    match &session.runtime_reason {
        Some(reason) if !reason.trim().is_empty() => {
            format!("{} - {reason}", session.runtime_status)
        }
        _ => session.runtime_status.to_string(),
    }
}

fn session_runtime_style(status: SessionRuntimeStatus) -> Style {
    match status {
        SessionRuntimeStatus::Active => Style::default().fg(theme::green()),
        SessionRuntimeStatus::Inactive => Style::default().fg(theme::muted()),
        SessionRuntimeStatus::Unknown => Style::default().fg(theme::gold()),
    }
}

fn render_branch_tree(frame: &mut Frame, area: Rect, app: &App) {
    let lines = if area.height < 4 {
        vec![handoff_path_line(app, area.width)]
    } else if let Some(trail) = app.handoff_trail_frame() {
        vec![
            handoff_path_line(app, area.width),
            handoff_trail_line(trail),
        ]
    } else {
        vec![
            handoff_path_line(app, area.width),
            cwd_inventory_line(app, area.width),
        ]
    };
    frame.render_widget(
        Paragraph::new(lines).block(dynamic_panel_block(
            format!(
                " {} ",
                i18n::text(app.effective_language(), Text::ActionPath)
            ),
            app.focus == Focus::Branches,
        )),
        area,
    );
}

fn handoff_path_line(app: &App, width: u16) -> Line<'static> {
    let (session, source_color) = app
        .current_session()
        .map(|session| {
            let keep = if width < 96 { 8 } else { 14 };
            (
                format!(
                    "source {} {}",
                    session.cli,
                    short_identifier(&session.id, keep)
                ),
                source_tool_color(session.cli),
            )
        })
        .unwrap_or_else(|| ("no session".into(), theme::muted()));
    let rewind = format!("rewind {}", short_identifier(&app.rewind_event_id, 12));
    let target_cli = if app.show_launch {
        app.pending_target
    } else {
        app.data.target
    };
    let target = format!("target {target_cli}");
    let nodes = [
        (session, source_color),
        (rewind, theme::role_rewind()),
        (target, theme::role_target()),
    ];

    let mut spans = vec![Span::styled("   ", Style::default().fg(theme::muted()))];
    for (idx, (label, color)) in nodes.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" -> ", Style::default().fg(theme::border())));
        }
        spans.push(Span::styled(
            label.clone(),
            Style::default().fg(*color).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn handoff_trail_line(frame: HandoffTrailFrame) -> Line<'static> {
    let arrow_one = match frame.step {
        0 => " ◆-> ",
        1 => " -◆> ",
        _ => " --> ",
    };
    let arrow_two = match frame.step {
        3 => " ◆-> ",
        4 => " -◆> ",
        _ => " --> ",
    };
    let source_style = if frame.step == 0 {
        Style::default()
            .fg(theme::gold())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    };
    let rewind_style = if (2..=3).contains(&frame.step) {
        Style::default()
            .fg(theme::gold())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    };
    let target_style = if frame.step >= 5 {
        Style::default()
            .fg(theme::cyan())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    };
    Line::from(vec![
        Span::styled("   handoff trail  ", Style::default().fg(theme::muted())),
        Span::styled("source", source_style),
        Span::styled(arrow_one, Style::default().fg(theme::role_rewind())),
        Span::styled("rewind", rewind_style),
        Span::styled(arrow_two, Style::default().fg(theme::role_target())),
        Span::styled("target", target_style),
        Span::styled("  ", Style::default().fg(theme::border())),
        Span::styled(
            frame.phase.label(),
            Style::default()
                .fg(theme::role_target())
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn cwd_inventory_line(app: &App, width: u16) -> Line<'static> {
    let Some(session) = app.current_session() else {
        return Line::from(Span::styled(
            "   cwd: no session",
            Style::default().fg(theme::muted()),
        ));
    };
    let codex = cwd_session_count(app, &session.cwd, CliTool::Codex);
    let claude = cwd_session_count(app, &session.cwd, CliTool::Claude);
    let hermes = cwd_session_count(app, &session.cwd, CliTool::Hermes);
    let max_path_chars = usize::from(width.saturating_sub(56)).clamp(12, 64);
    Line::from(vec![
        Span::styled("   cwd: ", Style::default().fg(theme::muted())),
        Span::styled(
            review_snippet(&session.cwd, max_path_chars),
            Style::default().fg(theme::text()),
        ),
        Span::styled(" · ", Style::default().fg(theme::border())),
        Span::styled(
            format!("Codex {codex}"),
            Style::default().fg(source_tool_color(CliTool::Codex)),
        ),
        Span::styled(" · ", Style::default().fg(theme::border())),
        Span::styled(
            format!("Claude {claude}"),
            Style::default().fg(source_tool_color(CliTool::Claude)),
        ),
        Span::styled(" · ", Style::default().fg(theme::border())),
        Span::styled(
            format!("Hermes {hermes}"),
            Style::default().fg(source_tool_color(CliTool::Hermes)),
        ),
    ])
}

fn cwd_session_count(app: &App, cwd: &str, tool: CliTool) -> usize {
    app.data
        .sessions
        .iter()
        .filter(|session| session.cwd == cwd && session.cli == tool)
        .count()
}

fn short_identifier(value: &str, keep: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(keep).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

fn render_command_bar(frame: &mut Frame, area: Rect, app: &App) {
    let lines = if app.command_mode && app.command_input.starts_with('/') {
        let (prompt, input) = if let Some(query) = app.command_input.strip_prefix('/') {
            ("/", query)
        } else {
            (":", app.command_input.as_str())
        };
        vec![
            status_line(app),
            Line::from(vec![
                Span::styled(
                    prompt,
                    Style::default()
                        .fg(theme::gold())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(input, Style::default().fg(theme::text())),
            ]),
        ]
    } else if app.command_mode {
        vec![
            status_line(app),
            hint_line(&[
                ("enter", "Run"),
                ("tab", "Complete"),
                ("j/k", "Select"),
                ("Esc", "Close"),
            ]),
        ]
    } else {
        let mut lines = vec![status_line(app)];
        let hints = active_key_hints(app);
        let chunk_size = if area.width < 120 { 4 } else { hints.len() };
        for chunk in hints.chunks(chunk_size.max(1)).take(3) {
            lines.push(hint_line(chunk));
        }
        lines
    };

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme::border()))
                .style(Style::default()),
        ),
        area,
    );
}

type KeyHint = (&'static str, &'static str);

fn active_key_hints(app: &App) -> Vec<KeyHint> {
    let language = app.effective_language();
    if app.show_skill_picker {
        let apply = if app.show_launch {
            localized(language, "Apply + generate", "应用并生成")
        } else {
            i18n::text(language, Text::Apply)
        };
        return vec![
            ("j/k", i18n::text(language, Text::Skill)),
            ("enter", apply),
            ("y", i18n::text(language, Text::CopyRef)),
            ("q", i18n::text(language, Text::Close)),
        ];
    }
    if app.show_launch {
        if app.is_launch_review_pending() {
            return vec![
                ("Esc/q", i18n::text(language, Text::Back)),
                ("wait", i18n::text(language, Text::WaitBackground)),
            ];
        }
        if app.target_launch_result.is_some() {
            return vec![
                ("r", i18n::text(language, Text::Rerun)),
                ("y", i18n::text(language, Text::CopyCommand)),
                ("Esc/q", i18n::text(language, Text::Back)),
            ];
        }
        if app.launch_review_error().is_some() {
            return vec![
                ("enter", i18n::text(language, Text::Retry)),
                ("S", i18n::text(language, Text::Skill)),
                ("y/r", i18n::text(language, Text::Unavailable)),
                ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                ("Esc/q", i18n::text(language, Text::Back)),
            ];
        }
        if app.launch_review {
            let capsule = app.launch_capsule_for_target(app.pending_target);
            let validation = app.validate_launch_for_target(app.pending_target);
            if app.launch_requires_handoff_skill(app.pending_target) {
                return vec![
                    ("S/enter", i18n::text(language, Text::Skill)),
                    ("y/r", i18n::text(language, Text::Unavailable)),
                    ("gg/G", i18n::text(language, Text::Jump)),
                    ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                    ("Esc/q", i18n::text(language, Text::Back)),
                ];
            }
            if validation_can_regenerate_handoff(&validation) {
                return vec![
                    ("enter", i18n::text(language, Text::RegenerateHandoffReview)),
                    ("y/r", i18n::text(language, Text::Unavailable)),
                    ("gg/G", i18n::text(language, Text::Jump)),
                    ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                    ("Esc/q", i18n::text(language, Text::Back)),
                ];
            }
            let run_hint = if validation.is_blocked() {
                i18n::text(language, Text::CannotRun)
            } else if compiler::compiler_is_builtin(&capsule.compiler)
                && app
                    .current_session()
                    .is_some_and(|session| session.source_provenance != SourceProvenance::Fixture)
            {
                i18n::text(language, Text::DraftCannotRun)
            } else {
                i18n::text(language, Text::RunLocalTarget)
            };
            return vec![
                ("r", run_hint),
                ("y", i18n::text(language, Text::CopyCommand)),
                ("gg/G", i18n::text(language, Text::Jump)),
                ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                ("Esc/q", i18n::text(language, Text::Back)),
            ];
        }
        let target_validation = app.validate_launch_for_target(app.pending_target);
        if app.launch_requires_handoff_skill(app.pending_target) {
            return vec![
                ("j/k", i18n::text(language, Text::Target)),
                ("S/enter", i18n::text(language, Text::Skill)),
                ("y", i18n::text(language, Text::Unavailable)),
                ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                ("Esc", i18n::text(language, Text::Cancel)),
            ];
        }
        if target_validation.state == LaunchValidationState::Blocked {
            if validation_can_regenerate_handoff(&target_validation) {
                return vec![
                    ("j/k", i18n::text(language, Text::Target)),
                    ("enter", i18n::text(language, Text::RegenerateHandoffReview)),
                    ("y", i18n::text(language, Text::Unavailable)),
                    ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                    ("Esc", i18n::text(language, Text::Cancel)),
                ];
            }
            return vec![
                ("j/k", i18n::text(language, Text::Target)),
                ("enter/y", i18n::text(language, Text::Blocked)),
                ("PgUp/Dn", i18n::text(language, Text::Scroll)),
                ("Esc", i18n::text(language, Text::Cancel)),
            ];
        }
        return vec![
            ("j/k", i18n::text(language, Text::Target)),
            ("enter", i18n::text(language, Text::Review)),
            ("y", i18n::text(language, Text::Unavailable)),
            ("PgUp/Dn", i18n::text(language, Text::Scroll)),
            ("Esc", i18n::text(language, Text::Cancel)),
        ];
    }
    if app.show_open_original {
        return vec![
            ("y", i18n::text(language, Text::Copy)),
            ("j/k", i18n::text(language, Text::Scroll)),
            ("PgUp/Dn", i18n::text(language, Text::Scroll)),
            ("Esc", i18n::text(language, Text::Close)),
        ];
    }
    if app.show_doctor {
        return vec![
            ("v", i18n::text(language, Text::Verify)),
            ("r", i18n::text(language, Text::Refresh)),
            ("y", i18n::text(language, Text::CopyJson)),
            ("j/k", i18n::text(language, Text::Scroll)),
            ("Esc", i18n::text(language, Text::Close)),
        ];
    }
    if app.show_capsules {
        return vec![
            ("r", i18n::text(language, Text::Refresh)),
            ("j/k", i18n::text(language, Text::Scroll)),
            ("PgUp/Dn", i18n::text(language, Text::Scroll)),
            ("Esc", i18n::text(language, Text::Close)),
        ];
    }
    if app.show_settings {
        return vec![
            ("space/t", i18n::text(language, Text::Toggle)),
            ("enter", i18n::text(language, Text::Save)),
            ("ctrl-s", i18n::text(language, Text::Save)),
            ("Esc", i18n::text(language, Text::Cancel)),
        ];
    }
    if app.show_data_space_config {
        return vec![
            ("enter", i18n::text(language, Text::ParseSave)),
            ("tab", i18n::text(language, Text::Next)),
            ("ctrl-s", i18n::text(language, Text::Save)),
            ("Esc", i18n::text(language, Text::Back)),
        ];
    }
    if app.show_data_spaces {
        return vec![
            ("n/a", i18n::text(language, Text::AddSsh)),
            ("x", i18n::text(language, Text::Delete)),
            ("j/k", i18n::text(language, Text::Choose)),
            ("enter", i18n::text(language, Text::Load)),
            ("r", i18n::text(language, Text::Reload)),
            ("Esc", i18n::text(language, Text::Close)),
        ];
    }
    if app.show_timeline_detail {
        return vec![
            ("j/k", i18n::text(language, Text::Scroll)),
            ("PgUp/Dn", i18n::text(language, Text::Scroll)),
            ("Esc", i18n::text(language, Text::Close)),
            ("q", i18n::text(language, Text::Close)),
        ];
    }
    if app.show_help {
        return vec![
            ("j/k", i18n::text(language, Text::Scroll)),
            ("PgUp/Dn", i18n::text(language, Text::Scroll)),
            ("Esc", i18n::text(language, Text::Close)),
            ("q", i18n::text(language, Text::Close)),
        ];
    }

    match app.focus {
        Focus::Sessions => vec![
            ("j/k", i18n::text(language, Text::SessionsTitle)),
            ("gg/G", i18n::text(language, Text::Jump)),
            ("/", i18n::text(language, Text::Search)),
            ("[ ]", i18n::text(language, Text::Source)),
            ("{ }", i18n::text(language, Text::Data)),
            ("d", i18n::text(language, Text::DataPicker)),
            (",", i18n::text(language, Text::Settings)),
            ("a", i18n::text(language, Text::Clear)),
            ("s", i18n::text(language, Text::Star)),
            ("S", i18n::text(language, Text::Skill)),
            ("+", i18n::text(language, Text::Zoom)),
            ("-", i18n::text(language, Text::Restore)),
            ("o", i18n::text(language, Text::Original)),
            ("enter", localized_enter_key_hint(app)),
            ("x/H", i18n::text(language, Text::Handoff)),
            ("tab", i18n::text(language, Text::Next)),
        ],
        Focus::Timeline => vec![
            ("j/k", i18n::text(language, Text::Events)),
            ("gg/G", i18n::text(language, Text::Jump)),
            ("e", i18n::text(language, Text::Detail)),
            ("space", i18n::text(language, Text::RewindPoint)),
            ("c", i18n::text(language, Text::Review)),
            ("+", i18n::text(language, Text::Zoom)),
            ("-", i18n::text(language, Text::Restore)),
            ("tab", i18n::text(language, Text::Next)),
            (":", i18n::text(language, Text::Cmd)),
            ("q", i18n::text(language, Text::Quit)),
        ],
        Focus::Capsule => vec![
            ("j/k", i18n::text(language, Text::Scroll)),
            ("gg/G", i18n::text(language, Text::TopBottom)),
            ("c", i18n::text(language, Text::Review)),
            ("v", i18n::text(language, Text::Verify)),
            ("S", i18n::text(language, Text::Skill)),
            ("+", i18n::text(language, Text::Zoom)),
            ("-", i18n::text(language, Text::Restore)),
            ("tab", i18n::text(language, Text::Next)),
            (":", i18n::text(language, Text::Cmd)),
            ("q", i18n::text(language, Text::Quit)),
        ],
        Focus::Branches => vec![
            ("enter", localized_enter_key_hint(app)),
            ("x/H", i18n::text(language, Text::Handoff)),
            ("o", i18n::text(language, Text::Original)),
            ("space", i18n::text(language, Text::RewindPoint)),
            ("D", i18n::text(language, Text::Preflight)),
            (",", i18n::text(language, Text::Settings)),
            ("+", i18n::text(language, Text::Zoom)),
            ("-", i18n::text(language, Text::Restore)),
            ("tab", i18n::text(language, Text::Next)),
            (":", i18n::text(language, Text::Cmd)),
            ("?", i18n::text(language, Text::Help)),
            ("q", i18n::text(language, Text::Quit)),
        ],
    }
}

fn hint_line(hints: &[KeyHint]) -> Line<'static> {
    let mut spans = Vec::new();
    for &(label, action) in hints {
        spans.push(key(label));
        spans.push(txt(action));
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

fn localized_enter_key_hint(app: &App) -> &'static str {
    let label = app.enter_key_hint();
    if app.effective_language() == crate::core::config::UiLanguage::English {
        return label;
    }
    match label {
        "Handoff" => i18n::text(app.effective_language(), Text::Handoff),
        "Resume" => i18n::text(app.effective_language(), Text::Resume),
        "Jump" => i18n::text(app.effective_language(), Text::Jump),
        "Unavailable" => i18n::text(app.effective_language(), Text::Unavailable),
        _ => label,
    }
}

fn status_line(app: &App) -> Line<'_> {
    let language = app.effective_language();
    let status_lower = app.status_message.to_ascii_lowercase();
    let (color, bold) = if app.data_space_error.is_some()
        || status_lower.contains("failed")
        || status_lower.contains("fail")
        || status_lower.contains("blocked")
        || status_lower.contains("not found")
        || status_lower.contains("cannot ")
        || status_lower.contains("invalid")
    {
        (theme::red(), true)
    } else if app.status_message.contains("cancelled")
        || app.status_message.contains("No session")
        || app.status_message.contains("Unknown")
        || app.status_message.contains("NEEDS REVIEW")
    {
        (theme::orange(), true)
    } else if app.status_message.contains("PASS")
        || app.status_message.contains("saved")
        || app.status_message.contains("compiled")
        || app.status_message.contains("refreshed")
        || app.status_message.contains("cleared")
    {
        (theme::green(), true)
    } else {
        (theme::muted(), false)
    };
    let message_style = if bold {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color)
    };

    let mut spans = vec![
        Span::styled(
            format!("{} ", i18n::text(language, Text::Status)),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(localized_status_message(app), message_style),
    ];
    if let Some(live) = app.hook_live_indicator() {
        let live_style = if live.is_error {
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD)
        } else if live.is_stale {
            Style::default().fg(theme::gold())
        } else {
            Style::default().fg(theme::green())
        };
        spans.push(Span::styled("   ", Style::default().fg(theme::border())));
        spans.push(Span::styled(live.label, live_style));
    }
    Line::from(spans)
}

fn localized_status_message(app: &App) -> String {
    let language = app.effective_language();
    if language == crate::core::config::UiLanguage::English {
        return app.status_message.clone();
    }
    if app.status_message == "Settings opened" {
        return i18n::text(language, Text::SettingsOpened).to_string();
    }
    if app.status_message == "Choose compiler skill" {
        return i18n::text(language, Text::ChooseCompilerSkill).to_string();
    }
    if let Some(target) = app
        .status_message
        .strip_prefix("Regenerating handoff review: ")
    {
        return format!(
            "{}: {target}",
            i18n::text(language, Text::RegenerateHandoffReview)
        );
    }
    if let Some(reason) = app.status_message.strip_prefix("Target blocked: ") {
        return format!(
            "{}: {}",
            i18n::text(language, Text::Blocked),
            localize_validation_detail(language, reason)
        );
    }
    app.status_message.clone()
}

fn render_settings(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 70, 58);
    frame.render_widget(Clear, area);
    let language = app.effective_language();
    let route = app.settings_enter_route_preview();
    let hooks = if app.hooks_enabled() {
        (i18n::text(language, Text::Enabled), theme::green())
    } else {
        (i18n::text(language, Text::Disabled), theme::muted())
    };
    let effect = settings_effect(app, language);

    let mut lines = vec![
        Line::from(Span::styled(
            i18n::text(language, Text::SettingsTitle),
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            i18n::text(language, Text::SettingsSubtitle),
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                format!("{:<22}", i18n::text(language, Text::HooksEventChannel)),
                Style::default().fg(theme::blue()),
            ),
            Span::styled(
                hooks.0,
                Style::default().fg(hooks.1).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", i18n::text(language, Text::HooksManagedByCli)),
                Style::default().fg(theme::muted()),
            ),
        ]),
        settings_row(
            app,
            language,
            SettingsField::Language,
            i18n::text(language, Text::Language),
            i18n::language_name(language, app.settings_language),
            i18n::language_name(language, app.ui_language()),
            app.settings_language_dirty(),
        ),
        settings_row(
            app,
            language,
            SettingsField::Theme,
            i18n::text(language, Text::Theme),
            app.settings_theme.label(),
            app.ui_theme().label(),
            app.settings_theme_dirty(),
        ),
        settings_row(
            app,
            language,
            SettingsField::SmartEnter,
            i18n::text(language, Text::SmartEnterTmux),
            i18n::on_off(language, app.settings_smart_enter_tmux),
            i18n::on_off(language, app.smart_enter_tmux_enabled()),
            app.settings_smart_enter_dirty(),
        ),
        Line::raw(""),
        Line::from(Span::styled(
            i18n::text(language, Text::CurrentEnterRoute),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
    ];

    if let Some(route) = route {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<10}", route.label),
                enter_route_style(route.kind),
            ),
            Span::styled(route.detail, Style::default().fg(theme::text())),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            i18n::text(language, Text::NoSelectedSession),
            Style::default().fg(theme::orange()),
        )));
    }

    lines.extend([
        Line::raw(""),
        Line::from(Span::styled(
            i18n::text(language, Text::Effect),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(effect, Style::default().fg(theme::text()))),
        Line::raw(""),
        Line::from(Span::styled(
            i18n::text(language, Text::SettingsSafety),
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            i18n::text(language, Text::SettingsKeys),
            Style::default().fg(theme::muted()),
        )),
    ]);

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(
                if app.settings_dirty() {
                    " Settings * "
                } else {
                    " Settings "
                },
                true,
            ))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn settings_row(
    app: &App,
    language: crate::core::config::UiLanguage,
    field: SettingsField,
    label: &str,
    draft: &str,
    saved: &str,
    dirty: bool,
) -> Line<'static> {
    let focused = app.settings_field_is_focused(field);
    let marker = if focused { ">" } else { " " };
    let marker_color = if focused {
        theme::gold()
    } else {
        theme::border()
    };
    let label_color = if focused {
        theme::text()
    } else {
        theme::blue()
    };
    let state_color = if dirty { theme::gold() } else { theme::green() };
    Line::from(vec![
        Span::styled(marker, Style::default().fg(marker_color)),
        Span::raw(" "),
        Span::styled(
            format!("{label:<22}"),
            Style::default().fg(label_color).add_modifier(if focused {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ),
        Span::styled(
            format!("{} {draft}", i18n::text(language, Text::Draft)),
            Style::default().fg(if dirty { theme::gold() } else { theme::text() }),
        ),
        Span::styled(
            format!("   {} {saved}", i18n::text(language, Text::Saved)),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(
            format!(
                "   {}",
                if dirty {
                    i18n::text(language, Text::Unsaved)
                } else {
                    i18n::text(language, Text::Saved)
                }
            ),
            Style::default().fg(state_color),
        ),
    ])
}

fn settings_effect(app: &App, language: crate::core::config::UiLanguage) -> &'static str {
    match language {
        crate::core::config::UiLanguage::English => {
            if !app.hooks_enabled() {
                "Smart Enter cannot jump until hooks are installed and new agent sessions are started."
            } else if !app.settings_smart_enter_tmux {
                "Enter keeps the existing resume or handoff behavior."
            } else if app.current_data_space().is_local() {
                "Enter validates hook tmux metadata, jumps to a live pane when available, and falls back to resume otherwise."
            } else {
                "SSH data spaces stay read-only; Enter opens guarded handoff instead of local resume or tmux jump."
            }
        }
        crate::core::config::UiLanguage::ZhHans => {
            if !app.hooks_enabled() {
                "安装 hooks 并新开 agent session 后，Smart Enter 才能跳转。"
            } else if !app.settings_smart_enter_tmux {
                "Enter 保持既有 resume 或 handoff 行为。"
            } else if app.current_data_space().is_local() {
                "Enter 会校验 hook 捕获的 tmux metadata；pane 存活才跳转，否则降级 resume。"
            } else {
                "SSH data space 保持只读；Enter 打开受保护 handoff，不做本地 resume 或 tmux jump。"
            }
        }
    }
}

fn render_data_spaces(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 70, 62);
    frame.render_widget(Clear, area);

    let selected_index = app
        .data_space_selection
        .min(app.data_spaces.len().saturating_sub(1));
    let selected = app.data_spaces.get(selected_index);
    let mut lines = vec![
        Line::from(Span::styled(
            "Data Spaces",
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Local plus SSH spaces saved in Moonbox. OpenSSH hosts are not auto-loaded.",
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
    ];

    if let Some(error) = &app.data_space_error {
        lines.push(Line::from(Span::styled(
            "Load Failed",
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            review_snippet(error, 118),
            Style::default().fg(theme::red()),
        )));
        lines.push(Line::from(Span::styled(
            "Install moonbox on the remote host, or set MOONBOX_REMOTE_BIN to an absolute remote path.",
            Style::default().fg(theme::muted()),
        )));
        lines.push(Line::raw(""));
    }

    if app.data_spaces.is_empty() {
        lines.push(Line::from(Span::styled(
            "No data spaces configured",
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        )));
    } else {
        for (index, space) in app.data_spaces.iter().enumerate() {
            lines.push(data_space_row(
                space,
                index == selected_index,
                index == app.selected_data_space,
            ));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Selected Configuration",
        Style::default()
            .fg(theme::blue())
            .add_modifier(Modifier::BOLD),
    )));
    if let Some(space) = selected {
        lines.extend(data_space_detail_lines(space));
    } else {
        lines.push(Line::from(Span::styled(
            "No selected data space",
            Style::default().fg(theme::muted()),
        )));
    }
    if let Some(name) = &app.data_space_delete_confirmation {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("Press x again to delete {name} from Moonbox config."),
            Style::default()
                .fg(theme::orange())
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "n/a add SSH   x delete   j/k choose   Enter load   r reload   Esc close",
        Style::default().fg(theme::muted()),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Data Space Picker ", true))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_data_space_config(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 68, 54);
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(Span::styled(
            "Add SSH Space",
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Paste ssh user@host, ssh://user@host:22, or an OpenSSH Host block.",
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
    ];

    for index in 0..DATA_SPACE_CONFIG_FIELD_COUNT {
        lines.push(data_space_config_field_line(app, index));
    }

    if let Some(error) = &app.data_space_error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            review_snippet(error, 96),
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Enter parse/save quick target   Tab next   Ctrl-S save   Esc back",
        Style::default().fg(theme::muted()),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Add SSH Data Space ", true))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn data_space_config_field_line(app: &App, index: usize) -> Line<'static> {
    let selected = app.data_space_config_field == index;
    let marker = if selected { "›" } else { " " };
    let cursor = if selected { "▏" } else { "" };
    let (label, value, hint, required) = match index {
        0 => (
            "Paste",
            app.data_space_config_form.quick.as_str(),
            "ssh user@host -p 22 -i key",
            false,
        ),
        1 => (
            "Name",
            app.data_space_config_form.name.as_str(),
            "shown in Moonbox",
            true,
        ),
        2 => (
            "Host",
            app.data_space_config_form.host.as_str(),
            "hostname or IP",
            true,
        ),
        3 => (
            "User",
            app.data_space_config_form.user.as_str(),
            "optional",
            false,
        ),
        4 => (
            "Port",
            app.data_space_config_form.port.as_str(),
            "optional",
            false,
        ),
        _ => (
            "Key",
            app.data_space_config_form.identity_file.as_str(),
            "optional",
            false,
        ),
    };
    let value = if value.is_empty() {
        if required { "<required>" } else { "<optional>" }
    } else {
        value
    };
    let value_style = if selected {
        Style::default()
            .fg(theme::text())
            .add_modifier(Modifier::BOLD)
    } else if required && value == "<required>" {
        Style::default().fg(theme::orange())
    } else {
        Style::default().fg(theme::text())
    };

    Line::from(vec![
        Span::styled(marker, Style::default().fg(theme::cyan())),
        Span::raw(" "),
        Span::styled(format!("{label:<7}"), Style::default().fg(theme::blue())),
        Span::styled(format!("{value}{cursor:<1}"), value_style),
        Span::raw("  "),
        Span::styled(hint, Style::default().fg(theme::muted())),
    ])
}

fn data_space_row(
    space: &crate::core::dataspace::DataSpaceEntry,
    selected: bool,
    active: bool,
) -> Line<'static> {
    let marker = if selected { "›" } else { " " };
    let state = if active { "ACTIVE" } else { "      " };
    let kind = if space.is_local() { "LOCAL" } else { "SSH" };
    let kind_color = if space.is_local() {
        theme::cyan()
    } else {
        theme::orange()
    };
    let label_style = if selected {
        Style::default()
            .fg(theme::text())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::text())
    };

    Line::from(vec![
        Span::styled(format!("{marker:<2}"), Style::default().fg(theme::cyan())),
        Span::styled(format!("{state:<7}"), data_space_state_style(active)),
        Span::styled(format!("{kind:<6}"), Style::default().fg(kind_color)),
        Span::styled(
            format!("{:<20}", review_snippet(&space.label, 20)),
            label_style,
        ),
        Span::styled(
            review_snippet(&space.detail, 42),
            Style::default().fg(theme::muted()),
        ),
    ])
}

fn data_space_state_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(theme::green())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::muted())
    }
}

fn data_space_detail_lines(space: &crate::core::dataspace::DataSpaceEntry) -> Vec<Line<'static>> {
    let mut lines = vec![
        detail_line("Name", &space.label, theme::text()),
        detail_line(
            "Kind",
            if space.is_local() {
                "Local source stores"
            } else {
                "SSH read-only inventory"
            },
            if space.is_local() {
                theme::cyan()
            } else {
                theme::orange()
            },
        ),
        detail_line("Target", &space.detail, theme::text()),
        detail_line(
            "Config",
            space.config_source.as_deref().unwrap_or("unknown"),
            theme::muted(),
        ),
    ];
    if let Some(path) = &space.config_path {
        lines.push(detail_line("Path", path, theme::muted()));
    }
    if space.is_local() {
        lines.push(detail_line(
            "Inventory",
            "reads local Codex / Claude / Hermes stores",
            theme::muted(),
        ));
    } else {
        lines.push(detail_line(
            "Inventory",
            &format!("ssh {} [moonbox|moon] sessions --json", space.detail),
            theme::muted(),
        ));
        lines.push(detail_line(
            "Safety",
            "read-only summary import; no remote resume or launch",
            theme::muted(),
        ));
    }
    lines
}

fn detail_line(label: &'static str, value: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), Style::default().fg(theme::muted())),
        Span::styled(value.to_owned(), Style::default().fg(color)),
    ])
}

fn render_help(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 52, 48);
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "Moonbox Keys",
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::raw("j/k, gg/G       navigate"),
        Line::raw("tab, shift-tab  switch panel"),
        Line::raw("f               cycle session source filter"),
        Line::raw("a               clear source and text filters"),
        Line::raw("d               open Local / SSH data space picker"),
        Line::raw("{ / }           previous / next data space"),
        Line::raw("s               star / unstar selected session"),
        Line::raw("*               star / unstar selected session alias"),
        Line::raw("/text           filter sessions by text"),
        Line::raw("o               preview original CLI resume"),
        Line::raw("enter           open original CLI, then return"),
        Line::raw("e               open selected Timeline event detail"),
        Line::raw("x / H           choose target for handoff"),
        Line::raw("D               open pre-flight details"),
        Line::raw("[ / ]           previous / next session source filter"),
        Line::raw("space           set rewind point"),
        Line::raw("c               refresh capsule and open handoff review"),
        Line::raw("v, S            verify capsule, switch skill"),
        Line::raw(",               open Settings"),
        Line::raw(":               command mode"),
        Line::raw("q / Ctrl-C      quit"),
        Line::raw("Esc             cancel command/search or close overlay"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Help ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_command_palette(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 78, 64);
    frame.render_widget(Clear, area);

    let matches = app.command_palette_matches();
    let selected = app.selected_command_palette_entry();
    let selected_index = if matches.is_empty() {
        0
    } else {
        app.command_selection.min(matches.len() - 1)
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                ": ",
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.command_input.clone(),
                Style::default().fg(theme::text()),
            ),
            Span::styled("▏", Style::default().fg(theme::cyan())),
        ]),
        Line::from(Span::styled(
            "Tab complete   Enter run selected   j/k choose   Esc close",
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
    ];

    if matches.is_empty() {
        lines.extend([
            Line::from(Span::styled(
                "No commands match",
                Style::default()
                    .fg(theme::red())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Try open, capsule, handoff, source, data, skill, doctor, or help.",
                Style::default().fg(theme::muted()),
            )),
        ]);
    } else {
        lines.push(Line::from(Span::styled(
            "Matches",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )));
        for (index, entry) in matches.iter().take(8).enumerate() {
            lines.push(command_palette_row(entry, index == selected_index));
        }
        if matches.len() > 8 {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} more commands hidden by the current terminal height",
                    matches.len() - 8
                ),
                Style::default().fg(theme::muted()),
            )));
        }
        if let Some(entry) = selected {
            lines.extend(command_palette_details(entry));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Command Palette ", true))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn command_palette_row(entry: &CommandPaletteEntry, selected: bool) -> Line<'static> {
    let marker = if selected { "›" } else { " " };
    let command_style = if selected {
        Style::default()
            .fg(theme::text())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::text())
    };
    Line::from(vec![
        Span::styled(marker, Style::default().fg(theme::cyan())),
        Span::raw(" "),
        Span::styled(format!("{:<14}", entry.command), command_style),
        Span::styled(
            format!(" {:<8} ", entry.badge),
            command_palette_badge_style(entry),
        ),
        Span::styled(entry.description, Style::default().fg(theme::muted())),
    ])
}

fn command_palette_details(entry: &CommandPaletteEntry) -> Vec<Line<'static>> {
    let aliases = if entry.aliases.is_empty() {
        "-".into()
    } else {
        entry.aliases.join(", ")
    };
    let risk = if entry.dangerous {
        "Danger: exits Moonbox; no session is opened or launched."
    } else if entry.badge == "DRY-RUN" || entry.badge == "PREVIEW" || entry.badge == "REVIEW" {
        "Danger: no execute path; command opens a preview or review flow."
    } else {
        "Danger: none; command stays inside Moonbox."
    };
    vec![
        Line::raw(""),
        Line::from(Span::styled(
            "Selected command",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Params: ", Style::default().fg(theme::blue())),
            Span::styled(entry.params, Style::default().fg(theme::text())),
        ]),
        Line::from(vec![
            Span::styled("Aliases: ", Style::default().fg(theme::blue())),
            Span::styled(aliases, Style::default().fg(theme::muted())),
        ]),
        Line::from(vec![
            Span::styled("Risk: ", Style::default().fg(theme::blue())),
            Span::styled(
                risk,
                Style::default().fg(if entry.dangerous {
                    theme::red()
                } else {
                    theme::muted()
                }),
            ),
        ]),
    ]
}

fn command_palette_badge_style(entry: &CommandPaletteEntry) -> Style {
    let color = if entry.dangerous {
        theme::red()
    } else {
        match entry.badge {
            "CHECK" => theme::green(),
            "DRY-RUN" | "PREVIEW" | "REVIEW" => theme::gold(),
            "SWITCH" | "PICKER" => theme::cyan(),
            _ => theme::muted(),
        }
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn render_doctor(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 84, 88);
    frame.render_widget(Clear, area);
    let preflight = preflight_summary(app);
    let verification = app.launch_verification_for_target(app.data.target);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Pre-flight",
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                preflight.status.label(),
                Style::default()
                    .fg(preflight.status.color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", preflight.confidence.label()),
                Style::default()
                    .fg(preflight.confidence.color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Compiler: ", Style::default().fg(theme::blue())),
            Span::styled(
                compile_status_label(app.compile_status),
                Style::default()
                    .fg(verification_color(preflight.compiler_status))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", app.data.capsule.compiler),
                Style::default().fg(theme::muted()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Doctor: ", Style::default().fg(theme::blue())),
            Span::styled(
                app.doctor_report.status.to_string(),
                Style::default()
                    .fg(verification_color(app.doctor_report.status))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} checks", app.doctor_report.checks.len()),
                Style::default().fg(theme::muted()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Verify: ", Style::default().fg(theme::blue())),
            Span::styled(
                if preflight.verify_reviewed {
                    preflight.verify_status.to_string()
                } else {
                    "BLOCKED".into()
                },
                Style::default()
                    .fg(if preflight.verify_reviewed {
                        verification_color(preflight.verify_status)
                    } else {
                        theme::red()
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                verify_detail_text(verification.as_ref(), preflight.verify_reviewed),
                Style::default().fg(theme::muted()),
            ),
        ]),
        Line::from(Span::styled(
            "v Verify   r Refresh doctor   y Copy JSON   Esc Close",
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "Verifier evidence",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(preflight_readiness_lines(
        verification.as_ref(),
        3,
        app.effective_language(),
    ));
    lines.extend([
        Line::raw(""),
        Line::from(Span::styled(
            "Environment doctor",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
    ]);

    for check in &app.doctor_report.checks {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", check.status),
                Style::default()
                    .fg(verification_color(check.status))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &check.name,
                Style::default()
                    .fg(theme::text())
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().fg(theme::muted())),
            Span::styled(&check.detail, Style::default().fg(theme::muted())),
        ]));
        lines.push(Line::raw(""));
    }

    lines.extend([
        Line::from(Span::styled(
            "Read-only diagnostics. No timeline load, resume, launch, or target spawn.",
            Style::default().fg(theme::muted()),
        )),
        Line::from(Span::styled(
            "v Verify   r Refresh doctor   y Copy JSON   Esc Close",
            Style::default().fg(theme::muted()),
        )),
    ]);

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Pre-flight ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn verify_detail_text(report: Option<&VerificationReport>, reviewed: bool) -> String {
    if !reviewed {
        return "  needs review".into();
    }
    match report {
        Some(report) => format!("  {} checks", report.checks.len()),
        None => "  no session selected".into(),
    }
}

fn render_skill_picker(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 78, 72);
    frame.render_widget(Clear, area);
    let language = app.effective_language();
    let catalog = compiler::compiler_catalog_entries();
    let mut lines = vec![
        Line::from(Span::styled(
            i18n::text(language, Text::ChooseCompilerSkill),
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            i18n::text(language, Text::SkillPickerSubtitle),
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
    ];

    for index in app.skill_picker_candidate_indices() {
        let Some(id) = app.data.compilers.get(index) else {
            continue;
        };
        let info = catalog
            .iter()
            .find(|entry| entry.id == *id)
            .cloned()
            .unwrap_or_else(|| fallback_compiler_info(id));
        let pending = index == app.pending_compiler;
        let active = app.compiler_selection_matches(index, app.selected_compiler);
        let status_color = skill_picker_status_color(&info);
        let row_style = if pending {
            Style::default()
                .fg(Color::Black)
                .bg(status_color)
                .add_modifier(Modifier::BOLD)
        } else if active {
            Style::default()
                .fg(theme::text())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::text())
        };
        let muted_style = if pending {
            Style::default().fg(theme::text())
        } else {
            Style::default().fg(theme::muted())
        };
        let cursor = if pending { ">" } else { " " };
        let active_mark = if active {
            i18n::text(language, Text::Active)
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{cursor} "), row_style),
            Span::styled(format!("{:<24}", skill_picker_row_title(&info)), row_style),
            Span::styled("  ", row_style),
            Span::styled(
                format!("{:<7}", skill_picker_status_label(&info, language)),
                row_style,
            ),
            Span::styled("  ", row_style),
            Span::styled(
                format!("{:<15}", compiler_kind_label(info.kind, language)),
                row_style,
            ),
            Span::styled("  ", row_style),
            Span::styled(active_mark, row_style),
        ]));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(skill_picker_description(&info, language), muted_style),
        ]));
        lines.extend(compiler_detail_lines(&info, language, muted_style));
        lines.push(Line::raw(""));
    }

    if app.data.compilers.is_empty() {
        lines.push(Line::from(Span::styled(
            i18n::text(language, Text::NoCompilerSkillsConfigured),
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        )));
    }
    let apply_label = if app.show_launch {
        localized(language, "Apply + generate", "应用并生成")
    } else {
        i18n::text(language, Text::Apply)
    };
    lines.push(Line::from(Span::styled(
        format!(
            "j/k {}   enter {}   y {}   q {}",
            i18n::text(language, Text::Choose),
            apply_label,
            i18n::text(language, Text::CopyLinkCommand),
            i18n::text(language, Text::Close)
        ),
        Style::default().fg(theme::muted()),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(dynamic_panel_block(
                format!(" {} ", i18n::text(language, Text::SkillPicker)),
                true,
            ))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_capsules(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 96, 78);
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(Span::styled(
            "Saved Capsules",
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Local continuation objects. Listing never opens, resumes, or launches source sessions.",
            Style::default().fg(theme::muted()),
        )),
        Line::raw(""),
    ];

    if let Some(error) = &app.saved_capsule_error {
        lines.push(Line::from(Span::styled(
            "Capsule store unavailable",
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            review_snippet(error, 120),
            Style::default().fg(theme::muted()),
        )));
    } else if app.saved_capsules.is_empty() {
        lines.push(Line::from(Span::styled(
            "No saved Capsules",
            Style::default()
                .fg(theme::muted())
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Use `moonbox capsule save <name> --session <id> --target <cli>` to create one.",
            Style::default().fg(theme::muted()),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Name", Style::default().fg(theme::blue())),
            Span::styled(
                "                     Target   Source session              Updated",
                Style::default().fg(theme::muted()),
            ),
        ]));
        for capsule in &app.saved_capsules {
            let source_color = source_tool_color(capsule.source_cli);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<24}", review_snippet(&capsule.name, 24)),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<8}", capsule.target_cli.id()),
                    Style::default().fg(theme::role_target()),
                ),
                Span::styled(
                    format!("{:<28}", review_snippet(&capsule.source_session, 28)),
                    Style::default().fg(source_color),
                ),
                Span::styled(&capsule.updated_at, Style::default().fg(theme::muted())),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("rewind ", Style::default().fg(theme::muted())),
                Span::styled(
                    review_snippet(&capsule.rewind_point, 52),
                    Style::default().fg(theme::role_rewind()),
                ),
                Span::styled("  checksum ", Style::default().fg(theme::muted())),
                Span::styled(&capsule.checksum, Style::default().fg(theme::cyan())),
            ]));
            lines.push(Line::raw(""));
        }
    }
    lines.push(Line::from(Span::styled(
        "r refresh   Esc/q close",
        Style::default().fg(theme::muted()),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Capsule Inventory ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_timeline_detail(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 88, 82);
    frame.render_widget(Clear, area);
    let visible_groups = visible_timeline_groups(app);
    let selected_group = selected_timeline_group_position(&visible_groups, app.selected_event);
    let Some(group) = visible_groups.get(selected_group) else {
        frame.render_widget(
            Paragraph::new(vec![Line::from(Span::styled(
                "No timeline event selected",
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            ))])
            .block(panel_block(" Timeline Detail ", true)),
            area,
        );
        return;
    };

    let mut lines = timeline_detail_group_header_lines(group, app);
    for (position, (_, event)) in group.events().enumerate() {
        if group.len() > 1 {
            if position > 0 {
                lines.push(Line::raw(""));
            }
            lines.push(timeline_detail_event_header_line(
                position + 1,
                group.len(),
                event,
            ));
        }
        lines.extend(timeline_detail_event_lines(
            event,
            group.len() == 1,
            &app.timeline_image_previews,
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "j/k scroll   PgUp/PgDn page   Esc/q close",
        Style::default().fg(theme::muted()),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Timeline Detail ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn timeline_detail_group_header_lines(group: &TimelineGroup<'_>, app: &App) -> Vec<Line<'static>> {
    let is_rewind = group.is_rewind(&app.rewind_event_id);
    let (label, color) = timeline_group_label(group, is_rewind, app.data.source);
    let primary = group.primary_event();
    let id = if group.len() == 1 {
        primary.id.clone()
    } else {
        format!("{}..{}", primary.id, group.last_event().id)
    };
    let kind = if group.len() == 1 {
        timeline_kind_label(primary.kind).to_owned()
    } else {
        format!("{label} group")
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{id} "),
            Style::default()
                .fg(theme::cyan())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            kind,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            timeline_group_time(group),
            Style::default().fg(theme::muted()),
        ),
    ])];
    if group.len() == 1 {
        lines.push(Line::from(vec![
            Span::styled("Title: ", Style::default().fg(theme::muted())),
            Span::styled(primary.title.clone(), Style::default().fg(theme::text())),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Events: ", Style::default().fg(theme::muted())),
            Span::styled(
                group.len().to_string(),
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));
    }
    lines
}

fn timeline_detail_event_header_line(
    position: usize,
    total: usize,
    event: &TimelineEvent,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{position}/{total} "),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(
            format!("{} ", event.id),
            Style::default()
                .fg(theme::cyan())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            timeline_kind_label(event.kind),
            Style::default()
                .fg(timeline_kind_color(event.kind))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(event.time.clone(), Style::default().fg(theme::muted())),
    ])
}

fn timeline_detail_event_lines(
    event: &TimelineEvent,
    include_section_labels: bool,
    image_previews: &[TimelineImagePreview],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if !event.metadata.attachments.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Attachments",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )));
        for attachment in &event.metadata.attachments {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    timeline_attachment_label(attachment),
                    Style::default().fg(theme::cyan()),
                ),
            ]));
        }
    }
    let event_image_previews = image_previews
        .iter()
        .filter(|preview| preview.event_id == event.id)
        .collect::<Vec<_>>();
    if !event_image_previews.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Image Preview",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )));
        for preview in event_image_previews {
            lines.extend(timeline_image_preview_lines(preview));
        }
    }

    if include_section_labels {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Body",
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )));
    }
    lines.extend(timeline_detail_body_lines(&event.detail));
    lines
}

fn timeline_image_preview_lines(preview: &TimelineImagePreview) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled(
            preview.label.clone(),
            Style::default()
                .fg(theme::cyan())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            timeline_image_preview_status(preview),
            Style::default().fg(timeline_image_preview_status_color(preview)),
        ),
    ])];
    if let Some((width, height)) = preview.dimensions {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{width}x{height}"),
                Style::default().fg(theme::muted()),
            ),
            Span::raw("  "),
            Span::styled(
                preview.path.clone().unwrap_or_default(),
                Style::default().fg(theme::muted()),
            ),
        ]));
    } else if let Some(path) = &preview.path {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(path.clone(), Style::default().fg(theme::muted())),
        ]));
    }
    if preview.is_rendered() {
        for row in &preview.rows {
            lines.push(Line::from(
                std::iter::once(Span::raw("  "))
                    .chain(row.iter().map(timeline_image_preview_cell_span))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    lines
}

fn timeline_image_preview_cell_span(cell: &PreviewCell) -> Span<'static> {
    let top = preview_color(cell.top);
    let bottom = cell
        .bottom
        .map(preview_color)
        .unwrap_or(Color::Rgb(20, 24, 32));
    Span::styled("▀", Style::default().fg(top).bg(bottom))
}

fn preview_color(color: PreviewRgb) -> Color {
    Color::Rgb(color.red, color.green, color.blue)
}

fn timeline_image_preview_status(preview: &TimelineImagePreview) -> String {
    match &preview.status {
        ImagePreviewStatus::Rendered => "rendered".into(),
        ImagePreviewStatus::MissingPath => "no local artifact path".into(),
        ImagePreviewStatus::UnsupportedPath(reason) => format!("not previewable: {reason}"),
        ImagePreviewStatus::TooLarge { bytes, limit } => {
            format!(
                "too large: {} / {}",
                format_bytes(*bytes),
                format_bytes(*limit)
            )
        }
        ImagePreviewStatus::DecodeError(error) => {
            format!("decode failed: {}", review_snippet(error, 72))
        }
    }
}

fn timeline_image_preview_status_color(preview: &TimelineImagePreview) -> Color {
    match preview.status {
        ImagePreviewStatus::Rendered => theme::green(),
        ImagePreviewStatus::MissingPath | ImagePreviewStatus::UnsupportedPath(_) => theme::muted(),
        ImagePreviewStatus::TooLarge { .. } | ImagePreviewStatus::DecodeError(_) => theme::orange(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn timeline_detail_body_lines(detail: &str) -> Vec<Line<'static>> {
    if detail.trim().is_empty() {
        return vec![Line::from(Span::styled(
            "(empty)",
            Style::default().fg(theme::muted()),
        ))];
    }
    detail
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line.to_owned(),
                Style::default().fg(theme::text()),
            ))
        })
        .collect()
}

fn timeline_kind_label(kind: TimelineKind) -> &'static str {
    match kind {
        TimelineKind::User => "USER",
        TimelineKind::Assistant => "ASSISTANT",
        TimelineKind::Tool => "TOOL",
        TimelineKind::Compact => "COMPACT",
        TimelineKind::Error => "ERROR",
        TimelineKind::GitDiff => "GIT DIFF",
        TimelineKind::RewindPoint => "REWIND",
    }
}

fn timeline_kind_color(kind: TimelineKind) -> Color {
    match kind {
        TimelineKind::User => theme::blue(),
        TimelineKind::Assistant => theme::gold(),
        TimelineKind::Tool => theme::muted(),
        TimelineKind::Compact => theme::cyan(),
        TimelineKind::Error => theme::red(),
        TimelineKind::GitDiff => theme::green(),
        TimelineKind::RewindPoint => theme::role_rewind(),
    }
}

fn fallback_compiler_info(id: &str) -> CompilerPresetInfo {
    CompilerPresetInfo {
        id: id.into(),
        kind: CompilerPresetKind::Config,
        status: CompilerPresetStatus::Warning,
        score: 0,
        command: None,
        args: Vec::new(),
        timeout_ms: None,
        reason: "compiler id is listed but missing from catalog".into(),
        description: None,
        homepage: None,
        github_stars: None,
    }
}

fn skill_picker_row_title(info: &CompilerPresetInfo) -> String {
    handoff::parse_compiler_id(&info.id)
        .map(|spec| spec.skill_id)
        .unwrap_or_else(|| info.id.clone())
}

fn skill_picker_status_label(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> &'static str {
    if handoff::parse_compiler_id(&info.id).is_some() {
        if compiler::compiler_skill_path(info).is_some() {
            return localized(language, "INSTALLED", "已安装");
        }
        return localized(language, "INSTALL", "安装");
    }
    compiler_status_label(info.status, language)
}

fn skill_picker_status_color(info: &CompilerPresetInfo) -> Color {
    if handoff::parse_compiler_id(&info.id).is_some() {
        if compiler::compiler_skill_path(info).is_some() {
            return theme::green();
        }
        return theme::gold();
    }
    compiler_status_color(info.status)
}

fn compiler_status_label(
    status: CompilerPresetStatus,
    language: crate::core::config::UiLanguage,
) -> &'static str {
    match status {
        CompilerPresetStatus::Ready => i18n::text(language, Text::Ready),
        CompilerPresetStatus::Warning => i18n::text(language, Text::Warning),
        CompilerPresetStatus::Disabled => i18n::text(language, Text::DisabledStatus),
    }
}

fn compiler_status_color(status: CompilerPresetStatus) -> Color {
    match status {
        CompilerPresetStatus::Ready => theme::green(),
        CompilerPresetStatus::Warning => theme::gold(),
        CompilerPresetStatus::Disabled => theme::muted(),
    }
}

fn compiler_kind_label(
    kind: CompilerPresetKind,
    language: crate::core::config::UiLanguage,
) -> &'static str {
    match kind {
        CompilerPresetKind::Builtin => i18n::text(language, Text::BuiltinKind),
        CompilerPresetKind::Environment => i18n::text(language, Text::EnvironmentKind),
        CompilerPresetKind::Config => i18n::text(language, Text::ConfigKind),
        CompilerPresetKind::Agent => i18n::text(language, Text::AgentKind),
    }
}

fn compiler_detail_lines(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
    muted_style: Style,
) -> Vec<Line<'static>> {
    if handoff::parse_compiler_id(&info.id).is_some() {
        return agent_skill_detail_lines(info, language);
    }

    match info.kind {
        CompilerPresetKind::Builtin => vec![compiler_detail_line(
            language,
            "Use",
            "用途",
            localized(
                language,
                "Built-in fallback draft; it does not call an AI skill. Prefer a Skill above for production handoff.",
                "内置 fallback 草稿，不调用 AI skill；正式 handoff 请选上方 Skill。",
            )
            .into(),
            muted_style,
        )],
        CompilerPresetKind::Environment | CompilerPresetKind::Config => {
            let mut lines = Vec::new();
            if let Some(command) = compiler_command(info) {
                lines.push(compiler_detail_line(
                    language,
                    "Command",
                    "命令",
                    command,
                    Style::default().fg(theme::cyan()),
                ));
            }
            lines.push(compiler_detail_line(
                language,
                "Status",
                "状态",
                compiler_status_detail(info, language),
                compiler_status_detail_style(info),
            ));
            if let Some(homepage) = &info.homepage {
                lines.push(compiler_detail_line(
                    language,
                    "Link",
                    "链接",
                    homepage.clone(),
                    Style::default().fg(theme::cyan()),
                ));
            }
            if let Some(stars) = info.github_stars {
                lines.push(compiler_detail_line(
                    language,
                    "Stars",
                    "热度",
                    format_stars(stars),
                    Style::default().fg(theme::gold()),
                ));
            }
            lines
        }
        CompilerPresetKind::Agent => Vec::new(),
    }
}

fn agent_skill_detail_lines(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> Vec<Line<'static>> {
    if let Some(path) = compiler::compiler_skill_path(info) {
        let mut lines = Vec::new();
        if let Some(provider) = skill_provider_label(info, language) {
            lines.push(compiler_detail_line(
                language,
                "Provider",
                "提供方",
                provider,
                Style::default().fg(theme::gold()),
            ));
        }
        if let Some(homepage) = &info.homepage {
            lines.push(compiler_detail_line(
                language,
                "Link",
                "链接",
                homepage.clone(),
                Style::default().fg(theme::cyan()),
            ));
        }
        lines.extend([
            compiler_detail_line(
                language,
                "Status",
                "状态",
                localized(language, "Installed locally", "本机已安装").into(),
                Style::default().fg(theme::green()),
            ),
            compiler_detail_line(
                language,
                "Path",
                "路径",
                path.into(),
                Style::default().fg(theme::cyan()),
            ),
        ]);
        return lines;
    }

    let mut lines = Vec::new();
    if let Some(provider) = skill_provider_label(info, language) {
        lines.push(compiler_detail_line(
            language,
            "Provider",
            "提供方",
            provider,
            Style::default().fg(theme::gold()),
        ));
    }
    lines.push(compiler_detail_line(
        language,
        "Status",
        "状态",
        localized(language, "Skill not installed", "Skill 未安装").into(),
        Style::default().fg(theme::gold()),
    ));
    if let Some(homepage) = &info.homepage {
        lines.push(compiler_detail_line(
            language,
            "Install",
            "安装",
            homepage.clone(),
            Style::default().fg(theme::cyan()),
        ));
    }
    lines
}

fn skill_provider_label(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> Option<String> {
    let homepage = info.homepage.as_deref()?;
    if let Some(provider) = github_provider_label(homepage) {
        return Some(match language {
            crate::core::config::UiLanguage::English => format!("{provider} (third-party)"),
            crate::core::config::UiLanguage::ZhHans => format!("{provider}（三方）"),
        });
    }
    Some(localized(language, "Third-party skill", "三方 Skill").into())
}

fn github_provider_label(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let owner = parts.next()?;
    let repo = parts.next()?;
    Some(format!("{owner}/{repo}"))
}

fn compiler_detail_line(
    language: crate::core::config::UiLanguage,
    english_label: &'static str,
    zh_label: &'static str,
    value: String,
    value_style: Style,
) -> Line<'static> {
    Line::from(vec![
        compiler_detail_label(language, english_label, zh_label),
        Span::styled(value, value_style),
    ])
}

fn compiler_detail_label(
    language: crate::core::config::UiLanguage,
    english_label: &'static str,
    zh_label: &'static str,
) -> Span<'static> {
    Span::styled(
        format!("    {}: ", localized(language, english_label, zh_label)),
        Style::default()
            .fg(theme::blue())
            .add_modifier(Modifier::BOLD),
    )
}

fn compiler_status_detail(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> String {
    if info.status == CompilerPresetStatus::Ready {
        return localized(language, "Ready to run", "可运行").into();
    }
    compiler_setup_hint(info, language)
}

fn compiler_status_detail_style(info: &CompilerPresetInfo) -> Style {
    Style::default().fg(compiler_status_color(info.status))
}

fn compiler_setup_hint(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> String {
    let reason = info.reason.as_str();
    if reason.contains("skill_not_installed") {
        return localized(
            language,
            "Install a generic handoff skill; press y to copy the install link.",
            "安装通用 handoff skill；按 y 复制安装链接。",
        )
        .into();
    }
    if reason.contains("sdk_not_found:") {
        let install = compiler_reason_field(reason, "install").unwrap_or_else(|| {
            if reason.contains("runner=Claude") {
                "python3 -m pip install claude-agent-sdk".into()
            } else {
                "python3 -m pip install openai-codex".into()
            }
        });
        return if language == crate::core::config::UiLanguage::ZhHans {
            format!("CLI 已安装，但 Python SDK 未被 Moonbox 找到。执行：{install}")
        } else {
            format!("CLI is installed, but the Python SDK is not visible. Run: {install}")
        };
    }
    if reason.contains("python_command_not_found:") {
        let env =
            compiler_reason_field(reason, "env").unwrap_or_else(|| "MOONBOX_*_SDK_PYTHON".into());
        return if language == crate::core::config::UiLanguage::ZhHans {
            format!("配置的 Python 找不到；请修正 {env}。")
        } else {
            format!("Configured Python was not found; fix {env}.")
        };
    }
    if reason.contains("install the Codex SDK") {
        return localized(
            language,
            "Install Codex SDK: pip install openai-codex",
            "安装 Codex SDK：pip install openai-codex",
        )
        .into();
    }
    if reason.contains("install the Claude Agent SDK") {
        return localized(
            language,
            "Install Claude Agent SDK: pip install claude-agent-sdk",
            "安装 Claude Agent SDK：pip install claude-agent-sdk",
        )
        .into();
    }
    if reason.contains("Codex SDK runner not installed")
        || reason.contains("Codex SDK runner command was not found")
    {
        return localized(
            language,
            "Install Codex SDK or set MOONBOX_CODEX_SDK_PYTHON.",
            "安装 Codex SDK，或设置 MOONBOX_CODEX_SDK_PYTHON。",
        )
        .into();
    }
    if reason.contains("Claude SDK runner not installed")
        || reason.contains("Claude SDK runner command was not found")
    {
        return localized(
            language,
            "Install Claude Agent SDK or set MOONBOX_CLAUDE_AGENT_SDK_PYTHON.",
            "安装 Claude Agent SDK，或设置 MOONBOX_CLAUDE_AGENT_SDK_PYTHON。",
        )
        .into();
    }
    if reason.contains("auth_required: Codex") {
        return localized(
            language,
            "Sign in to Codex or provide OPENAI_API_KEY.",
            "登录 Codex，或提供 OPENAI_API_KEY。",
        )
        .into();
    }
    if reason.contains("auth_required: Claude") {
        return localized(
            language,
            "Sign in to Claude or provide ANTHROPIC_API_KEY.",
            "登录 Claude，或提供 ANTHROPIC_API_KEY。",
        )
        .into();
    }
    localize_compiler_reason(language, &review_snippet(reason, 120))
}

fn localized(
    language: crate::core::config::UiLanguage,
    english: &'static str,
    zh_hans: &'static str,
) -> &'static str {
    match language {
        crate::core::config::UiLanguage::English => english,
        crate::core::config::UiLanguage::ZhHans => zh_hans,
    }
}

fn compiler_command(info: &CompilerPresetInfo) -> Option<String> {
    info.command.as_ref().map(|command| {
        if info.args.is_empty() {
            command.clone()
        } else {
            format!("{} {}", command, info.args.join(" "))
        }
    })
}

fn skill_picker_description(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> String {
    if handoff::parse_compiler_id(&info.id).is_some()
        && let Some(description) = info.description.as_deref()
    {
        if description.ends_with(" runner placeholder for the community `handoff` skill.") {
            return localized(
                language,
                "Community handoff skill for transferring context to another agent.",
                "社区 handoff skill，用于把上下文交给另一个 agent 接手。",
            )
            .into();
        }
        if let Some((_, skill_description)) =
            description.split_once(" runner using community handoff skill: ")
        {
            return localized_external_skill_description_for_language(language, skill_description);
        }
    }
    compiler_description(info, language)
}

fn compiler_description(
    info: &CompilerPresetInfo,
    language: crate::core::config::UiLanguage,
) -> String {
    if language == crate::core::config::UiLanguage::ZhHans
        && let Some(description) = localized_compiler_description(info)
    {
        return description;
    }
    info.description
        .clone()
        .unwrap_or_else(|| localize_compiler_reason(language, &review_snippet(&info.reason, 96)))
}

fn localized_compiler_description(info: &CompilerPresetInfo) -> Option<String> {
    let description = info.description.as_deref()?;
    let localized = match info.id.as_str() {
        "engineering-handoff" => "通用跨 CLI continuation 的草稿 handoff capsule。".to_string(),
        "bugfix-continuation" => "从选定 rewind point 继续 bugfix 工作的草稿 capsule。".to_string(),
        "design-review" => "用于设计评审和架构跟进的草稿 capsule。".to_string(),
        _ if description == "Environment-provided compiler skill." => {
            "由环境变量提供的 compiler skill。".to_string()
        }
        _ if description.ends_with(" runner placeholder for the community `handoff` skill.") => {
            let runner = description
                .strip_suffix(" runner placeholder for the community `handoff` skill.")
                .unwrap_or("Agent");
            format!("{runner} runner 的社区 `handoff` skill 占位项。")
        }
        _ if description.contains(" runner using community handoff skill: ") => {
            let (runner, skill_description) =
                description.split_once(" runner using community handoff skill: ")?;
            format!(
                "{runner} runner 使用社区 handoff skill：{}",
                localized_external_skill_description(skill_description)
            )
        }
        _ => return None,
    };
    Some(localized)
}

fn localized_external_skill_description(description: &str) -> String {
    localized_external_skill_description_for_language(
        crate::core::config::UiLanguage::ZhHans,
        description,
    )
}

fn localized_external_skill_description_for_language(
    language: crate::core::config::UiLanguage,
    description: &str,
) -> String {
    match description {
        "Compact the current conversation into a handoff document for another agent to pick up." => {
            localized(
                language,
                "Compact the current conversation into a handoff document for another agent to pick up.",
                "将当前对话压缩成 handoff 文档，交给另一个 agent 接手。",
            )
            .into()
        }
        other => other.into(),
    }
}

fn localize_compiler_reason(language: crate::core::config::UiLanguage, reason: &str) -> String {
    if language == crate::core::config::UiLanguage::ZhHans {
        if reason == "compiler id is listed but missing from catalog" {
            return "配置中列出了这个 compiler id，但 catalog 中找不到。".into();
        }
        if reason == "compiler command was not found on disk or PATH" {
            return "compiler 命令在磁盘或 PATH 中找不到。".into();
        }
    }
    reason.into()
}

fn launch_failure_reason_lines(
    language: crate::core::config::UiLanguage,
    message: &str,
) -> Vec<Line<'static>> {
    if message.contains("sdk_not_found:") {
        let runner = compiler_reason_field(message, "runner").unwrap_or_else(|| "Agent".into());
        let cli = compiler_reason_field(message, "cli").unwrap_or_else(|| "not_found".into());
        let module = compiler_reason_field(message, "module").unwrap_or_else(|| "SDK".into());
        let checked = compiler_reason_field(message, "checked").unwrap_or_else(|| "none".into());
        let install = compiler_reason_field(message, "install")
            .unwrap_or_else(|| "python3 -m pip install <sdk>".into());
        let env =
            compiler_reason_field(message, "env").unwrap_or_else(|| "MOONBOX_*_SDK_PYTHON".into());

        return vec![
            reason_kv_line(
                localized(language, "CLI", "CLI"),
                if cli == "not_found" {
                    localized(language, "not found", "未找到").into()
                } else {
                    cli
                },
            ),
            reason_kv_line(
                localized(language, "Missing SDK module", "缺少 SDK 模块"),
                module,
            ),
            reason_kv_line(
                localized(language, "Checked Python", "已检查 Python"),
                checked.replace(',', ", "),
            ),
            reason_kv_line(localized(language, "Install", "安装"), install),
            reason_kv_line(
                localized(language, "Other venv", "其他 venv"),
                if language == crate::core::config::UiLanguage::ZhHans {
                    format!("设置 {env}=/path/to/python")
                } else {
                    format!("set {env}=/path/to/python")
                },
            ),
            Line::from(Span::styled(
                if language == crate::core::config::UiLanguage::ZhHans {
                    format!("仅安装 {runner} CLI 还不足以运行当前 SDK runner。")
                } else {
                    format!("{runner} CLI alone is not enough for this SDK runner.")
                },
                Style::default().fg(theme::muted()),
            )),
        ];
    }

    if message.contains("python_command_not_found:") {
        let command = compiler_reason_field(message, "command").unwrap_or_else(|| "python".into());
        let env =
            compiler_reason_field(message, "env").unwrap_or_else(|| "MOONBOX_*_SDK_PYTHON".into());
        return vec![
            reason_kv_line(
                localized(language, "Configured Python", "配置的 Python"),
                command,
            ),
            reason_kv_line(
                localized(language, "Fix", "修复"),
                if language == crate::core::config::UiLanguage::ZhHans {
                    format!("安装该 Python，或更新 {env}")
                } else {
                    format!("install that Python or update {env}")
                },
            ),
        ];
    }

    vec![Line::from(Span::raw(localize_compiler_reason(
        language, message,
    )))]
}

fn compiler_reason_field(reason: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    reason.split(';').find_map(|part| {
        let part = part.trim();
        let start = part.find(&needle)? + needle.len();
        let value = part[start..].trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn reason_kv_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(value),
    ])
}

fn action_button<'a>(key: &'a str, label: &'a str) -> Span<'a> {
    Span::styled(
        format!(" {key} {label} "),
        Style::default()
            .fg(ratatui::style::Color::Black)
            .bg(theme::gold())
            .add_modifier(Modifier::BOLD),
    )
}

fn disabled_action_button<'a>(key: &'a str, label: &'a str) -> Span<'a> {
    Span::styled(
        format!(" {key} {label} "),
        Style::default()
            .fg(theme::muted())
            .bg(theme::border())
            .add_modifier(Modifier::BOLD),
    )
}

fn modal_scroll_offset(requested: u16, lines: &[Line<'_>], area: Rect) -> u16 {
    if requested != u16::MAX {
        return requested;
    }

    let width = usize::from(area.width.saturating_sub(2).max(1));
    let height = usize::from(area.height.saturating_sub(2).max(1));
    let rows = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum::<usize>();
    rows.saturating_sub(height).min(usize::from(u16::MAX - 1)) as u16
}

fn format_stars(stars: u64) -> String {
    if stars >= 1_000 {
        format!("{:.1}k", stars as f64 / 1_000.0)
    } else {
        stars.to_string()
    }
}

fn render_launch(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 76, 78);
    frame.render_widget(Clear, modal_area(root, 100, 60));
    frame.render_widget(Clear, area);
    let language = app.effective_language();
    let session = app
        .current_session()
        .map(|session| format!("{} / {}", session.cli, session.id))
        .unwrap_or_else(|| "No session selected".into());
    let handoff_label = app.launch_handoff_label();
    if app.is_launch_review_pending() {
        let mut lines = vec![
            Line::from(Span::styled(
                i18n::text(language, Text::GeneratingReview),
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Session)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(session),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Target)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    app.pending_target.to_string(),
                    Style::default()
                        .fg(theme::cyan())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        if let Some(status) = app.launch_review_job_status() {
            lines.extend([
                Line::from(vec![
                    Span::styled("Compiler: ", Style::default().fg(theme::blue())),
                    Span::styled(status.compiler_id, Style::default().fg(theme::cyan())),
                ]),
                Line::from(vec![
                    Span::styled("Stage: ", Style::default().fg(theme::blue())),
                    Span::styled(
                        status.stage_label,
                        Style::default()
                            .fg(theme::green())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("  {} ms", status.elapsed_ms)),
                ]),
                Line::from(vec![
                    Span::styled("Last: ", Style::default().fg(theme::blue())),
                    Span::raw(status.detail),
                ]),
            ]);
        }
        lines.extend([
            Line::from(Span::styled(
                if app.current_data_space().is_local() {
                    "Reading local source timeline and running the selected handoff skill..."
                } else {
                    "Reading SSH source timeline in read-only mode and running the local handoff skill..."
                },
                Style::default().fg(theme::text()),
            )),
            Line::raw(""),
        ]);
        if let Some(trail) = app.handoff_trail_frame() {
            lines.push(handoff_trail_line(trail));
        } else {
            lines.push(Line::from(Span::styled(
                "   source --> rewind --> target   Review",
                Style::default().fg(theme::muted()),
            )));
        }
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                i18n::text(language, Text::LoadingHiddenJob),
                Style::default().fg(theme::muted()),
            )),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(" Launch ", true))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if let Some(error) = app.launch_review_error() {
        let mut lines = vec![
            Line::from(Span::styled(
                localized(language, "Handoff Review Failed", "Handoff Review 失败"),
                Style::default()
                    .fg(theme::red())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Session)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(session),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Target)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    error.target.to_string(),
                    Style::default()
                        .fg(theme::cyan())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Skill)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    error.compiler_id.clone(),
                    Style::default().fg(theme::cyan()),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Runtime)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(format!("{} ms", error.elapsed_ms)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Action)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    localized(
                        language,
                        "The handoff was not generated; target launch is disabled.",
                        "handoff 没有生成，目标启动已禁用。",
                    ),
                    Style::default()
                        .fg(theme::red())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
            Line::from(Span::styled(
                localized(language, "Reason", "原因"),
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        lines.extend(launch_failure_reason_lines(language, &error.message));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                localized(
                    language,
                    "Next: press Enter to retry with the current skill, or S to choose another handoff skill.",
                    "下一步：按 Enter 用当前 skill 重试，或按 S 选择其他 handoff skill。",
                ),
                Style::default().fg(theme::gold()),
            )),
            Line::raw(""),
            Line::from(vec![
                action_button("Enter", i18n::text(language, Text::Retry)),
                Span::raw("  "),
                action_button("S", i18n::text(language, Text::Skill)),
                Span::raw("  "),
                disabled_action_button("y/r", i18n::text(language, Text::Unavailable)),
                Span::raw("  "),
                action_button("Esc", i18n::text(language, Text::Back)),
            ]),
            Line::from(Span::styled(
                i18n::text(language, Text::ScrollOnlyKeys),
                Style::default().fg(theme::muted()),
            )),
        ]);
        let scroll = modal_scroll_offset(app.modal_scroll, &lines, area);
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(
                    localized(language, " Handoff Review Failed ", " Handoff Review 失败 "),
                    true,
                ))
                .scroll((scroll, 0))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let pending_validation = app.validate_launch_for_target(app.pending_target);
    let pending_report = app.launch_verification_for_target(app.pending_target);
    if let Some(result) = &app.target_launch_result {
        let outcome_color = if result.success {
            theme::green()
        } else {
            theme::red()
        };
        let lines = vec![
            Line::from(Span::styled(
                if result.success {
                    "Launch Complete"
                } else {
                    "Launch Finished With Error"
                },
                Style::default()
                    .fg(outcome_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Result)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    result.outcome.clone(),
                    Style::default()
                        .fg(outcome_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Source)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(format!("{} {}", result.source, result.session_id)),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Target)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(result.target.to_string()),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Command)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    result.command_summary.clone(),
                    Style::default().fg(theme::cyan()),
                ),
            ]),
            Line::raw(""),
            Line::from(vec![
                action_button("r", i18n::text(language, Text::Rerun)),
                Span::raw("  "),
                action_button("y", i18n::text(language, Text::CopyCommand)),
                Span::raw("  "),
                action_button("Esc", i18n::text(language, Text::Back)),
            ]),
            Line::raw(""),
            Line::from(Span::styled(
                i18n::text(language, Text::TargetLaunchNoAuto),
                Style::default().fg(theme::muted()),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(" Target Launch Result ", true))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if app.launch_review {
        let capsule = app.launch_capsule_for_target(app.pending_target);
        let launch_blocked = pending_validation.state == LaunchValidationState::Blocked;
        let needs_handoff_skill = app.launch_requires_handoff_skill(app.pending_target);
        if needs_handoff_skill {
            let lines = vec![
                Line::from(Span::styled(
                    "Handoff Review",
                    Style::default()
                        .fg(theme::gold())
                        .add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Action)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::styled(
                        i18n::text(language, Text::HandoffSkillRequired),
                        Style::default()
                            .fg(theme::gold())
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(
                    i18n::text(language, Text::BuiltinDraftReviewRemoved),
                    Style::default().fg(theme::text()),
                )),
                Line::from(Span::styled(
                    i18n::text(language, Text::OpenSkillPickerBeforeReview),
                    Style::default().fg(theme::gold()),
                )),
                Line::raw(""),
                handoff_review_path_line(app),
                handoff_review_portrait_line(app),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Session)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::raw(session),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Target)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::raw(app.pending_target.to_string()),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Skill)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::styled(
                        selected_skill_label(app),
                        Style::default().fg(theme::cyan()),
                    ),
                ]),
                Line::raw(""),
                Line::from(vec![
                    action_button("S", i18n::text(language, Text::Skill)),
                    Span::raw("  "),
                    action_button("Enter", i18n::text(language, Text::SkillPicker)),
                    Span::raw("  "),
                    disabled_action_button("y/r", i18n::text(language, Text::Unavailable)),
                    Span::raw("  "),
                    action_button("Esc", i18n::text(language, Text::Back)),
                ]),
                Line::from(Span::styled(
                    i18n::text(language, Text::ScrollOnlyKeys),
                    Style::default().fg(theme::muted()),
                )),
            ];
            let scroll = modal_scroll_offset(app.modal_scroll, &lines, area);
            frame.render_widget(
                Paragraph::new(lines)
                    .block(panel_block(" Handoff Review ", true))
                    .scroll((scroll, 0))
                    .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        let can_regenerate_handoff = validation_can_regenerate_handoff(&pending_validation);
        if can_regenerate_handoff {
            let selected_skill = selected_skill_label(app);
            let lines = vec![
                Line::from(Span::styled(
                    "Handoff Review",
                    Style::default()
                        .fg(theme::gold())
                        .add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Action)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::styled(
                        i18n::text(language, Text::RegenerateHandoffReview),
                        Style::default()
                            .fg(theme::gold())
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(
                    i18n::text(language, Text::HandoffRegenerateRequired),
                    Style::default().fg(theme::text()),
                )),
                Line::from(Span::styled(
                    i18n::text(language, Text::RegenerateBeforeLaunch),
                    Style::default()
                        .fg(theme::gold())
                        .add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                handoff_review_path_line(app),
                handoff_review_portrait_line(app),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Session)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::raw(session),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Target)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::raw(app.pending_target.to_string()),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Skill)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::styled(selected_skill, Style::default().fg(theme::cyan())),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", i18n::text(language, Text::Validation)),
                        Style::default().fg(theme::blue()),
                    ),
                    Span::styled(
                        validation_label(language, pending_validation.state),
                        Style::default()
                            .fg(validation_color(pending_validation.state))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        validation_summary_text(language, &pending_validation),
                        Style::default().fg(theme::muted()),
                    ),
                ]),
                Line::raw(""),
                Line::from(vec![
                    action_button("Enter", i18n::text(language, Text::RegenerateHandoffReview)),
                    Span::raw("  "),
                    disabled_action_button("y/r", i18n::text(language, Text::Unavailable)),
                    Span::raw("  "),
                    action_button("Esc", i18n::text(language, Text::Back)),
                ]),
                Line::from(Span::styled(
                    i18n::text(language, Text::ScrollOnlyKeys),
                    Style::default().fg(theme::muted()),
                )),
            ];
            let scroll = modal_scroll_offset(app.modal_scroll, &lines, area);
            frame.render_widget(
                Paragraph::new(lines)
                    .block(panel_block(" Handoff Review ", true))
                    .scroll((scroll, 0))
                    .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        let draft_run_blocked = compiler::compiler_is_builtin(&capsule.compiler)
            && app
                .current_session()
                .is_some_and(|session| session.source_provenance != SourceProvenance::Fixture);
        let run_blocked = launch_blocked || draft_run_blocked;
        let mut lines = vec![
            Line::from(Span::styled(
                "Handoff Review",
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Action)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    "handoff",
                    Style::default()
                        .fg(theme::cyan())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                if run_blocked {
                    i18n::text(language, Text::NextCopyOnly).to_string()
                } else {
                    i18n::text(language, Text::NextRunCopy).to_string()
                },
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                if app.current_data_space().is_local() {
                    i18n::text(language, Text::LocalSourceOriginal)
                } else {
                    i18n::text(language, Text::SshSourceReadOnly)
                },
                Style::default().fg(theme::muted()),
            )),
            handoff_review_path_line(app),
            handoff_review_portrait_line(app),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Session)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(session),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Target)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(app.pending_target.to_string()),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Label)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(handoff_label),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::RewindPoint)),
                    Style::default().fg(theme::blue()),
                ),
                Span::raw(capsule.rewind_point.clone()),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::Validation)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    validation_label(language, pending_validation.state),
                    Style::default()
                        .fg(validation_color(pending_validation.state))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    validation_summary_text(language, &pending_validation),
                    Style::default().fg(theme::muted()),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n::text(language, Text::TargetCommand)),
                    Style::default().fg(theme::blue()),
                ),
                Span::styled(
                    app.target_launch_command_summary()
                        .unwrap_or_else(|| app.launch_command()),
                    Style::default().fg(theme::cyan()),
                ),
            ]),
            Line::raw(""),
            Line::from(Span::styled(
                i18n::text(language, Text::TargetReceives),
                Style::default()
                    .fg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        lines.extend(target_input_lines(app, language));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                if compiler::compiler_is_builtin(&capsule.compiler) {
                    "Draft Handoff"
                } else {
                    "Handoff Artifact"
                },
                Style::default()
                    .fg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )),
        ]);
        lines.extend(capsule_review_lines(&capsule, 1, language));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                i18n::text(language, Text::Readiness),
                Style::default()
                    .fg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )),
        ]);
        lines.extend(readiness_lines(pending_report.as_ref(), 6, language));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                i18n::text(language, Text::TargetContent),
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
        ]);
        lines.extend(target_prompt_lines(app));
        lines.extend([
            Line::raw(""),
            if launch_blocked {
                Line::from(vec![
                    disabled_action_button("r", i18n::text(language, Text::CannotRun)),
                    Span::raw("  "),
                    disabled_action_button("y", i18n::text(language, Text::CannotCopy)),
                    Span::raw("  "),
                    action_button("Esc", i18n::text(language, Text::Back)),
                    Span::styled(
                        format!("  {}", i18n::text(language, Text::ValidationFailed)),
                        Style::default().fg(theme::red()),
                    ),
                ])
            } else if draft_run_blocked {
                Line::from(vec![
                    disabled_action_button("r", i18n::text(language, Text::DraftCannotRun)),
                    Span::raw("  "),
                    action_button("y", i18n::text(language, Text::CopyCommand)),
                    Span::raw("  "),
                    action_button("Esc", i18n::text(language, Text::Back)),
                    Span::styled(
                        format!("  {}", i18n::text(language, Text::ChooseAiSkillToRun)),
                        Style::default().fg(theme::muted()),
                    ),
                ])
            } else {
                Line::from(vec![
                    action_button("r", i18n::text(language, Text::RunLocalTarget)),
                    Span::raw("  "),
                    action_button("y", i18n::text(language, Text::CopyCommand)),
                    Span::raw("  "),
                    action_button("Esc", i18n::text(language, Text::Back)),
                ])
            },
            Line::from(Span::styled(
                if run_blocked {
                    i18n::text(language, Text::ScrollOnlyKeys)
                } else {
                    i18n::text(language, Text::ReviewActionKeys)
                },
                Style::default().fg(theme::muted()),
            )),
        ]);
        let scroll = modal_scroll_offset(app.modal_scroll, &lines, area);
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(" Handoff Review ", true))
                .scroll((scroll, 0))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let mut target_lines = Vec::new();
    for target in CliTool::ALL {
        let selected = target == app.pending_target;
        let validation = app.validate_launch_for_target(target);
        let needs_handoff_skill = app.launch_requires_handoff_skill(target);
        let validation_state = if needs_handoff_skill {
            LaunchValidationState::Blocked
        } else {
            validation.state
        };
        let style = if selected {
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(validation_color(validation_state))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(validation_color(validation_state))
        };
        let muted_style = if selected {
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(validation_color(validation_state))
        } else {
            Style::default().fg(theme::muted())
        };
        let cursor = if selected { ">" } else { " " };
        let mark = if selected { "[x]" } else { "[ ]" };
        target_lines.push(Line::from(vec![
            Span::styled(format!("{cursor} {mark} {target:<6}"), style),
            Span::styled(
                format!("  {}", validation_label(language, validation_state)),
                style,
            ),
        ]));
        target_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                if needs_handoff_skill {
                    i18n::text(language, Text::HandoffSkillRequired).to_string()
                } else {
                    validation_summary_text(language, &validation)
                },
                muted_style,
            ),
        ]));
    }
    let mut lines = vec![
        Line::from(Span::styled(
            i18n::text(language, Text::ChooseTargetCli),
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                format!("{}: ", i18n::text(language, Text::Session)),
                Style::default().fg(theme::blue()),
            ),
            Span::raw(session),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            i18n::text(language, Text::Target),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(target_lines);
    let pending_needs_handoff_skill = app.launch_requires_handoff_skill(app.pending_target);
    let pending_validation_state = if pending_needs_handoff_skill {
        LaunchValidationState::Blocked
    } else {
        pending_validation.state
    };
    lines.extend([
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                format!("{}: ", i18n::text(language, Text::Selected)),
                Style::default().fg(theme::blue()),
            ),
            Span::raw(app.pending_target.to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}: ", i18n::text(language, Text::Validation)),
                Style::default().fg(theme::blue()),
            ),
            Span::styled(
                validation_label(language, pending_validation_state),
                Style::default()
                    .fg(validation_color(pending_validation_state))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                if pending_needs_handoff_skill {
                    i18n::text(language, Text::HandoffSkillRequired).to_string()
                } else {
                    validation_summary_text(language, &pending_validation)
                },
                Style::default().fg(theme::muted()),
            ),
        ]),
        Line::raw(""),
    ]);
    if pending_needs_handoff_skill {
        lines.extend([
            Line::from(Span::styled(
                i18n::text(language, Text::HandoffSkillRequired),
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                i18n::text(language, Text::BuiltinDraftReviewRemoved),
                Style::default().fg(theme::text()),
            )),
            Line::from(Span::styled(
                i18n::text(language, Text::OpenSkillPickerBeforeReview),
                Style::default().fg(theme::gold()),
            )),
        ]);
    } else {
        lines.push(Line::from(Span::styled(
            i18n::text(language, Text::Readiness),
            Style::default()
                .fg(theme::blue())
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(readiness_lines(pending_report.as_ref(), 6, language));
    }
    let can_regenerate_handoff = validation_can_regenerate_handoff(&pending_validation);
    lines.extend([
        if pending_needs_handoff_skill {
            Line::from(Span::styled(
                i18n::text(language, Text::HandoffSkillRequired),
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            ))
        } else if pending_validation.state == LaunchValidationState::Blocked {
            Line::from(Span::styled(
                if can_regenerate_handoff {
                    i18n::text(language, Text::RegenerateBeforeLaunch)
                } else {
                    i18n::text(language, Text::LaunchReviewDisabled)
                },
                Style::default()
                    .fg(if can_regenerate_handoff {
                        theme::gold()
                    } else {
                        theme::red()
                    })
                    .add_modifier(Modifier::BOLD),
            ))
        } else {
            Line::from(Span::styled(
                i18n::text(language, Text::ReviewBeforeCopy),
                Style::default().fg(theme::gold()),
            ))
        },
        Line::raw(""),
        Line::from(Span::styled(
            if pending_needs_handoff_skill {
                if language == crate::core::config::UiLanguage::ZhHans {
                    "j/k 选择目标   S/enter 选择 Skill   y 不可用   Esc 取消"
                } else {
                    "j/k choose target   S/enter choose skill   y unavailable   Esc cancel"
                }
            } else if pending_validation.state == LaunchValidationState::Blocked {
                if can_regenerate_handoff && language == crate::core::config::UiLanguage::ZhHans {
                    "j/k 选择目标   enter 重新生成   y 不可用   Esc 取消"
                } else if can_regenerate_handoff {
                    "j/k choose target   enter regenerate   y unavailable   Esc cancel"
                } else if language == crate::core::config::UiLanguage::ZhHans {
                    "j/k 选择目标   enter/y 已阻塞   Esc 取消"
                } else {
                    "j/k choose target   enter/y blocked   Esc cancel"
                }
            } else {
                if language == crate::core::config::UiLanguage::ZhHans {
                    "j/k 选择目标   enter 确认   y 不可用   Esc 取消"
                } else {
                    "j/k choose target   enter review   y unavailable   Esc cancel"
                }
            },
            Style::default().fg(theme::muted()),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .block(dynamic_panel_block(
                format!(" {} ", i18n::text(language, Text::Launch)),
                true,
            ))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn handoff_review_path_line(app: &App) -> Line<'static> {
    let session = app
        .current_session()
        .map(|session| {
            format!(
                "source {} {}",
                session.cli,
                short_identifier(&session.id, 14)
            )
        })
        .unwrap_or_else(|| "source no session".into());
    Line::from(vec![
        Span::styled("Path: ", Style::default().fg(theme::blue())),
        Span::styled(
            session,
            Style::default()
                .fg(theme::text())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" -> ", Style::default().fg(theme::border())),
        Span::styled(
            format!("rewind {}", short_identifier(&app.rewind_event_id, 12)),
            Style::default()
                .fg(theme::gold())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" -> ", Style::default().fg(theme::border())),
        Span::styled(
            format!("target {}", app.pending_target),
            Style::default()
                .fg(theme::cyan())
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn handoff_review_portrait_line(app: &App) -> Line<'static> {
    let portrait = app
        .current_session()
        .map(|session| session_portrait_detail(app, session))
        .unwrap_or_else(|| "no session selected".into());
    Line::from(vec![
        Span::styled("Portrait: ", Style::default().fg(theme::blue())),
        Span::styled(
            portrait,
            Style::default()
                .fg(theme::cyan())
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn capsule_review_lines(
    capsule: &WorkCapsule,
    _max_rows: usize,
    language: crate::core::config::UiLanguage,
) -> Vec<Line<'static>> {
    let decision = capsule
        .decisions
        .first()
        .map(|value| review_snippet(value, 88))
        .unwrap_or_else(|| "none".into());
    let todo = capsule
        .todo
        .first()
        .map(|item| {
            let mark = if item.done { "[x]" } else { "[ ]" };
            review_snippet(&format!("{mark} {}", item.text), 88)
        })
        .unwrap_or_else(|| "none".into());
    let risk = capsule
        .risks
        .first()
        .map(|value| review_snippet(value, 88))
        .unwrap_or_else(|| "none".into());
    let mut lines = vec![
        review_label_line(
            i18n::text(language, Text::Goal),
            review_snippet(&capsule.goal, 88),
            theme::blue(),
        ),
        review_label_line(
            i18n::text(language, Text::State),
            capsule.state.clone(),
            theme::gold(),
        ),
        review_label_line(
            i18n::text(language, Text::Decision),
            decision,
            theme::blue(),
        ),
        review_label_line(i18n::text(language, Text::Todo), todo, theme::blue()),
        review_label_line(i18n::text(language, Text::Risk), risk, theme::red()),
    ];
    if let Some(artifact) = &capsule.handoff_artifact {
        lines.push(review_label_line(
            "Handoff",
            review_snippet(artifact, 88),
            theme::green(),
        ));
    }
    lines
}

fn target_input_lines(app: &App, language: crate::core::config::UiLanguage) -> Vec<Line<'static>> {
    let Some(preview) = app.target_command_preview() else {
        return vec![Line::from(Span::styled(
            "No target input available for the current selection.",
            Style::default().fg(theme::muted()),
        ))];
    };
    let cwd = preview.cwd.unwrap_or_else(|| "terminal default".into());
    vec![
        review_label_line(
            i18n::text(language, Text::Program),
            preview.program,
            theme::blue(),
        ),
        review_label_line(i18n::text(language, Text::Directory), cwd, theme::blue()),
        review_label_line(
            i18n::text(language, Text::Arguments),
            format!(
                "{} {}",
                preview.args.len(),
                i18n::text(language, Text::ArgumentCountHandoff)
            ),
            theme::blue(),
        ),
        review_label_line(
            i18n::text(language, Text::Prompt),
            i18n::text(language, Text::PromptDisplayedBelow).into(),
            theme::blue(),
        ),
    ]
}

fn target_prompt_lines(app: &App) -> Vec<Line<'static>> {
    let Some(preview) = app.target_command_preview() else {
        return vec![Line::from(Span::styled(
            "No prompt available for the current selection.",
            Style::default().fg(theme::muted()),
        ))];
    };
    preview
        .prompt
        .lines()
        .map(|line| {
            Line::from(vec![
                Span::styled("> ", Style::default().fg(theme::muted())),
                Span::raw(line.to_string()),
            ])
        })
        .collect()
}

fn review_label_line(
    label: &'static str,
    value: String,
    color: ratatui::style::Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(value, Style::default().fg(theme::text())),
    ])
}

fn review_snippet(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index == max_chars {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}

fn validation_label(
    language: crate::core::config::UiLanguage,
    state: LaunchValidationState,
) -> &'static str {
    if language == crate::core::config::UiLanguage::ZhHans {
        return match state {
            LaunchValidationState::Ready => "就绪",
            LaunchValidationState::Warning => "警告",
            LaunchValidationState::Blocked => "阻塞",
        };
    }
    match state {
        LaunchValidationState::Ready => "READY",
        LaunchValidationState::Warning => "WARN",
        LaunchValidationState::Blocked => "BLOCKED",
    }
}

fn validation_summary_text(
    language: crate::core::config::UiLanguage,
    validation: &crate::core::model::LaunchValidation,
) -> String {
    validation
        .reasons
        .iter()
        .map(|reason| localize_validation_detail(language, reason))
        .collect::<Vec<_>>()
        .join("; ")
}

fn localize_validation_detail(language: crate::core::config::UiLanguage, detail: &str) -> String {
    if is_stale_handoff_compiler_mismatch(detail) {
        return i18n::text(language, Text::HandoffRegenerateRequired).to_string();
    }
    detail.to_string()
}

fn validation_can_regenerate_handoff(validation: &crate::core::model::LaunchValidation) -> bool {
    validation.state == LaunchValidationState::Blocked
        && !validation.reasons.is_empty()
        && validation
            .reasons
            .iter()
            .all(|reason| is_stale_handoff_compiler_mismatch(reason))
}

fn is_stale_handoff_compiler_mismatch(reason: &str) -> bool {
    reason.contains("raw source map mismatch")
        && reason.contains("generated_by ")
        && reason.contains(" vs compiler ")
}

fn validation_color(state: LaunchValidationState) -> Color {
    match state {
        LaunchValidationState::Ready => theme::green(),
        LaunchValidationState::Warning => theme::gold(),
        LaunchValidationState::Blocked => theme::red(),
    }
}

fn readiness_lines(
    report: Option<&VerificationReport>,
    _max_rows: usize,
    language: crate::core::config::UiLanguage,
) -> Vec<Line<'static>> {
    let Some(report) = report else {
        return vec![Line::from(vec![
            Span::styled(
                validation_label(language, LaunchValidationState::Blocked).to_string() + " ",
                Style::default()
                    .fg(theme::red())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("session", Style::default().fg(theme::text())),
            Span::styled("  No session selected", Style::default().fg(theme::muted())),
        ])];
    };

    let mut lines = Vec::new();
    for group in readiness_groups() {
        lines.push(Line::from(Span::styled(
            readiness_group_title(language, group.title),
            Style::default()
                .fg(group.color)
                .add_modifier(Modifier::BOLD),
        )));
        let checks = grouped_checks(report, group.names);
        lines.extend(
            checks
                .into_iter()
                .map(|check| readiness_check_line(check, language)),
        );
    }
    lines
}

fn preflight_readiness_lines(
    report: Option<&VerificationReport>,
    max_rows: usize,
    language: crate::core::config::UiLanguage,
) -> Vec<Line<'static>> {
    let Some(report) = report else {
        return readiness_lines(None, max_rows, language);
    };

    let checks = report
        .checks
        .iter()
        .filter(|check| check.status == VerificationStatus::Fail)
        .chain(
            report
                .checks
                .iter()
                .filter(|check| check.status == VerificationStatus::Warn),
        )
        .take(max_rows)
        .collect::<Vec<_>>();
    let checks = if checks.is_empty() {
        report.checks.iter().take(max_rows).collect::<Vec<_>>()
    } else {
        checks
    };
    let shown = checks.len();
    let mut lines = checks
        .into_iter()
        .map(|check| readiness_check_line(check, language))
        .collect::<Vec<_>>();
    let remaining = report.checks.len().saturating_sub(shown);
    if remaining > 0 {
        lines.push(Line::from(Span::styled(
            format!("  {remaining} more verifier checks"),
            Style::default().fg(theme::muted()),
        )));
    }
    lines
}

struct ReadinessGroup {
    title: &'static str,
    color: Color,
    names: &'static [&'static str],
}

fn readiness_groups() -> [ReadinessGroup; 5] {
    [
        ReadinessGroup {
            title: "Target Readiness",
            color: theme::green(),
            names: &["target_support", "target_command"],
        },
        ReadinessGroup {
            title: "Workspace Restore",
            color: theme::purple(),
            names: &["continuation_level", "package_import", "workspace_restore"],
        },
        ReadinessGroup {
            title: "Source Health",
            color: theme::blue(),
            names: &["source_health", "token_budget", "rewind_exists"],
        },
        ReadinessGroup {
            title: "Capsule Health",
            color: theme::gold(),
            names: &[
                "compiler_mode",
                "capsule_version",
                "capsule_required_fields",
                "capsule_source",
                "target_cli",
                "handoff_label",
                "handoff_label_namespace",
                "handoff_context",
                "risk_context",
                "redaction_policy",
                "capsule_size",
            ],
        },
        ReadinessGroup {
            title: "Semantic Evidence",
            color: theme::cyan(),
            names: &[
                "semantic_source_map",
                "semantic_compiler_coverage",
                "semantic_todo_timeline",
                "semantic_file_refs",
                "semantic_diff_applicability",
            ],
        },
    ]
}

fn readiness_group_title(
    language: crate::core::config::UiLanguage,
    title: &'static str,
) -> &'static str {
    if language == crate::core::config::UiLanguage::English {
        return title;
    }
    match title {
        "Target Readiness" => i18n::text(language, Text::TargetReadiness),
        "Workspace Restore" => i18n::text(language, Text::WorkspaceRestore),
        "Source Health" => i18n::text(language, Text::SourceHealth),
        "Capsule Health" => i18n::text(language, Text::CapsuleHealth),
        "Semantic Evidence" => i18n::text(language, Text::SemanticEvidence),
        _ => title,
    }
}

fn grouped_checks<'a>(
    report: &'a VerificationReport,
    names: &[&str],
) -> Vec<&'a crate::core::model::VerificationCheck> {
    let mut checks = names
        .iter()
        .filter_map(|name| report.checks.iter().find(|check| check.name == *name))
        .filter(|check| check.status != VerificationStatus::Pass)
        .collect::<Vec<_>>();
    if checks.is_empty() {
        checks = names
            .iter()
            .filter_map(|name| report.checks.iter().find(|check| check.name == *name))
            .take(2)
            .collect();
    }
    checks
}

fn readiness_check_line(
    check: &crate::core::model::VerificationCheck,
    language: crate::core::config::UiLanguage,
) -> Line<'static> {
    let color = verification_color(check.status);
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<5} ", check.status),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            check.name.clone(),
            Style::default()
                .fg(theme::text())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", localize_validation_detail(language, &check.detail)),
            Style::default().fg(theme::muted()),
        ),
    ])
}

fn verification_color(status: VerificationStatus) -> Color {
    match status {
        VerificationStatus::Pass => theme::green(),
        VerificationStatus::Warn => theme::gold(),
        VerificationStatus::Fail => theme::red(),
    }
}

fn render_open_original(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 72, 64);
    frame.render_widget(Clear, area);
    let lines = if let Some(session) = app.current_session() {
        vec![
            Line::from(Span::styled(
                "Open original session",
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled("CLI: ", Style::default().fg(theme::blue())),
                Span::raw(session.cli.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Session: ", Style::default().fg(theme::blue())),
                Span::raw(&session.id),
            ]),
            Line::from(vec![
                Span::styled("cwd: ", Style::default().fg(theme::blue())),
                Span::raw(&session.cwd),
            ]),
            Line::raw(""),
            Line::from(Span::styled(
                "Will run",
                Style::default()
                    .fg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                app.original_resume_display_command().unwrap_or_default(),
                Style::default().fg(theme::cyan()),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Copy wrapper",
                Style::default()
                    .fg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                app.original_open_command().unwrap_or_default(),
                Style::default().fg(theme::muted()),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Action: Moonbox hands this terminal to the original CLI, then returns.",
                Style::default().fg(theme::muted()),
            )),
            Line::from(Span::styled(
                "enter resume   y copy wrapper command   Esc close",
                Style::default().fg(theme::muted()),
            )),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                "No session selected",
                Style::default()
                    .fg(theme::gold())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Adjust filter or search, then try again.",
                Style::default().fg(theme::muted()),
            )),
        ]
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Open Original ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn panel_block(title: &'static str, focused: bool) -> Block<'static> {
    let color = if focused {
        theme::gold()
    } else {
        theme::border()
    };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .style(Style::default().fg(theme::text()))
        .padding(Padding::horizontal(1))
}

fn dynamic_panel_block(title: String, focused: bool) -> Block<'static> {
    let color = if focused {
        theme::gold()
    } else {
        theme::border()
    };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .style(Style::default().fg(theme::text()))
        .padding(Padding::horizontal(1))
}

fn stable_panel_title(content: String, area: Rect) -> String {
    let width = usize::from(area.width.saturating_sub(4)).clamp(18, 30);
    let clipped = content.chars().take(width).collect::<String>();
    format!(" {clipped:<width$} ")
}

fn key(label: &'static str) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(theme::blue())
            .add_modifier(Modifier::BOLD),
    )
}

fn txt(label: &'static str) -> Span<'static> {
    Span::styled(label, Style::default().fg(theme::muted()))
}

fn modal_area(root: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let width = if root.width < 100 { 100 } else { percent_x };
    let height = if root.height < 34 { 100 } else { percent_y };
    centered(root, width, height)
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        app::{App, LaunchReviewErrorState},
        core::{
            dataspace,
            model::{
                CliTool, SessionStatus, SourceProvenance, TimelineAttachment, TimelineEvent,
                TimelineKind, VerificationStatus,
            },
        },
    };

    fn render_text(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| render(frame, app)).expect("draw");
        format!("{}", terminal.backend())
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn screen_column(screen: &str, needle: &str) -> usize {
        screen
            .lines()
            .find_map(|line| line.find(needle).map(|index| line[..index].chars().count()))
            .unwrap_or_else(|| panic!("missing {needle:?} in screen:\n{screen}"))
    }

    fn render_loading_text(tick: usize, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_loading(frame, tick))
            .expect("draw");
        format!("{}", terminal.backend())
    }

    fn assert_screen_contains(screen: &str, expected: &str) {
        assert!(
            screen.contains(expected),
            "screen did not contain {expected:?}\n{screen}"
        );
    }

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
    }

    #[test]
    fn main_screen_renders_core_regions_across_viewports() {
        for (width, height) in [(140, 40), (80, 24)] {
            let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
            let screen = render_text(&app, width, height);

            assert_screen_contains(&screen, "MOONBOX");
            assert_screen_contains(&screen, "Sessions");
            assert_screen_contains(&screen, "Timeline");
            assert_screen_contains(&screen, "Session Details");
            assert_screen_contains(&screen, "Status");
            assert!(screen.chars().any(|ch| !ch.is_whitespace()));
        }
    }

    #[test]
    fn loading_screen_renders_animated_state() {
        let first = render_loading_text(0, 100, 30);
        let second = render_loading_text(1, 100, 30);

        assert_screen_contains(&first, "MOONBOX");
        assert_screen_contains(&first, "indexing source sessions");
        assert_screen_contains(&first, "bounded scan");
        assert_ne!(first, second);
    }

    #[test]
    fn session_list_window_keeps_render_work_bounded() {
        assert_eq!(session_list_window(0, 0, 20), (0, 0));
        assert_eq!(session_list_window(3, 1, 20), (0, 3));

        let (start, end) = session_list_window(5_000, 2_500, 22);
        assert!(start <= 2_500 && end > 2_500);
        assert!(end - start <= 14);

        let (start, end) = session_list_window(5_000, 4_999, 22);
        assert!(start <= 4_999 && end == 5_000);
        assert!(end - start <= 14);
    }

    #[test]
    fn session_panel_title_keeps_stable_width_across_filters() {
        let area = Rect::new(0, 0, 36, 10);
        let all = stable_panel_title("Sessions · All (2/231)".into(), area);
        let codex = stable_panel_title("Sessions · Codex (12/128)".into(), area);
        let hermes = stable_panel_title("Sessions · Hermes (1/2)".into(), area);

        assert_eq!(all.len(), codex.len());
        assert_eq!(codex.len(), hermes.len());
    }

    #[test]
    fn compile_status_label_keeps_header_width_stable() {
        assert_eq!(compile_status_label("ACTIVE"), "ACTIVE  ");
        assert_eq!(compile_status_label("LOADING"), "LOADING ");
        assert_eq!(compile_status_label("COMPILED"), "COMPILED");
    }

    #[test]
    fn timeline_detail_prefixes_keep_stable_width_across_focus() {
        let active = timeline_detail_prefix(true, false, 0, 0);
        let inactive = timeline_detail_prefix(false, false, 0, 0);
        let active_ai_group = timeline_detail_prefix(true, true, 1, 0);
        let inactive_ai_group = timeline_detail_prefix(false, true, 1, 0);

        assert_eq!(active.chars().count(), 5);
        assert_eq!(inactive.chars().count(), 5);
        assert_eq!(active_ai_group.chars().count(), 5);
        assert_eq!(inactive_ai_group.chars().count(), 5);
    }

    #[test]
    fn timeline_detail_body_column_does_not_shift_when_selected() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.data.timeline = vec![
            TimelineEvent {
                id: "evt-001".into(),
                time: "10:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "active aligned body".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-002".into(),
                time: "10:01".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "inactive aligned body".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 0;
        app.rewind_event_id = "evt-001".into();

        let screen = render_text(&app, 100, 18);
        let active_column = screen_column(&screen, "active aligned body");
        let inactive_column = screen_column(&screen, "inactive aligned body");

        assert_eq!(active_column, inactive_column, "{screen}");
    }

    #[test]
    fn selected_timeline_body_keeps_stable_font_weight() {
        let active = timeline_detail_style(true, false, TimelineKind::User);
        let inactive_rewind = timeline_detail_style(false, true, TimelineKind::User);
        let inactive = timeline_detail_style(false, false, TimelineKind::User);

        assert!(!active.add_modifier.contains(Modifier::BOLD));
        assert_eq!(active.fg, Some(theme::text()));
        assert!(!inactive_rewind.add_modifier.contains(Modifier::BOLD));
        assert_eq!(inactive_rewind.fg, Some(theme::text()));
        assert_eq!(inactive.fg, Some(theme::muted()));
    }

    #[test]
    fn neutral_status_line_is_auxiliary_not_selected() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let line = status_line(&app);
        let message = &line.spans[1];

        assert_eq!(message.style.fg, Some(theme::muted()));
        assert!(!message.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn hooks_disabled_does_not_render_live_queue_noise() {
        let app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 150, 36);

        assert!(!screen.contains("WAITING ON YOU"), "{screen}");
        assert!(!screen.contains("Live on"), "{screen}");
    }

    #[test]
    fn hooks_enabled_waiting_state_renders_queue_badge_and_status() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        let session = app
            .data
            .sessions
            .get(app.selected_session)
            .expect("session");
        let session_cli = session.cli;
        let session_id = session.id.clone();
        app.set_hook_live_events_for_test(vec![test_hook_event(
            session_cli,
            &session_id,
            hooks::HookEventKind::PermissionRequest,
            "Approval: Edit src/app.rs",
            Some("Approval: Edit src/app.rs"),
            hooks::current_millis(),
        )]);

        let screen = render_text(&app, 150, 36);

        assert_screen_contains(&screen, "WAITING ON YOU");
        assert_screen_contains(&screen, "WAIT");
        assert_screen_contains(&screen, "Approval: Edit");
        assert_screen_contains(&screen, "Live on");
    }

    #[test]
    fn hooks_enabled_marks_live_unavailable_for_ssh_data_space() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        let session = app
            .data
            .sessions
            .get(app.selected_session)
            .expect("session");
        let session_cli = session.cli;
        let session_id = session.id.clone();
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "devbox.example".into(),
                ssh_host: Some("devbox.example".into()),
                ssh_user: None,
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: Some("~/.config/moonbox/config.json".into()),
            },
        ];
        app.selected_data_space = 1;
        app.set_hook_live_events_for_test(vec![test_hook_event(
            session_cli,
            &session_id,
            hooks::HookEventKind::PermissionRequest,
            "Approval: Edit src/app.rs",
            Some("Approval: Edit src/app.rs"),
            hooks::current_millis(),
        )]);

        let screen = render_text(&app, 150, 36);

        assert_screen_contains(&screen, "Live unavailable: SSH data");
        assert!(!screen.contains("WAITING ON YOU"), "{screen}");
    }

    #[test]
    fn settings_overlay_previews_smart_enter_effect_before_save() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_hooks_config_for_test(crate::core::config::HooksConfig {
            enabled: true,
            smart_enter_tmux: false,
            ..crate::core::config::HooksConfig::default()
        });
        let session = app
            .data
            .sessions
            .get(app.selected_session)
            .expect("session");
        let session_cli = session.cli;
        let session_id = session.id.clone();
        let mut event = test_hook_event(
            session_cli,
            &session_id,
            hooks::HookEventKind::PreToolUse,
            "Edit src/app.rs",
            None,
            hooks::current_millis(),
        );
        event.tmux = Some("/tmp/tmux-501/default,1,0".into());
        app.set_hook_live_events_for_test(vec![event]);

        app.handle_key(key(','));
        let off_screen = render_text(&app, 150, 36);
        assert_screen_contains(&off_screen, "Settings");
        assert_screen_contains(&off_screen, "Language");
        assert_screen_contains(&off_screen, "Theme");
        assert_screen_contains(&off_screen, "Draft Off");
        assert_screen_contains(&off_screen, "Resume");

        app.handle_key(key('j'));
        app.handle_key(key('j'));
        app.handle_key(key(' '));
        let on_screen = render_text(&app, 150, 36);
        assert_screen_contains(&on_screen, "Draft On");
        assert_screen_contains(&on_screen, "Unsaved");
        assert_screen_contains(&on_screen, "Jump");
        assert_screen_contains(&on_screen, "never translates session content");
    }

    #[test]
    fn settings_overlay_previews_language_and_theme_without_touching_session_text() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.data.timeline = vec![TimelineEvent {
            id: "evt-zh".into(),
            time: "10:00".into(),
            kind: TimelineKind::User,
            title: "User".into(),
            detail: "看下这个问题".into(),
            metadata: Default::default(),
        }];

        app.handle_key(key(','));
        app.handle_key(key(' '));
        let zh_screen = render_text(&app, 150, 36);
        assert_screen_contains(&zh_screen, "设置");
        assert_screen_contains(&zh_screen, "语言");
        assert_screen_contains(&zh_screen, "简体中文");

        app.handle_key(key('j'));
        app.handle_key(key(' '));
        let theme_screen = render_text(&app, 150, 36);
        assert_screen_contains(&theme_screen, "Tokyo Night");
        assert_screen_contains(&theme_screen, "看下这个问题");
    }

    #[test]
    fn zh_hans_preferences_localize_main_chrome_without_touching_session_content() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });

        let screen = render_text(&app, 150, 42);

        assert_screen_contains(&screen, "筛选");
        assert_screen_contains(&screen, "数据:");
        assert_screen_contains(&screen, "技能:");
        assert_screen_contains(&screen, "本地");
        assert_screen_contains(&screen, "会话 · 全部");
        assert_screen_contains(&screen, "时间线");
        assert_screen_contains(&screen, "会话详情");
        assert_screen_contains(&screen, "真实会话元数据");
        assert_screen_contains(&screen, "原始标题");
        assert_screen_contains(&screen, "保真度");
        assert_screen_contains(&screen, "操作路径");
        assert_screen_contains(&screen, "状态");
        assert_screen_contains(&screen, "Moonbox session rewind design");
    }

    #[test]
    fn settings_overlay_renders_all_theme_names() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.handle_key(key(','));

        let moonbox = render_text(&app, 150, 36);
        assert_screen_contains(&moonbox, "Moonbox");

        app.handle_key(key('j'));
        app.handle_key(key(' '));
        let tokyo = render_text(&app, 150, 36);
        assert_screen_contains(&tokyo, "Tokyo Night");

        app.handle_key(key(' '));
        let gruvbox = render_text(&app, 150, 36);
        assert_screen_contains(&gruvbox, "Gruvbox");
    }

    #[test]
    fn session_row_marks_enter_jump_when_smart_enter_is_active() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_hooks_config_for_test(crate::core::config::HooksConfig {
            enabled: true,
            smart_enter_tmux: true,
            ..crate::core::config::HooksConfig::default()
        });
        let session = app
            .data
            .sessions
            .get(app.selected_session)
            .expect("session");
        let session_cli = session.cli;
        let session_id = session.id.clone();
        let mut event = test_hook_event(
            session_cli,
            &session_id,
            hooks::HookEventKind::PreToolUse,
            "Edit src/app.rs",
            None,
            hooks::current_millis(),
        );
        event.tmux = Some("/tmp/tmux-501/default,1,0".into());
        app.set_hook_live_events_for_test(vec![event]);

        let screen = render_text(&app, 150, 36);

        assert_screen_contains(&screen, "Enter");
        assert_screen_contains(&screen, "Jump");
    }

    #[test]
    fn unselected_session_titles_are_muted() {
        assert_eq!(session_title_style(false).fg, Some(theme::muted()));
        assert_eq!(session_title_style(true).fg, Some(theme::text()));
        assert!(
            session_title_style(true)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn header_tokens_do_not_show_fake_budget() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 64);

        assert!(!screen.contains("/ 100K"), "{screen}");
    }

    #[test]
    fn header_shows_current_data_space() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 160, 40);

        assert_screen_contains(&screen, "Data:");
        assert_screen_contains(&screen, "Local");
    }

    #[test]
    fn header_marks_ssh_data_space_explicitly() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "yangyang.1205@10.37.218.31".into(),
                ssh_host: Some("10.37.218.31".into()),
                ssh_user: Some("yangyang.1205".into()),
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: Some("~/.config/moonbox/config.json".into()),
            },
        ];
        app.selected_data_space = 1;

        let screen = render_text(&app, 160, 40);

        assert_screen_contains(&screen, "Data:");
        assert_screen_contains(&screen, "SSH: devbox");
    }

    #[test]
    fn data_space_picker_shows_visual_config_and_switch_hint() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "yangyang.1205@10.37.218.31".into(),
                ssh_host: Some("10.37.218.31".into()),
                ssh_user: Some("yangyang.1205".into()),
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: Some("~/.config/moonbox/config.json".into()),
            },
        ];
        app.show_data_spaces = true;
        app.data_space_selection = 1;

        let screen = render_text(&app, 160, 44);

        assert_screen_contains(&screen, "Data Space Picker");
        assert_screen_contains(&screen, "SSH read-only inventory");
        assert_screen_contains(&screen, "Moonbox config");
        assert_screen_contains(
            &screen,
            "ssh yangyang.1205@10.37.218.31 [moonbox|moon] sessions --json",
        );
        assert_screen_contains(&screen, "no remote resume or launch");
        assert_screen_contains(&screen, "Enter load");
    }

    #[test]
    fn data_space_picker_renders_load_failure_prominently() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_data_spaces = true;
        app.data_space_error = Some(
            "cannot load data space devbox: ssh inventory exited with exit status: 127".into(),
        );

        let screen = render_text(&app, 160, 44);

        assert_screen_contains(&screen, "Load Failed");
        assert_screen_contains(&screen, "Install moonbox");
        let line = status_line(&app);
        assert_eq!(line.spans[1].style.fg, Some(theme::red()));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn data_space_config_overlay_renders_required_fields() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_data_spaces = true;
        app.show_data_space_config = true;
        app.data_space_config_form.name = "devbox".into();
        app.data_space_config_form.host = "10.37.218.31".into();

        let screen = render_text(&app, 160, 44);

        assert_screen_contains(&screen, "Add SSH Data Space");
        assert_screen_contains(&screen, "Add SSH Space");
        assert_screen_contains(&screen, "devbox");
        assert_screen_contains(&screen, "10.37.218.31");
        assert_screen_contains(&screen, "Ctrl-S save");
    }

    #[test]
    fn header_brand_degrades_on_narrow_width() {
        let narrow = header_title_spans(80, crate::core::config::UiLanguage::English)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();
        let wide = header_title_spans(140, crate::core::config::UiLanguage::English)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();
        let zh_wide = header_title_spans(140, crate::core::config::UiLanguage::ZhHans)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();

        assert_eq!(narrow, " MOONBOX ");
        assert_eq!(wide, " MOONBOX ");
        assert_eq!(zh_wide, " MOONBOX 月光宝盒");
    }

    #[test]
    fn header_collapses_preflight_signals() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.doctor_report.status = VerificationStatus::Warn;
        app.doctor_report.ready = true;
        app.compile_status = "ACTIVE";
        app.verify_passed = true;
        let screen = render_text(&app, 160, 40);

        assert_screen_contains(&screen, "Pre-flight:");
        assert_screen_contains(&screen, "WARN");
        assert_screen_contains(&screen, "Medium");
        assert!(!screen.contains("Compiler:"), "{screen}");
        assert!(!screen.contains("Doctor:"), "{screen}");
        assert!(!screen.contains("Verify:"), "{screen}");
    }

    #[test]
    fn confidence_uses_stable_semantic_colors() {
        assert_eq!(
            PreflightConfidence::Strong.color(),
            theme::confidence_strong()
        );
        assert_eq!(
            PreflightConfidence::Medium.color(),
            theme::confidence_medium()
        );
        assert_eq!(PreflightConfidence::Weak.color(), theme::confidence_weak());
    }

    #[test]
    fn session_list_secondary_uses_relative_time_with_branch() {
        let now = parse_session_timestamp("2026-06-07T13:34:00+08:00").expect("now");
        let session = test_session("2026-06-07T13:33:44+08:00", Some("dev"));

        assert_eq!(
            session_list_secondary_at(&session, now),
            "    size unknown  ·  16s ago  ·  dev"
        );
    }

    #[test]
    fn session_inventory_metric_uses_user_readable_size_terms() {
        let mut session = test_session("2026-06-07T13:33:44+08:00", Some("dev"));
        session.event_count = 24;

        assert_eq!(session_inventory_metric(&session), "timeline indexed");

        session.token_count = Some(42_000);
        assert_eq!(session_inventory_metric(&session), "42K tokens");

        session.source_size_bytes = Some(1_572_864);
        assert_eq!(
            session_inventory_metric(&session),
            "42K tokens · 1.5MB source"
        );
    }

    #[test]
    fn session_row_markers_keep_star_visible_with_health_status() {
        let mut session = test_session("2026-06-07T13:33:44+08:00", None);

        session.status = SessionStatus::Failed;
        let failed_markers: Vec<String> = session_row_markers(&session, true)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect();
        assert_eq!(failed_markers, ["*", "!"]);

        session.status = SessionStatus::Warning;
        let warning_markers: Vec<String> = session_row_markers(&session, true)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect();
        assert_eq!(warning_markers, ["*", "▲"]);

        session.status = SessionStatus::Healthy;
        assert!(session_row_markers(&session, false).is_empty());
    }

    #[test]
    fn session_list_renders_star_and_failed_marker_together() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let star_key = {
            let session = app
                .data
                .sessions
                .get_mut(app.selected_session)
                .expect("session");
            session.status = SessionStatus::Failed;
            format!("{}:{}", session.cli.id(), session.id)
        };
        app.starred_sessions = vec![star_key];

        let screen = render_text(&app, 140, 64);

        assert_screen_contains(&screen, "* !");
    }

    #[test]
    fn selected_session_portrait_uses_readable_cached_timeline_roles() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let session = app.current_session().expect("session");

        assert_eq!(
            session_portrait_detail(&app, session),
            "user 1 / assistant 1 / tool 4 / rewind 1 · cached timeline"
        );
    }

    #[test]
    fn session_list_renders_readable_activity_metric() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 64);

        assert_screen_contains(&screen, "42K tokens");
        assert_screen_contains(&screen, "Timeline Items");
        assert_screen_contains(&screen, "Portrait");
        assert_screen_contains(&screen, "user 1 / assistant 1 / tool");
        assert_screen_contains(&screen, "4 / rewind 1");
        assert!(!screen.contains("shape U"), "{screen}");
    }

    #[test]
    fn zoomed_session_details_surface_value_ranked_anatomy() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Capsule;
        app.zoomed_focus = Some(Focus::Capsule);
        app.data.sessions[app.selected_session].anatomy =
            Some(crate::core::model::SessionAnatomy {
                status: SessionAnatomyStatus::Ready,
                scan_scope: "full".into(),
                source_size_bytes: Some(2_048),
                analyzed_bytes: 2_048,
                sampled: false,
                total_lines: Some(5),
                malformed_lines: 0,
                value_signals: vec![crate::core::model::AnatomySignal {
                    rank: 1,
                    group: "Continuation".into(),
                    label: "Active tail".into(),
                    value: "512B / 2 rows".into(),
                    detail: "newest content after compact".into(),
                }],
                size_profile: vec![AnatomyMetric {
                    label: "compacted".into(),
                    count: 1,
                    bytes: 1_024,
                }],
                event_profile: vec![AnatomyMetric {
                    label: "token_count".into(),
                    count: 1,
                    bytes: 256,
                }],
                content_profile: vec![AnatomyMetric {
                    label: "content:image".into(),
                    count: 1,
                    bytes: 128,
                }],
                compact: Some(crate::core::model::CompactFrontier {
                    label: "context_compacted".into(),
                    line_number: Some(3),
                    tail_lines: 2,
                    tail_bytes: 512,
                    detail: "active tail".into(),
                }),
                token_profile: None,
                sidecars: vec![crate::core::model::SessionSidecarSummary {
                    kind: "subagents".into(),
                    path: "/tmp/session".into(),
                    file_count: 2,
                    bytes: 512,
                }],
                notes: vec!["fixture note".into()],
            });

        let screen = render_text(&app, 140, 64);

        assert_screen_contains(&screen, "Session Anatomy");
        assert_screen_contains(&screen, "Value Signals");
        assert_screen_contains(&screen, "Compact Frontier");
        assert_screen_contains(&screen, "Size Profile");
        assert_screen_contains(&screen, "compacted");
        assert_screen_contains(&screen, "Content Profile");
        assert_screen_contains(&screen, "content:image");
        assert_screen_contains(&screen, "Sidecars");
        assert_screen_contains(&screen, "subagents");
    }

    #[test]
    fn zoomed_session_details_surface_remote_anatomy_fallback_reason() {
        let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Capsule;
        app.zoomed_focus = Some(Focus::Capsule);
        app.data.sessions[app.selected_session].anatomy =
            Some(crate::core::model::SessionAnatomy {
                status: SessionAnatomyStatus::Missing,
                scan_scope: "remote-unavailable".into(),
                notes: vec![
                    "Remote moonbox on devbox did not return session anatomy; upgrade the remote moonbox binary to M92 or newer."
                        .into(),
                ],
                ..crate::core::model::SessionAnatomy::default()
            });

        let screen = render_text(&app, 140, 56);

        assert_screen_contains(&screen, "Session Anatomy");
        assert_screen_contains(&screen, "remote-unavailable");
        assert_screen_contains(&screen, "upgrade the remote moonbox binary");
        assert!(!screen.contains("Source path is not readable"), "{screen}");
    }

    #[test]
    fn source_badges_share_one_color_mapping() {
        assert_eq!(source_tool_color(CliTool::Codex), theme::blue());
        assert_eq!(source_tool_color(CliTool::Claude), theme::purple());
        assert_eq!(source_tool_color(CliTool::Hermes), theme::orange());
        assert_eq!(
            source_tool_style(CliTool::Claude).fg,
            Some(source_tool_color(CliTool::Claude))
        );
    }

    #[test]
    fn relative_time_label_matches_resume_picker_style() {
        let now = parse_session_timestamp("2026-06-07T13:34:00Z").expect("now");

        assert_eq!(
            relative_time_label("2026-06-07T13:30:00Z", now).as_deref(),
            Some("4m ago")
        );
        assert_eq!(
            relative_time_label("2026-06-07T04:34:00Z", now).as_deref(),
            Some("9h ago")
        );
        assert_eq!(
            relative_time_label("2026-06-05T13:34:00Z", now).as_deref(),
            Some("2d ago")
        );
    }

    fn test_session(updated_at: &str, branch: Option<&str>) -> crate::core::model::SessionSummary {
        crate::core::model::SessionSummary {
            id: "session-id".into(),
            cli: CliTool::Codex,
            title: "Session".into(),
            cwd: "/repo".into(),
            updated_at: updated_at.into(),
            updated: "2026-06-07 13:33".into(),
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: Some("test adapter does not expose live runtime activity".into()),
            status: SessionStatus::Healthy,
            branch: branch.map(str::to_owned),
            token_count: None,
            health_reason: None,
            event_count: 0,
            resume_command: "codex resume session-id".into(),
            source_provenance: SourceProvenance::Real,
            source_path: None,
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: None,
            anatomy: None,
        }
    }

    fn test_hook_event(
        cli: CliTool,
        session_id: &str,
        kind: hooks::HookEventKind,
        summary: &str,
        wait_reason: Option<&str>,
        captured_at_ms: u128,
    ) -> hooks::HookSpoolEvent {
        hooks::HookSpoolEvent {
            cli,
            session_id: session_id.into(),
            transcript_path: None,
            cwd: Some("/repo".into()),
            tmux: None,
            tmux_pane: Some("%42".into()),
            captured_at_ms,
            event_name: format!("{kind:?}"),
            kind,
            summary: summary.into(),
            wait_reason: wait_reason.map(str::to_owned),
        }
    }

    #[test]
    fn main_timeline_hides_low_signal_tool_events() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "REWIND");
        assert!(!screen.contains("Tool: rg"), "{screen}");
    }

    #[test]
    fn timeline_scroll_accounts_for_wrapped_event_details() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.data.timeline = vec![
            TimelineEvent {
                id: "evt-001".into(),
                time: "10:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "very long context ".repeat(40),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-002".into(),
                time: "10:01".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "selected user question".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 1;
        app.rewind_event_id = "evt-002".into();

        let scroll = timeline_scroll(&app, Rect::new(0, 0, 48, 8));

        assert!(scroll > 6, "scroll should include wrapped detail height");
    }

    #[test]
    fn timeline_renders_image_attachments_without_raw_markup() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.data.timeline = vec![TimelineEvent {
            id: "evt-001".into(),
            time: "09:08".into(),
            kind: TimelineKind::User,
            title: "User".into(),
            detail: "看下这个问题".into(),
            metadata: crate::core::model::TimelineEventMetadata {
                attachments: vec![TimelineAttachment {
                    name: Some("Image #1".into()),
                    mime_type: Some("image/unknown".into()),
                    ..TimelineAttachment::default()
                }],
                ..Default::default()
            },
        }];
        app.selected_event = 0;
        app.rewind_event_id = "evt-001".into();

        let screen = render_text(&app, 100, 18);

        assert_screen_contains(&screen, "[image] Image #1");
        assert_screen_contains(&screen, "看下这个问题");
        let image_pos = screen.find("[image] Image #1").expect("image row");
        let detail_pos = screen.find("看下这个问题").expect("detail row");
        assert!(image_pos < detail_pos, "{screen}");
        assert!(!screen.contains("<image"), "{screen}");
    }

    #[test]
    fn timeline_focus_keybar_exposes_event_detail_key() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;

        let hints = active_key_hints(&app);

        assert!(hints.contains(&("e", "Detail")));
        assert!(!hints.contains(&("enter", "Detail")));
    }

    #[test]
    fn timeline_detail_overlay_renders_selected_event_body_and_attachments() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.show_timeline_detail = true;
        app.data.timeline = vec![TimelineEvent {
            id: "evt-777".into(),
            time: "12:18".into(),
            kind: TimelineKind::User,
            title: "User".into(),
            detail: "第一行\n第二行完整内容".into(),
            metadata: crate::core::model::TimelineEventMetadata {
                attachments: vec![TimelineAttachment {
                    name: Some("Image #1".into()),
                    mime_type: Some("image/unknown".into()),
                    ..TimelineAttachment::default()
                }],
                ..Default::default()
            },
        }];
        app.selected_event = 0;
        app.rewind_event_id = "evt-777".into();

        let screen = render_text(&app, 120, 32);

        assert_screen_contains(&screen, "Timeline Detail");
        assert_screen_contains(&screen, "evt-777");
        assert_screen_contains(&screen, "USER");
        assert_screen_contains(&screen, "Attachments");
        assert_screen_contains(&screen, "[image] Image #1");
        assert_screen_contains(&screen, "第一行");
        assert_screen_contains(&screen, "第二行完整内容");
        assert_screen_contains(&screen, "Esc");
        assert_screen_contains(&screen, "close");
    }

    #[test]
    fn timeline_detail_overlay_renders_image_preview_pixels_when_cached() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.show_timeline_detail = true;
        app.data.timeline = vec![TimelineEvent {
            id: "evt-img".into(),
            time: "12:18".into(),
            kind: TimelineKind::User,
            title: "User".into(),
            detail: "看下这个问题".into(),
            metadata: crate::core::model::TimelineEventMetadata {
                attachments: vec![TimelineAttachment {
                    name: Some("Image #1".into()),
                    path: Some("/tmp/moonbox-image-preview.png".into()),
                    mime_type: Some("image/png".into()),
                    ..TimelineAttachment::default()
                }],
                ..Default::default()
            },
        }];
        app.timeline_image_previews = vec![TimelineImagePreview {
            event_id: "evt-img".into(),
            label: "Image #1".into(),
            path: Some("/tmp/moonbox-image-preview.png".into()),
            dimensions: Some((4, 2)),
            status: ImagePreviewStatus::Rendered,
            rows: vec![vec![
                PreviewCell {
                    top: PreviewRgb {
                        red: 255,
                        green: 0,
                        blue: 0,
                    },
                    bottom: Some(PreviewRgb {
                        red: 0,
                        green: 0,
                        blue: 255,
                    }),
                },
                PreviewCell {
                    top: PreviewRgb {
                        red: 0,
                        green: 255,
                        blue: 0,
                    },
                    bottom: None,
                },
            ]],
        }];
        app.selected_event = 0;
        app.rewind_event_id = "evt-img".into();

        let screen = render_text(&app, 120, 32);

        assert_screen_contains(&screen, "Image Preview");
        assert_screen_contains(&screen, "rendered");
        assert_screen_contains(&screen, "4x2");
        assert_screen_contains(&screen, "▀▀");
    }

    #[test]
    fn timeline_detail_overlay_expands_selected_assistant_group() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.show_timeline_detail = true;
        app.data.timeline = vec![
            TimelineEvent {
                id: "evt-001".into(),
                time: "12:58".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "先看仓库状态".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-119".into(),
                time: "12:59".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "我会从当前仓库和 GitHub 状态重新开始。".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-120".into(),
                time: "13:00".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "复核结果：当前本地在 chore/restart-governance。".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-121".into(),
                time: "13:01".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "新的 CI 全绿，我会合入治理 PR。".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 1;
        app.rewind_event_id = "evt-001".into();

        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "evt-119..evt-121");
        assert_screen_contains(&screen, "Codex x3 group");
        assert_screen_contains(&screen, "Events:");
        assert_screen_contains(&screen, "1/3");
        assert_screen_contains(&screen, "2/3");
        assert_screen_contains(&screen, "3/3");
        assert!(!screen.contains("Title: Assistant"), "{screen}");
        assert!(!screen.contains("Body"), "{screen}");
        assert_screen_contains(&screen, "我会从当前仓库和 GitHub 状态重新开始。");
        assert_screen_contains(&screen, "复核结果：当前本地在 chore/restart-governance。");
        assert_screen_contains(&screen, "新的 CI 全绿，我会合入治理 PR。");
    }

    #[test]
    fn timeline_visually_groups_consecutive_assistant_events() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.data.timeline = vec![
            TimelineEvent {
                id: "evt-001".into(),
                time: "10:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "分析下 cxcp".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-002".into(),
                time: "10:01".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "先定位项目。".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-003".into(),
                time: "10:02".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "继续分析缓存。".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-004".into(),
                time: "10:03".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "下一步".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 1;
        app.rewind_event_id = "evt-001".into();

        let screen = render_text(&app, 120, 28);

        assert_screen_contains(&screen, "Codex x2");
        assert_screen_contains(&screen, "先定位项目");
        assert_screen_contains(&screen, "继续分析缓存");
        assert_eq!(screen.matches("Codex x2").count(), 1, "{screen}");
        assert!(!screen.contains("ASSISTANT  Assistant"), "{screen}");
    }

    #[test]
    fn timeline_assistant_group_label_uses_source_cli() {
        let mut app = App::new(CliTool::Claude, CliTool::Codex).expect("app");
        app.focus = Focus::Timeline;
        app.data.timeline = vec![
            TimelineEvent {
                id: "evt-001".into(),
                time: "10:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "start".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-002".into(),
                time: "10:01".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "first".into(),
                metadata: Default::default(),
            },
            TimelineEvent {
                id: "evt-003".into(),
                time: "10:02".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "second".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 1;
        app.rewind_event_id = "evt-001".into();

        let screen = render_text(&app, 120, 28);

        assert_screen_contains(&screen, "Claude Code x2");
        assert!(!screen.contains("AI x2"), "{screen}");
    }

    #[test]
    fn skill_picker_renders_compiler_metadata() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_skill_picker = true;
        app.data.compilers = vec!["agent:codex:handoff".into(), "engineering-handoff".into()];
        app.pending_compiler = 0;
        app.selected_compiler = 0;

        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Skill Picker");
        assert_screen_contains(&screen, "Choose handoff skill");
        assert_screen_contains(
            &screen,
            "Pick the handoff skill; runner setup is checked before launch.",
        );
        assert_screen_contains(&screen, "handoff");
        assert_screen_contains(&screen, "Skill");
        assert_screen_contains(&screen, "Status:");
        assert_screen_contains(&screen, "engineering-handoff");
        assert_screen_contains(&screen, "draft template");
        assert_screen_contains(&screen, "Built-in fallback draft");
        assert_screen_contains(&screen, "j/k Choose");
        assert!(!screen.contains("agent:codex:handoff"), "{screen}");
        assert!(!screen.contains("Runner:"), "{screen}");
        assert!(!screen.contains("Command:"), "{screen}");
        assert!(!screen.contains("pip install"), "{screen}");
        assert!(!screen.contains("stars:"), "{screen}");
        assert!(!screen.contains("n/a"), "{screen}");
    }

    #[test]
    fn installed_agent_skill_details_highlight_provider_and_link() {
        let info = crate::core::model::CompilerPresetInfo {
            id: "agent:codex:handoff".into(),
            kind: crate::core::model::CompilerPresetKind::Agent,
            status: crate::core::model::CompilerPresetStatus::Ready,
            score: 95,
            command: Some("python3".into()),
            args: vec!["skill=/Users/example/.codex/skills/handoff/SKILL.md".into()],
            timeout_ms: None,
            reason: "Codex SDK runner is installed and auth preflight passed".into(),
            description: Some(
                "Codex runner using community handoff skill: Compact the current conversation into a handoff document for another agent to pick up.".into(),
            ),
            homepage: Some(
                "https://github.com/mattpocock/skills/tree/main/skills/productivity/handoff"
                    .into(),
            ),
            github_stars: None,
        };

        let text = agent_skill_detail_lines(&info, crate::core::config::UiLanguage::ZhHans)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("提供方: mattpocock/skills（三方）"), "{text}");
        assert!(
            text.contains("链接: https://github.com/mattpocock/skills"),
            "{text}"
        );
        assert!(text.contains("状态: 本机已安装"), "{text}");
        assert!(
            text.contains("路径: /Users/example/.codex/skills/handoff/SKILL.md"),
            "{text}"
        );
    }

    #[test]
    fn skill_picker_over_launch_shows_apply_and_generate_hint() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        app.show_launch = true;
        app.show_skill_picker = true;
        app.data.compilers = vec!["agent:codex:handoff".into()];
        app.pending_compiler = 0;
        app.selected_compiler = 0;

        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "enter 应用并生成");
        assert_screen_contains(&screen, "y 复制 Skill 引用");
    }

    #[test]
    fn skill_picker_uses_simplified_chinese_chrome_when_configured() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        app.show_skill_picker = true;
        app.data.compilers = vec!["agent:codex:handoff".into(), "engineering-handoff".into()];
        app.pending_compiler = 0;
        app.selected_compiler = 0;

        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Skill 选择器");
        assert_screen_contains(&screen, "选择 Handoff Skill");
        assert_screen_contains(
            &screen,
            "这里只选择 handoff skill；执行器配置会在启动前预检。",
        );
        assert_screen_contains(&screen, "handoff");
        assert_screen_contains(&screen, "Skill");
        assert_screen_contains(&screen, "状态:");
        assert_screen_contains(&screen, "engineering-handoff");
        assert_screen_contains(&screen, "警告");
        assert_screen_contains(&screen, "草稿模板");
        assert_screen_contains(&screen, "用途:");
        assert_screen_contains(&screen, "内置 fallback 草稿");
        assert_screen_contains(&screen, "已启用");
        assert_screen_contains(&screen, "j/k 选择");
        assert!(!screen.contains("Choose compiler skill"), "{screen}");
        assert!(!screen.contains("agent:codex:handoff"), "{screen}");
        assert!(!screen.contains("执行器:"), "{screen}");
        assert!(!screen.contains("命令:"), "{screen}");
        assert!(!screen.contains("pip install"), "{screen}");
        assert!(!screen.contains("stars:"), "{screen}");
        assert!(!screen.contains("热度:"), "{screen}");
        assert!(!screen.contains("不适用"), "{screen}");
    }

    #[test]
    fn capsule_inventory_overlay_renders_saved_capsules() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_capsules = true;
        app.saved_capsules = vec![crate::core::capsule_store::CapsuleSummary {
            version: 1,
            name: "demo".into(),
            created_at: "unix:1780732800".into(),
            updated_at: "unix:1780736400".into(),
            checksum: "fnv64:0123456789abcdef".into(),
            size_bytes: 4096,
            source_cli: CliTool::Codex,
            target_cli: CliTool::Hermes,
            source_session: "codex-cxcp-design".into(),
            rewind_point: "evt-091 / Continue".into(),
            compiler: "engineering-handoff".into(),
            handoff_label: "moonbox/hermes-rewind-evt-091".into(),
        }];
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Capsule Inventory");
        assert_screen_contains(&screen, "Saved Capsules");
        assert_screen_contains(&screen, "Local continuation objects");
        assert_screen_contains(&screen, "demo");
        assert_screen_contains(&screen, "codex-cxcp-design");
        assert_screen_contains(&screen, "fnv64:0123456789abcdef");
        assert_screen_contains(&screen, "r refresh");
    }

    #[test]
    fn zoomed_timeline_uses_full_body_without_side_panels() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.focus = Focus::Timeline;
        app.zoomed_focus = Some(Focus::Timeline);

        let screen = render_text(&app, 140, 36);

        assert_screen_contains(&screen, "Timeline");
        assert!(!screen.contains("Sessions ·"), "{screen}");
        assert!(!screen.contains("Session Details"), "{screen}");
        assert_screen_contains(&screen, "Action Path");
    }

    #[test]
    fn action_path_renders_explicit_handoff_arrow() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "source Codex codex-cxcp-des...");
        assert_screen_contains(&screen, "-> rewind evt-091 -> target Hermes");
    }

    #[test]
    fn action_path_uses_source_rewind_target_semantic_colors() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let line = handoff_path_line(&app, 140);
        let source = line
            .spans
            .iter()
            .find(|span| span.content.contains("source Codex"))
            .expect("source span");
        let rewind = line
            .spans
            .iter()
            .find(|span| span.content.contains("rewind evt-091"))
            .expect("rewind span");
        let target = line
            .spans
            .iter()
            .find(|span| span.content.contains("target Hermes"))
            .expect("target span");

        assert_eq!(source.style.fg, Some(source_tool_color(CliTool::Codex)));
        assert_eq!(rewind.style.fg, Some(theme::role_rewind()));
        assert_eq!(target.style.fg, Some(theme::role_target()));
    }

    #[test]
    fn action_path_renders_short_handoff_trail() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.start_handoff_trail_for_review();
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "handoff trail");
        assert_screen_contains(&screen, "source");
        assert_screen_contains(&screen, "rewind");
        assert_screen_contains(&screen, "target");
        assert_screen_contains(&screen, "Review");
    }

    #[test]
    fn selected_timeline_rows_keep_role_accent_colors() {
        assert_eq!(timeline_group_accent(theme::blue(), false), theme::blue());
        assert_eq!(timeline_group_accent(theme::gold(), false), theme::gold());
        assert_eq!(
            timeline_group_accent(theme::blue(), true),
            theme::role_rewind()
        );

        let selected_user_prefix = timeline_prefix_style(true, theme::blue());
        assert_eq!(selected_user_prefix.fg, Some(theme::blue()));
        assert!(selected_user_prefix.add_modifier.contains(Modifier::BOLD));

        let selected_ai_prefix = timeline_prefix_style(true, theme::gold());
        assert_eq!(selected_ai_prefix.fg, Some(theme::gold()));
        assert!(selected_ai_prefix.add_modifier.contains(Modifier::BOLD));

        let active_cursor_marker = timeline_marker_style(true, true, false);
        assert_eq!(active_cursor_marker.fg, Some(theme::cyan()));
        assert!(active_cursor_marker.add_modifier.contains(Modifier::BOLD));

        let inactive_rewind_marker = timeline_marker_style(false, false, true);
        assert_eq!(inactive_rewind_marker.fg, Some(theme::role_rewind()));
        assert!(inactive_rewind_marker.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn right_panel_shows_compact_handoff_snapshot_not_full_capsule() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Real Session Metadata");
        assert_screen_contains(&screen, "Fidelity");
        assert_screen_contains(&screen, "fallback · embedded_fixture");
        assert_screen_contains(&screen, "Handoff Snapshot");
        assert_screen_contains(&screen, "draft_from_builtin_compiler");
        assert_screen_contains(&screen, "Risk: Built-in draft compiler");
        assert!(
            !screen.contains("Production handoff should use"),
            "{screen}"
        );
        assert!(!screen.contains("Define canonical timeline"), "{screen}");
    }

    #[test]
    fn doctor_overlay_renders_diagnostics_and_actions() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_doctor = true;
        app.doctor_report.status = VerificationStatus::Pass;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Pre-flight");
        assert_screen_contains(&screen, "Compiler:");
        assert_screen_contains(&screen, "Doctor:");
        assert_screen_contains(&screen, "Verify:");
        assert_screen_contains(&screen, "Verifier evidence");
        assert_screen_contains(&screen, "Environment doctor");
        assert_screen_contains(&screen, "source_codex_adapter");
        assert_screen_contains(&screen, "fidelity=fallback");
        assert_screen_contains(&screen, "fixtures/adapters/codex");
        assert_screen_contains(&screen, "Copy JSON");
    }

    #[test]
    fn command_palette_renders_floating_completion_details() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.command_mode = true;
        app.command_input = "cap".into();
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Command Palette");
        assert_eq!(screen.matches("Command Palette").count(), 1, "{screen}");
        assert_screen_contains(&screen, ": cap");
        assert_screen_contains(&screen, "capsule");
        assert_screen_contains(&screen, "Refresh the Capsule");
        assert_screen_contains(&screen, "REVIEW");
        assert_screen_contains(&screen, "Params:");
        assert_screen_contains(&screen, "selected rewind");
        assert_screen_contains(&screen, "Risk:");
        assert_screen_contains(&screen, "no execute path");
    }

    #[test]
    fn command_palette_renders_empty_state() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.command_mode = true;
        app.command_input = "zzzz".into();
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Command Palette");
        assert_screen_contains(&screen, "No commands match");
        assert_screen_contains(&screen, "Try open, capsule, handoff");
    }

    #[test]
    fn command_palette_marks_exit_as_dangerous() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.command_mode = true;
        app.command_input = "quit".into();
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "quit");
        assert_screen_contains(&screen, "EXIT");
        assert_screen_contains(&screen, "exits Moonbox");
    }

    #[test]
    fn launch_overlay_renders_blocked_target_state() {
        let mut app = App::new(CliTool::Hermes, CliTool::Hermes).expect("app");
        app.show_launch = true;
        app.pending_target = CliTool::Hermes;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Launch");
        assert_screen_contains(&screen, "Choose target CLI");
        assert_screen_contains(&screen, "BLOCKED");
        assert_screen_contains(&screen, "Readiness");
        assert_screen_contains(&screen, "Source Health");
        assert_screen_contains(&screen, "Capsule Health");
        assert_screen_contains(&screen, "Target Readiness");
        assert_screen_contains(&screen, "FAIL");
        assert_screen_contains(&screen, "target_support");
        assert_screen_contains(&screen, "raw resume is known failed");
        assert_screen_contains(&screen, "enter/y Blocked");
    }

    #[test]
    fn launch_overlay_explains_stale_handoff_compiler_mismatch() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        let compiler = "agent:codex:handoff".to_string();
        app.data.compilers.insert(0, compiler);
        app.selected_compiler = 0;
        app.show_launch = true;
        app.pending_target = CliTool::Hermes;

        let screen = render_text(&app, 140, 52);

        assert_screen_contains(&screen, "选择目标 CLI");
        assert_screen_contains(&screen, "当前 handoff 由其他 skill/compiler 生成");
        assert_screen_contains(&screen, "请用当前 skill 重新生成后再启动");
        assert_screen_contains(&screen, "按 Enter 用当前 skill 重新生成 handoff");
        assert_screen_contains(&screen, "enter 重新生成");
        assert!(!screen.contains("generated_by"), "{screen}");
        assert!(!screen.contains(" vs compiler "), "{screen}");
    }

    #[test]
    fn launch_review_stale_skill_prompts_regeneration_not_draft_run() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        let compiler = "agent:codex:handoff".to_string();
        app.data.compilers.insert(0, compiler);
        app.selected_compiler = 0;
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;

        let screen = render_text(&app, 140, 52);

        assert_screen_contains(&screen, "Handoff Review");
        assert_screen_contains(&screen, "重新生成 Handoff Review");
        assert_screen_contains(&screen, "当前 handoff 由其他 skill/compiler 生成");
        assert_screen_contains(&screen, "按 Enter 用当前 skill 重新生成 handoff");
        assert_screen_contains(&screen, "技能:");
        assert_screen_contains(&screen, "handoff");
        assert!(!screen.contains("agent:codex:handoff"), "{screen}");
        assert!(!screen.contains("Draft Handoff"), "{screen}");
        assert!(!screen.contains("草稿不可运行"), "{screen}");
        assert!(!screen.contains("选择 AI skill 后可运行"), "{screen}");
        assert!(
            !screen.contains("Production handoff should use"),
            "{screen}"
        );
    }

    #[test]
    fn real_builtin_draft_launch_requires_skill_without_rendering_draft_review() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        app.data.sessions[app.selected_session].source_provenance = SourceProvenance::Real;
        app.selected_compiler = app
            .data
            .compilers
            .iter()
            .position(|compiler| compiler == "engineering-handoff")
            .expect("engineering-handoff compiler");
        app.data.capsule.compiler = "engineering-handoff".into();
        app.show_launch = true;
        app.pending_target = CliTool::Hermes;

        let target_screen = render_text(&app, 140, 48);
        assert_screen_contains(&target_screen, "先选择 AI handoff skill");
        assert_screen_contains(&target_screen, "真实 session 不再进入草稿 Review");
        assert_screen_contains(&target_screen, "S/enter 选择 Skill");
        assert!(
            !target_screen.contains("engineering-handoff is a built-in"),
            "{target_screen}"
        );

        app.launch_review = true;
        let review_screen = render_text(&app, 140, 48);
        assert_screen_contains(&review_screen, "先选择 AI handoff skill");
        assert_screen_contains(&review_screen, "内置草稿模板不会调用 AI skill");
        assert!(!review_screen.contains("Draft Handoff"), "{review_screen}");
        assert!(
            !review_screen.contains("This preview uses the built-in deterministic draft compiler"),
            "{review_screen}"
        );
        assert!(!review_screen.contains("草稿不可运行"), "{review_screen}");
    }

    #[test]
    fn launch_review_renders_explicit_handoff_action() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;
        let screen = render_text(&app, 120, 48);

        assert_screen_contains(&screen, "Handoff Review");
        assert_screen_contains(&screen, "handoff");
        assert_screen_contains(&screen, "Next:");
        assert_screen_contains(&screen, "Path:");
        assert_screen_contains(&screen, "source Codex codex-cxcp-des...");
        assert_screen_contains(&screen, "-> rewind evt-091 -> target Hermes");
        assert_screen_contains(&screen, "Portrait:");
        assert_screen_contains(&screen, "user 1 / assistant 1 / tool 4 / rewind 1");
        assert_screen_contains(&screen, "Target receives");
        assert_screen_contains(&screen, "Prompt");
        assert_screen_contains(&screen, "Draft Handoff");
        assert_screen_contains(&screen, "Goal");
        assert_screen_contains(&screen, "Readiness");
        assert_screen_contains(&screen, "PASS");
        assert_screen_contains(&screen, "target_support");
        assert_screen_contains(&screen, "Target command:");
        assert_screen_contains(&screen, "r Run");
        assert_screen_contains(&screen, "y Copy command");
    }

    #[test]
    fn launch_review_uses_simplified_chinese_ui_when_configured() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;
        let screen = render_text(&app, 120, 48);

        assert_screen_contains(&screen, "下一步:");
        assert_screen_contains(&screen, "目标会收到");
        assert_screen_contains(&screen, "就绪检查");
        assert_screen_contains(&screen, "目标命令:");
        assert_screen_contains(&screen, "r 运行");
        assert_screen_contains(&screen, "y 复制命令");
    }

    #[test]
    fn launch_review_pending_renders_loading_state() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");

        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Generating Handoff Review");
        assert_screen_contains(&screen, "Compiler:");
        assert_screen_contains(&screen, "Stage:");
        assert_screen_contains(&screen, "queued");
        assert_screen_contains(&screen, "waiting for background handoff worker");
        assert_screen_contains(&screen, "Esc hides this panel");
        assert_screen_contains(&screen, "wait background job");
    }

    #[test]
    fn launch_review_error_renders_persistent_retry_panel() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.set_ui_preferences_for_test(crate::core::config::UiPreferencesConfig {
            language: crate::core::config::UiLanguage::ZhHans,
            theme: crate::core::config::UiThemeName::Moonbox,
        });
        app.show_launch = true;
        app.pending_target = CliTool::Hermes;
        app.set_launch_review_error_for_test(LaunchReviewErrorState {
            target: CliTool::Hermes,
            compiler_id: "agent:claude:handoff".into(),
            message: "invalid compiler config agent:claude:handoff: sdk_not_found: runner=Claude; cli=/opt/homebrew/bin/claude; module=claude_agent_sdk; checked=python3,/opt/homebrew/bin/python3; install=/opt/homebrew/bin/python3 -m pip install claude-agent-sdk; env=MOONBOX_CLAUDE_AGENT_SDK_PYTHON".into(),
            elapsed_ms: 42,
        });

        let screen = render_text(&app, 140, 42);

        assert_screen_contains(&screen, "Handoff Review 失败");
        assert_screen_contains(&screen, "handoff 没有生成，目标启动已禁用。");
        assert_screen_contains(&screen, "原因");
        assert_screen_contains(&screen, "agent:claude:handoff");
        assert_screen_contains(&screen, "缺少 SDK 模块");
        assert_screen_contains(&screen, "claude_agent_sdk");
        assert_screen_contains(&screen, "已检查 Python");
        assert_screen_contains(
            &screen,
            "/opt/homebrew/bin/python3 -m pip install claude-agent-sdk",
        );
        assert_screen_contains(&screen, "仅安装 Claude CLI 还不足以运行当前 SDK runner。");
        assert_screen_contains(&screen, "下一步：按 Enter 用当前 skill 重试");
        assert_screen_contains(&screen, "Enter 重试");
        assert_screen_contains(&screen, "S 技能");
        assert_screen_contains(&screen, "y/r 不可用");
    }

    #[test]
    fn launch_review_scrolls_to_exact_target_prompt() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;
        app.modal_scroll = 38;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Content sent to target");
        assert_screen_contains(&screen, "You are receiving a Moonbox cross-CLI handoff");
        assert_screen_contains(&screen, "- CLI: Hermes");
    }

    #[test]
    fn launch_overlay_renders_warning_readiness_signal() {
        let mut app = App::new(CliTool::Codex, CliTool::Codex).expect("app");
        app.show_launch = true;
        app.pending_target = CliTool::Codex;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "WARN");
        assert_screen_contains(&screen, "Readiness");
        assert_screen_contains(&screen, "Target Readiness");
        assert_screen_contains(&screen, "target_support");
        assert_screen_contains(&screen, "Same-CLI handoff");
        assert_screen_contains(&screen, "enter Review");
    }
}
