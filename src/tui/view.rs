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
    app::{App, CommandPaletteEntry, Focus, HandoffTrailFrame},
    core::compiler,
    core::model::{
        CliTool, CompilerPresetInfo, CompilerPresetKind, CompilerPresetStatus,
        LaunchValidationState, SessionRuntimeStatus, SessionStatus, SourceAdapterReport,
        SourceFidelityStatus, SourceProvenance, TimelineAttachment, TimelineEvent, TimelineKind,
        VerificationReport, VerificationStatus, WorkCapsule,
    },
};

use super::theme;

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme::TEXT)),
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
        Block::default().style(Style::default().fg(theme::TEXT)),
        area,
    );
    let root = centered(area, 52, 32);
    let spinner = ["|", "/", "-", "\\"][tick % 4];
    let lines = vec![
        Line::from(vec![
            Span::styled(
                " MOONBOX ",
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("月光宝盒", Style::default().fg(theme::MUTED)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(spinner, Style::default().fg(theme::GOLD)),
            Span::raw(" indexing source sessions"),
        ]),
        Line::from(vec![
            Span::raw("   bounded scan "),
            Span::styled("active", Style::default().fg(theme::GREEN)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("q", Style::default().fg(theme::BLUE)),
            Span::raw(" quit   "),
            Span::styled("ctrl-c", Style::default().fg(theme::BLUE)),
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
    let preflight = preflight_summary(app);

    let title = Line::from(header_title_spans(area.width));
    let state = Line::from(vec![
        Span::raw("Filter "),
        Span::styled("[ ]", Style::default().fg(theme::MUTED)),
        Span::raw(": "),
        Span::styled(
            filter_label(app),
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Data: "),
        Span::styled(
            app.current_data_space().label.clone(),
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Skill: "),
        Span::styled(&app.data.capsule.compiler, Style::default().fg(theme::CYAN)),
    ]);
    let token_budget = app
        .current_session()
        .map(|session| format_token_count(session.token_count))
        .unwrap_or_else(|| "-".into());
    let budget = Line::from(vec![
        Span::raw("Tokens: "),
        Span::styled(
            token_budget,
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Pre-flight: "),
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
            .block(panel_block(" Moonbox CLI ", app.focus == Focus::Branches))
            .alignment(Alignment::Left),
        area,
    );
}

fn header_title_spans(width: u16) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        " MOONBOX ",
        Style::default()
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD),
    )];
    if width >= 120 {
        spans.push(Span::styled("月光宝盒", Style::default().fg(theme::MUTED)));
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
            Self::Pass => theme::GREEN,
            Self::Warn => theme::GOLD,
            Self::Blocked => theme::RED,
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
            Self::Strong => theme::CONFIDENCE_STRONG,
            Self::Medium => theme::CONFIDENCE_MEDIUM,
            Self::Weak => theme::CONFIDENCE_WEAK,
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

fn render_sessions(frame: &mut Frame, area: Rect, app: &App) {
    let visible = app.visible_session_indices();
    let selected = visible
        .iter()
        .position(|index| *index == app.selected_session)
        .unwrap_or(0);
    let items: Vec<ListItem> = if visible.is_empty() {
        let mut lines = vec![
            Line::from(Span::styled(
                "No sessions match",
                Style::default().fg(theme::MUTED),
            )),
            Line::from(vec![
                Span::styled("Filter: ", Style::default().fg(theme::MUTED)),
                Span::styled(app.session_filter.label(), Style::default().fg(theme::CYAN)),
            ]),
        ];
        if !app.search_query.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Query: ", Style::default().fg(theme::MUTED)),
                Span::styled(
                    format!("/{}", app.search_query),
                    Style::default().fg(theme::CYAN),
                ),
            ]));
        }
        lines.push(Line::from(Span::styled(
            "Press a to clear",
            Style::default().fg(theme::MUTED),
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
                            .fg(theme::TEXT)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw(" ")
                };
                let marker = session_row_marker(session, app.is_session_starred(session));
                let mut title_spans = vec![selector, Span::raw(" ")];
                if let Some(marker) = marker {
                    title_spans.push(marker);
                    title_spans.push(Span::raw(" "));
                }
                title_spans.extend([
                    Span::styled(source_pill(session.cli), source_tool_style(session.cli)),
                    Span::raw("  "),
                    Span::styled(&session.title, session_title_style(selected_row)),
                ]);
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
                "Sessions · {} {}",
                app.session_filter.label(),
                session_position_label(visible.len(), selected)
            ),
            area,
        )
    } else if area.width < 28 {
        stable_panel_title(format!("Sessions /{}", app.search_query), area)
    } else {
        stable_panel_title(
            format!(
                "Sessions · {} {}",
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
                .fg(theme::TEXT)
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
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
    }
}

fn session_row_marker(
    session: &crate::core::model::SessionSummary,
    starred: bool,
) -> Option<Span<'static>> {
    match session.status {
        SessionStatus::Warning => Some(Span::styled("▲", Style::default().fg(theme::GOLD))),
        SessionStatus::Failed => Some(Span::styled(
            "!",
            Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
        )),
        SessionStatus::Healthy if starred => Some(Span::styled(
            "*",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        SessionStatus::Healthy => None,
    }
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
    _app: &App,
    session: &crate::core::model::SessionSummary,
    selected: bool,
    width: u16,
) -> Line<'static> {
    let updated = relative_time_label(&session.updated_at, current_unix_timestamp())
        .unwrap_or_else(|| session.updated.clone());
    let mut spans = vec![Span::raw("    ")];
    let metric_style = if selected {
        Style::default().fg(theme::CYAN)
    } else {
        Style::default().fg(theme::MUTED)
    };
    spans.push(Span::styled(
        session_inventory_metric(session),
        metric_style,
    ));
    spans.push(Span::styled(" · ", Style::default().fg(theme::BORDER)));
    spans.push(Span::styled(updated, Style::default().fg(theme::MUTED)));
    if width >= 48
        && let Some(branch) = session
            .branch
            .as_deref()
            .filter(|branch| !branch.is_empty())
    {
        let max_branch = usize::from(width.saturating_sub(34)).clamp(8, 28);
        spans.push(Span::styled(" · ", Style::default().fg(theme::BORDER)));
        spans.push(Span::styled(
            review_snippet(branch, max_branch),
            Style::default().fg(theme::MUTED),
        ));
    }
    Line::from(spans)
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
    let mut text = format_event_count(session.event_count);
    if let Some(tokens) = session.token_count {
        text.push_str(" · ");
        text.push_str(&format_token_count(Some(tokens)));
    }
    text
}

fn format_event_count(event_count: usize) -> String {
    if event_count == 1 {
        "1 event".into()
    } else {
        format!("{event_count} events")
    }
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

fn source_tool_color(tool: CliTool) -> Color {
    match tool {
        CliTool::Codex => theme::BLUE,
        CliTool::Claude => theme::PURPLE,
        CliTool::Hermes => theme::ORANGE,
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
    if app.search_query.is_empty() {
        app.session_filter.label().to_string()
    } else {
        format!("{} · /{}", app.session_filter.label(), app.search_query)
    }
}

fn format_token_count(token_count: Option<usize>) -> String {
    match token_count {
        Some(count) if count >= 1_000 => format!("{}K", count / 1_000),
        Some(count) => count.to_string(),
        None => "-".into(),
    }
}

fn session_health_style(status: SessionStatus) -> Style {
    match status {
        SessionStatus::Healthy => Style::default().fg(theme::GREEN),
        SessionStatus::Warning => Style::default().fg(theme::GOLD),
        SessionStatus::Failed => Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
    }
}

fn source_provenance_style(provenance: SourceProvenance) -> Style {
    match provenance {
        SourceProvenance::Real => Style::default()
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD),
        SourceProvenance::Fixture => Style::default().fg(theme::BLUE),
        SourceProvenance::Missing => Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
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
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
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

        let detail_style = if active {
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD)
        } else if is_rewind || group.kind() == TimelineKind::RewindPoint {
            Style::default().fg(theme::TEXT)
        } else {
            Style::default().fg(theme::MUTED)
        };
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
            "No user or assistant turns loaded",
            Style::default().fg(theme::MUTED),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Timeline ", app.focus == Focus::Timeline))
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
    if is_rewind { theme::ROLE_REWIND } else { color }
}

fn timeline_prefix_style(active: bool, accent: Color) -> Style {
    if active {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
    }
}

fn timeline_marker_style(active: bool, selected: bool, is_rewind: bool) -> Style {
    if active {
        Style::default()
            .fg(theme::CYAN)
            .add_modifier(Modifier::BOLD)
    } else if is_rewind {
        Style::default()
            .fg(theme::ROLE_REWIND)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default()
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
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
        return "  └ ";
    }
    if active && ai_group && line_index == 0 {
        return "  • ";
    }
    if active {
        return "    ";
    }
    if ai_group && event_offset > 0 && line_index == 0 {
        return "  · ";
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
        return ("REWIND".into(), theme::GOLD);
    }
    match group.kind() {
        TimelineKind::User => ("USER".into(), theme::BLUE),
        TimelineKind::Assistant if group.len() > 1 => (
            format!("{} x{}", assistant_source_label(source), group.len()),
            theme::GOLD,
        ),
        TimelineKind::Assistant => (assistant_source_label(source).into(), theme::GOLD),
        TimelineKind::Tool => ("TOOL".into(), theme::MUTED),
        TimelineKind::Compact => ("COMPACT".into(), theme::CYAN),
        TimelineKind::Error => ("ERROR".into(), theme::RED),
        TimelineKind::GitDiff => ("GIT DIFF".into(), theme::GREEN),
        TimelineKind::RewindPoint => ("REWIND".into(), theme::GOLD),
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
    let capsule = &app.data.capsule;
    let mut lines = session_detail_lines(app);

    if app.is_session_load_pending() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Loading",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Hydrating timeline and capsule preview for the selected session.",
            Style::default().fg(theme::TEXT),
        )));
        lines.push(Line::from(Span::styled(
            "  Launch, verify, compile, and rewind wait until this completes.",
            Style::default().fg(theme::MUTED),
        )));
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(
                    " Session Details ",
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
            .block(panel_block(
                " Session Details ",
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
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        review_label_line("State", capsule.state.clone(), theme::GOLD),
        review_label_line(
            "Rewind",
            review_snippet(&capsule.rewind_point, 96),
            theme::GOLD,
        ),
        review_label_line("Goal", review_snippet(&capsule.goal, 96), theme::BLUE),
        review_label_line(
            "Risk",
            capsule
                .risks
                .first()
                .map(|risk| review_snippet(risk, 96))
                .unwrap_or_else(|| "none".into()),
            theme::RED,
        ),
        Line::from(Span::styled(
            "Press c to refresh and review the full handoff.",
            Style::default().fg(theme::MUTED),
        )),
    ]
}

fn session_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(session) = app.current_session() else {
        return vec![
            Line::from(Span::styled(
                "Real Session Metadata",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  No session selected",
                Style::default().fg(theme::MUTED),
            )),
        ];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "Real Session Metadata",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        metadata_line(
            "Raw Title",
            &session.title,
            Style::default().fg(theme::TEXT),
        ),
        metadata_line(
            "Source",
            &format!("{} · {}", session.cli, session.source_provenance),
            source_provenance_style(session.source_provenance),
        ),
        source_fidelity_line(app, session.cli),
        metadata_line(
            "Portrait",
            &session_portrait_summary(app, session),
            Style::default().fg(theme::CYAN),
        ),
        metadata_line(
            "Updated",
            &session.updated,
            Style::default().fg(theme::BLUE),
        ),
        metadata_line(
            "Runtime",
            &session_runtime_detail(session),
            session_runtime_style(session.runtime_status),
        ),
        metadata_line("Cwd", &session.cwd, Style::default().fg(theme::TEXT)),
        metadata_line(
            "Branch",
            session.branch.as_deref().unwrap_or("-"),
            Style::default().fg(theme::CYAN),
        ),
        metadata_line(
            "Events",
            &session.event_count.to_string(),
            Style::default().fg(theme::MUTED),
        ),
        metadata_line(
            "Tokens",
            &format_token_count(session.token_count),
            Style::default().fg(theme::GOLD),
        ),
        metadata_line(
            "Source Health",
            &session_health_detail(session),
            session_health_style(session.status),
        ),
    ];
    if let Some(path) = &session.source_path {
        lines.push(metadata_line(
            "Path",
            path,
            Style::default().fg(theme::MUTED),
        ));
    }
    lines
}

fn source_fidelity_line(app: &App, cli: CliTool) -> Line<'static> {
    let Some(report) = source_adapter_report(app, cli) else {
        return metadata_line(
            "Fidelity",
            "missing · none",
            source_fidelity_style(SourceFidelityStatus::Missing),
        );
    };
    let value = source_fidelity_detail(report);
    metadata_line(
        "Fidelity",
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
        SourceFidelityStatus::FullFidelity => Style::default().fg(theme::GREEN),
        SourceFidelityStatus::Partial => Style::default().fg(theme::GOLD),
        SourceFidelityStatus::Fallback => Style::default().fg(theme::ORANGE),
        SourceFidelityStatus::Missing => Style::default().fg(theme::RED),
    }
}

fn metadata_line(label: &'static str, value: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(theme::MUTED)),
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
        SessionRuntimeStatus::Active => Style::default().fg(theme::GREEN),
        SessionRuntimeStatus::Inactive => Style::default().fg(theme::MUTED),
        SessionRuntimeStatus::Unknown => Style::default().fg(theme::GOLD),
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
        Paragraph::new(lines).block(panel_block(" Action Path ", app.focus == Focus::Branches)),
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
        .unwrap_or_else(|| ("no session".into(), theme::MUTED));
    let rewind = format!("rewind {}", short_identifier(&app.rewind_event_id, 12));
    let target_cli = if app.show_launch {
        app.pending_target
    } else {
        app.data.target
    };
    let target = format!("target {target_cli}");
    let nodes = [
        (session, source_color),
        (rewind, theme::ROLE_REWIND),
        (target, theme::ROLE_TARGET),
    ];

    let mut spans = vec![Span::styled("   ", Style::default().fg(theme::MUTED))];
    for (idx, (label, color)) in nodes.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" -> ", Style::default().fg(theme::BORDER)));
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
            .fg(theme::GOLD)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
    };
    let rewind_style = if (2..=3).contains(&frame.step) {
        Style::default()
            .fg(theme::GOLD)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
    };
    let target_style = if frame.step >= 5 {
        Style::default()
            .fg(theme::CYAN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
    };
    Line::from(vec![
        Span::styled("   handoff trail  ", Style::default().fg(theme::MUTED)),
        Span::styled("source", source_style),
        Span::styled(arrow_one, Style::default().fg(theme::ROLE_REWIND)),
        Span::styled("rewind", rewind_style),
        Span::styled(arrow_two, Style::default().fg(theme::ROLE_TARGET)),
        Span::styled("target", target_style),
        Span::styled("  ", Style::default().fg(theme::BORDER)),
        Span::styled(
            frame.phase.label(),
            Style::default()
                .fg(theme::ROLE_TARGET)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn cwd_inventory_line(app: &App, width: u16) -> Line<'static> {
    let Some(session) = app.current_session() else {
        return Line::from(Span::styled(
            "   cwd: no session",
            Style::default().fg(theme::MUTED),
        ));
    };
    let codex = cwd_session_count(app, &session.cwd, CliTool::Codex);
    let claude = cwd_session_count(app, &session.cwd, CliTool::Claude);
    let hermes = cwd_session_count(app, &session.cwd, CliTool::Hermes);
    let max_path_chars = usize::from(width.saturating_sub(56)).clamp(12, 64);
    Line::from(vec![
        Span::styled("   cwd: ", Style::default().fg(theme::MUTED)),
        Span::styled(
            review_snippet(&session.cwd, max_path_chars),
            Style::default().fg(theme::TEXT),
        ),
        Span::styled(" · ", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!("Codex {codex}"),
            Style::default().fg(source_tool_color(CliTool::Codex)),
        ),
        Span::styled(" · ", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!("Claude {claude}"),
            Style::default().fg(source_tool_color(CliTool::Claude)),
        ),
        Span::styled(" · ", Style::default().fg(theme::BORDER)),
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
                        .fg(theme::GOLD)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(input, Style::default().fg(theme::TEXT)),
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
                .border_style(Style::default().fg(theme::BORDER))
                .style(Style::default()),
        ),
        area,
    );
}

type KeyHint = (&'static str, &'static str);

fn active_key_hints(app: &App) -> Vec<KeyHint> {
    if app.show_launch {
        if app.launch_review {
            let enter_hint = if app
                .validate_launch_for_target(app.pending_target)
                .is_blocked()
            {
                "Blocked"
            } else {
                "Handoff"
            };
            return vec![
                ("y", "Copy"),
                ("enter", enter_hint),
                ("PgUp/Dn", "Scroll"),
                ("Esc/q", "Close"),
            ];
        }
        if app.validate_launch_for_target(app.pending_target).state
            == LaunchValidationState::Blocked
        {
            return vec![
                ("j/k", "Target"),
                ("enter/y", "Blocked"),
                ("PgUp/Dn", "Scroll"),
                ("Esc", "Cancel"),
            ];
        }
        return vec![
            ("j/k", "Target"),
            ("enter", "Review"),
            ("y", "Unavailable"),
            ("PgUp/Dn", "Scroll"),
            ("Esc", "Cancel"),
        ];
    }
    if app.show_open_original {
        return vec![
            ("y", "Copy"),
            ("j/k", "Scroll"),
            ("PgUp/Dn", "Scroll"),
            ("Esc", "Close"),
        ];
    }
    if app.show_skill_picker {
        return vec![
            ("j/k", "Skill"),
            ("enter", "Apply"),
            ("y", "Copy Ref"),
            ("q", "Close"),
        ];
    }
    if app.show_doctor {
        return vec![
            ("v", "Verify"),
            ("r", "Refresh"),
            ("y", "Copy JSON"),
            ("j/k", "Scroll"),
            ("Esc", "Close"),
        ];
    }
    if app.show_capsules {
        return vec![
            ("r", "Refresh"),
            ("j/k", "Scroll"),
            ("PgUp/Dn", "Scroll"),
            ("Esc", "Close"),
        ];
    }
    if app.show_timeline_detail {
        return vec![
            ("j/k", "Scroll"),
            ("PgUp/Dn", "Scroll"),
            ("Esc", "Close"),
            ("q", "Close"),
        ];
    }
    if app.show_help {
        return vec![
            ("j/k", "Scroll"),
            ("PgUp/Dn", "Scroll"),
            ("Esc", "Close"),
            ("q", "Close"),
        ];
    }

    match app.focus {
        Focus::Sessions => vec![
            ("j/k", "Sessions"),
            ("gg/G", "Jump"),
            ("/", "Search"),
            ("[ ]", "Source"),
            ("{ }", "Data"),
            ("a", "Clear"),
            ("s", "Star"),
            ("S", "Skill"),
            ("+", "Zoom"),
            ("-", "Restore"),
            ("o", "Original"),
            ("enter", "Open"),
            ("x/H", "Handoff"),
            ("tab", "Next"),
        ],
        Focus::Timeline => vec![
            ("j/k", "Events"),
            ("gg/G", "Jump"),
            ("e", "Detail"),
            ("space", "Rewind"),
            ("c", "Review"),
            ("+", "Zoom"),
            ("-", "Restore"),
            ("tab", "Next"),
            (":", "Cmd"),
            ("q", "Quit"),
        ],
        Focus::Capsule => vec![
            ("j/k", "Scroll"),
            ("gg/G", "Top/Bottom"),
            ("c", "Review"),
            ("v", "Verify"),
            ("S", "Skill"),
            ("+", "Zoom"),
            ("-", "Restore"),
            ("tab", "Next"),
            (":", "Cmd"),
            ("q", "Quit"),
        ],
        Focus::Branches => vec![
            ("enter", "Open"),
            ("x/H", "Handoff"),
            ("o", "Original"),
            ("space", "Rewind"),
            ("D", "Pre-flight"),
            ("+", "Zoom"),
            ("-", "Restore"),
            ("tab", "Next"),
            (":", "Cmd"),
            ("?", "Help"),
            ("q", "Quit"),
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

fn status_line(app: &App) -> Line<'_> {
    let (color, bold) = if app.status_message.contains("cancelled")
        || app.status_message.contains("No session")
        || app.status_message.contains("Unknown")
        || app.status_message.contains("NEEDS REVIEW")
    {
        (theme::ORANGE, true)
    } else if app.status_message.contains("PASS")
        || app.status_message.contains("saved")
        || app.status_message.contains("compiled")
        || app.status_message.contains("refreshed")
        || app.status_message.contains("cleared")
    {
        (theme::GREEN, true)
    } else {
        (theme::MUTED, false)
    };
    let message_style = if bold {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color)
    };

    Line::from(vec![
        Span::styled("Status ", Style::default().fg(theme::MUTED)),
        Span::styled(&app.status_message, message_style),
    ])
}

fn render_help(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 52, 48);
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "Moonbox Keys",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::raw("j/k, gg/G       navigate"),
        Line::raw("tab, shift-tab  switch panel"),
        Line::raw("f               cycle session source filter"),
        Line::raw("a               clear source and text filters"),
        Line::raw("s               star / unstar selected session"),
        Line::raw("*               star / unstar selected session alias"),
        Line::raw("/text           filter sessions by text"),
        Line::raw("o               open original session with original CLI"),
        Line::raw("enter           open selected session with original CLI"),
        Line::raw("e               open selected Timeline event detail"),
        Line::raw("x / H           choose target for handoff"),
        Line::raw("D               open pre-flight details"),
        Line::raw("[ / ]           previous / next session source filter"),
        Line::raw("space           set rewind point"),
        Line::raw("c               refresh capsule and open handoff review"),
        Line::raw("v, S            verify capsule, switch skill"),
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
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(app.command_input.clone(), Style::default().fg(theme::TEXT)),
            Span::styled("▏", Style::default().fg(theme::CYAN)),
        ]),
        Line::from(Span::styled(
            "Tab complete   Enter run selected   j/k choose   Esc close",
            Style::default().fg(theme::MUTED),
        )),
        Line::raw(""),
    ];

    if matches.is_empty() {
        lines.extend([
            Line::from(Span::styled(
                "No commands match",
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Try open, capsule, handoff, source, data, skill, doctor, or help.",
                Style::default().fg(theme::MUTED),
            )),
        ]);
    } else {
        lines.push(Line::from(Span::styled(
            "Matches",
            Style::default()
                .fg(theme::BLUE)
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
                Style::default().fg(theme::MUTED),
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
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT)
    };
    Line::from(vec![
        Span::styled(marker, Style::default().fg(theme::CYAN)),
        Span::raw(" "),
        Span::styled(format!("{:<14}", entry.command), command_style),
        Span::styled(
            format!(" {:<8} ", entry.badge),
            command_palette_badge_style(entry),
        ),
        Span::styled(entry.description, Style::default().fg(theme::MUTED)),
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
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Params: ", Style::default().fg(theme::BLUE)),
            Span::styled(entry.params, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Aliases: ", Style::default().fg(theme::BLUE)),
            Span::styled(aliases, Style::default().fg(theme::MUTED)),
        ]),
        Line::from(vec![
            Span::styled("Risk: ", Style::default().fg(theme::BLUE)),
            Span::styled(
                risk,
                Style::default().fg(if entry.dangerous {
                    theme::RED
                } else {
                    theme::MUTED
                }),
            ),
        ]),
    ]
}

fn command_palette_badge_style(entry: &CommandPaletteEntry) -> Style {
    let color = if entry.dangerous {
        theme::RED
    } else {
        match entry.badge {
            "CHECK" => theme::GREEN,
            "DRY-RUN" | "PREVIEW" | "REVIEW" => theme::GOLD,
            "SWITCH" | "PICKER" => theme::CYAN,
            _ => theme::MUTED,
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
                    .fg(theme::GOLD)
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
            Span::styled("Compiler: ", Style::default().fg(theme::BLUE)),
            Span::styled(
                compile_status_label(app.compile_status),
                Style::default()
                    .fg(verification_color(preflight.compiler_status))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", app.data.capsule.compiler),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        Line::from(vec![
            Span::styled("Doctor: ", Style::default().fg(theme::BLUE)),
            Span::styled(
                app.doctor_report.status.to_string(),
                Style::default()
                    .fg(verification_color(app.doctor_report.status))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} checks", app.doctor_report.checks.len()),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        Line::from(vec![
            Span::styled("Verify: ", Style::default().fg(theme::BLUE)),
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
                        theme::RED
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                verify_detail_text(verification.as_ref(), preflight.verify_reviewed),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        Line::from(Span::styled(
            "v Verify   r Refresh doctor   y Copy JSON   Esc Close",
            Style::default().fg(theme::MUTED),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "Verifier evidence",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(preflight_readiness_lines(verification.as_ref(), 3));
    lines.extend([
        Line::raw(""),
        Line::from(Span::styled(
            "Environment doctor",
            Style::default()
                .fg(theme::BLUE)
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
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().fg(theme::MUTED)),
            Span::styled(&check.detail, Style::default().fg(theme::MUTED)),
        ]));
        lines.push(Line::raw(""));
    }

    lines.extend([
        Line::from(Span::styled(
            "Read-only diagnostics. No timeline load, resume, launch, or target spawn.",
            Style::default().fg(theme::MUTED),
        )),
        Line::from(Span::styled(
            "v Verify   r Refresh doctor   y Copy JSON   Esc Close",
            Style::default().fg(theme::MUTED),
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
    let catalog = compiler::compiler_catalog_entries();
    let mut lines = vec![
        Line::from(Span::styled(
            "Choose compiler skill",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Compression and compatibility live in replaceable compiler skills.",
            Style::default().fg(theme::MUTED),
        )),
        Line::raw(""),
    ];

    for (index, id) in app.data.compilers.iter().enumerate() {
        let info = catalog
            .iter()
            .find(|entry| entry.id == *id)
            .cloned()
            .unwrap_or_else(|| fallback_compiler_info(id));
        let pending = index == app.pending_compiler;
        let active = index == app.selected_compiler;
        let status_color = compiler_status_color(info.status);
        let row_style = if pending {
            Style::default()
                .fg(Color::Black)
                .bg(status_color)
                .add_modifier(Modifier::BOLD)
        } else if active {
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
        };
        let muted_style = if pending {
            Style::default().fg(theme::TEXT)
        } else {
            Style::default().fg(theme::MUTED)
        };
        let cursor = if pending { ">" } else { " " };
        let active_mark = if active { "active" } else { "      " };
        lines.push(Line::from(vec![
            Span::styled(format!("{cursor} "), row_style),
            Span::styled(format!("{:<24}", info.id), row_style),
            Span::styled("  ", row_style),
            Span::styled(
                format!("{:<7}", compiler_status_label(info.status)),
                row_style,
            ),
            Span::styled("  ", row_style),
            Span::styled(format!("{:<11}", compiler_kind_label(info.kind)), row_style),
            Span::styled("  ", row_style),
            Span::styled(active_mark, row_style),
        ]));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(compiler_description(&info), muted_style),
        ]));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("stars: ", Style::default().fg(theme::MUTED)),
            Span::styled(format_star_count(&info), Style::default().fg(theme::GOLD)),
            Span::styled("  link: ", Style::default().fg(theme::MUTED)),
            Span::styled(compiler_reference(&info), Style::default().fg(theme::CYAN)),
        ]));
        lines.push(Line::raw(""));
    }

    if app.data.compilers.is_empty() {
        lines.push(Line::from(Span::styled(
            "No compiler skills configured.",
            Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(Span::styled(
        "j/k choose   enter apply   y copy link/command   q close",
        Style::default().fg(theme::MUTED),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Skill Picker ", true))
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
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Local continuation objects. Listing never opens, resumes, or launches source sessions.",
            Style::default().fg(theme::MUTED),
        )),
        Line::raw(""),
    ];

    if let Some(error) = &app.saved_capsule_error {
        lines.push(Line::from(Span::styled(
            "Capsule store unavailable",
            Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            review_snippet(error, 120),
            Style::default().fg(theme::MUTED),
        )));
    } else if app.saved_capsules.is_empty() {
        lines.push(Line::from(Span::styled(
            "No saved Capsules",
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Use `moonbox capsule save <name> --session <id> --target <cli>` to create one.",
            Style::default().fg(theme::MUTED),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Name", Style::default().fg(theme::BLUE)),
            Span::styled(
                "                     Target   Source session              Updated",
                Style::default().fg(theme::MUTED),
            ),
        ]));
        for capsule in &app.saved_capsules {
            let source_color = source_tool_color(capsule.source_cli);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<24}", review_snippet(&capsule.name, 24)),
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<8}", capsule.target_cli.id()),
                    Style::default().fg(theme::ROLE_TARGET),
                ),
                Span::styled(
                    format!("{:<28}", review_snippet(&capsule.source_session, 28)),
                    Style::default().fg(source_color),
                ),
                Span::styled(&capsule.updated_at, Style::default().fg(theme::MUTED)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("rewind ", Style::default().fg(theme::MUTED)),
                Span::styled(
                    review_snippet(&capsule.rewind_point, 52),
                    Style::default().fg(theme::ROLE_REWIND),
                ),
                Span::styled("  checksum ", Style::default().fg(theme::MUTED)),
                Span::styled(&capsule.checksum, Style::default().fg(theme::CYAN)),
            ]));
            lines.push(Line::raw(""));
        }
    }
    lines.push(Line::from(Span::styled(
        "r refresh   Esc/q close",
        Style::default().fg(theme::MUTED),
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
    let Some(event) = app.data.timeline.get(app.selected_event) else {
        frame.render_widget(
            Paragraph::new(vec![Line::from(Span::styled(
                "No timeline event selected",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            ))])
            .block(panel_block(" Timeline Detail ", true)),
            area,
        );
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", event.id),
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                timeline_kind_label(event.kind),
                Style::default()
                    .fg(timeline_kind_color(event.kind))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(&event.time, Style::default().fg(theme::MUTED)),
        ]),
        Line::from(vec![
            Span::styled("Title: ", Style::default().fg(theme::MUTED)),
            Span::styled(&event.title, Style::default().fg(theme::TEXT)),
        ]),
    ];

    if !event.metadata.attachments.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Attachments",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )));
        for attachment in &event.metadata.attachments {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    timeline_attachment_label(attachment),
                    Style::default().fg(theme::CYAN),
                ),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Body",
        Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::BOLD),
    )));
    lines.extend(timeline_detail_body_lines(&event.detail));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "j/k scroll   PgUp/PgDn page   Esc/q close",
        Style::default().fg(theme::MUTED),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Timeline Detail ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn timeline_detail_body_lines(detail: &str) -> Vec<Line<'static>> {
    if detail.trim().is_empty() {
        return vec![Line::from(Span::styled(
            "(empty)",
            Style::default().fg(theme::MUTED),
        ))];
    }
    detail
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line.to_owned(),
                Style::default().fg(theme::TEXT),
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
        TimelineKind::User => theme::BLUE,
        TimelineKind::Assistant => theme::GOLD,
        TimelineKind::Tool => theme::MUTED,
        TimelineKind::Compact => theme::CYAN,
        TimelineKind::Error => theme::RED,
        TimelineKind::GitDiff => theme::GREEN,
        TimelineKind::RewindPoint => theme::ROLE_REWIND,
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

fn compiler_status_label(status: CompilerPresetStatus) -> &'static str {
    match status {
        CompilerPresetStatus::Ready => "READY",
        CompilerPresetStatus::Warning => "WARN",
        CompilerPresetStatus::Disabled => "DISABLE",
    }
}

fn compiler_status_color(status: CompilerPresetStatus) -> Color {
    match status {
        CompilerPresetStatus::Ready => theme::GREEN,
        CompilerPresetStatus::Warning => theme::GOLD,
        CompilerPresetStatus::Disabled => theme::MUTED,
    }
}

fn compiler_kind_label(kind: CompilerPresetKind) -> &'static str {
    match kind {
        CompilerPresetKind::Builtin => "builtin",
        CompilerPresetKind::Environment => "env",
        CompilerPresetKind::Config => "config",
    }
}

fn compiler_description(info: &CompilerPresetInfo) -> String {
    info.description
        .clone()
        .unwrap_or_else(|| review_snippet(&info.reason, 96))
}

fn compiler_reference(info: &CompilerPresetInfo) -> String {
    info.homepage
        .clone()
        .or_else(|| info.command.clone())
        .unwrap_or_else(|| "built-in".into())
}

fn format_star_count(info: &CompilerPresetInfo) -> String {
    let Some(stars) = info.github_stars else {
        return match info.kind {
            CompilerPresetKind::Builtin => "n/a".into(),
            CompilerPresetKind::Environment | CompilerPresetKind::Config => "not configured".into(),
        };
    };
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
    let session = app
        .current_session()
        .map(|session| format!("{} / {}", session.cli, session.id))
        .unwrap_or_else(|| "No session selected".into());
    let handoff_label = app.launch_handoff_label();
    let pending_validation = app.validate_launch_for_target(app.pending_target);
    let pending_report = app.launch_verification_for_target(app.pending_target);
    if app.launch_review {
        let capsule = app.launch_capsule_for_target(app.pending_target);
        let launch_blocked = pending_validation.state == LaunchValidationState::Blocked;
        let mut lines = vec![
            Line::from(Span::styled(
                "Capsule Review",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Action: ", Style::default().fg(theme::BLUE)),
                Span::styled(
                    "handoff",
                    Style::default()
                        .fg(theme::CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            handoff_review_path_line(app),
            handoff_review_portrait_line(app),
            Line::from(vec![
                Span::styled("Session: ", Style::default().fg(theme::BLUE)),
                Span::raw(session),
            ]),
            Line::from(vec![
                Span::styled("Target: ", Style::default().fg(theme::BLUE)),
                Span::raw(app.pending_target.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Handoff label: ", Style::default().fg(theme::BLUE)),
                Span::raw(handoff_label),
            ]),
            Line::from(vec![
                Span::styled("Rewind: ", Style::default().fg(theme::BLUE)),
                Span::raw(capsule.rewind_point.clone()),
            ]),
            Line::from(vec![
                Span::styled("Validation: ", Style::default().fg(theme::BLUE)),
                Span::styled(
                    validation_label(pending_validation.state),
                    Style::default()
                        .fg(validation_color(pending_validation.state))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    pending_validation.summary(),
                    Style::default().fg(theme::MUTED),
                ),
            ]),
            Line::from(vec![
                Span::styled("Command: ", Style::default().fg(theme::BLUE)),
                Span::styled(app.launch_command(), Style::default().fg(theme::CYAN)),
            ]),
            Line::raw(""),
            Line::from(Span::styled(
                "Target receives",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        lines.extend(target_input_lines(app));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                if compiler::compiler_is_builtin(&capsule.compiler) {
                    "Draft Work Capsule"
                } else {
                    "Work Capsule"
                },
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
        ]);
        lines.extend(capsule_review_lines(&capsule, 1));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                "Readiness",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
        ]);
        lines.extend(readiness_lines(pending_report.as_ref(), 6));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                "Prompt argument",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            )),
        ]);
        lines.extend(target_prompt_lines(app));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                if launch_blocked {
                    "enter disabled   y copy blocked   Esc/q close"
                } else {
                    "enter handoff   y copy command   Esc/q close"
                },
                Style::default().fg(theme::MUTED),
            )),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(" Handoff Review ", true))
                .scroll((app.modal_scroll, 0))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let mut target_lines = Vec::new();
    for target in CliTool::ALL {
        let selected = target == app.pending_target;
        let validation = app.validate_launch_for_target(target);
        let style = if selected {
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(validation_color(validation.state))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(validation_color(validation.state))
        };
        let muted_style = if selected {
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(validation_color(validation.state))
        } else {
            Style::default().fg(theme::MUTED)
        };
        let cursor = if selected { ">" } else { " " };
        let mark = if selected { "[x]" } else { "[ ]" };
        target_lines.push(Line::from(vec![
            Span::styled(format!("{cursor} {mark} {target:<6}"), style),
            Span::styled(format!("  {}", validation_label(validation.state)), style),
        ]));
        target_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(validation.summary(), muted_style),
        ]));
    }
    let mut lines = vec![
        Line::from(Span::styled(
            "Choose target CLI",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Session: ", Style::default().fg(theme::BLUE)),
            Span::raw(session),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Target",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(target_lines);
    lines.extend([
        Line::raw(""),
        Line::from(vec![
            Span::styled("Selected: ", Style::default().fg(theme::BLUE)),
            Span::raw(app.pending_target.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Validation: ", Style::default().fg(theme::BLUE)),
            Span::styled(
                validation_label(pending_validation.state),
                Style::default()
                    .fg(validation_color(pending_validation.state))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                pending_validation.summary(),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Readiness",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
    ]);
    lines.extend(readiness_lines(pending_report.as_ref(), 6));
    lines.extend([
        if pending_validation.state == LaunchValidationState::Blocked {
            Line::from(Span::styled(
                "Launch review disabled until validation passes",
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            ))
        } else {
            Line::from(Span::styled(
                "Press enter to review the handoff command before copying",
                Style::default().fg(theme::GOLD),
            ))
        },
        Line::raw(""),
        Line::from(Span::styled(
            if pending_validation.state == LaunchValidationState::Blocked {
                "j/k choose target   enter/y blocked   Esc cancel"
            } else {
                "j/k choose target   enter review   y unavailable   Esc cancel"
            },
            Style::default().fg(theme::MUTED),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Launch ", true))
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
        Span::styled("Path: ", Style::default().fg(theme::BLUE)),
        Span::styled(
            session,
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" -> ", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!("rewind {}", short_identifier(&app.rewind_event_id, 12)),
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" -> ", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!("target {}", app.pending_target),
            Style::default()
                .fg(theme::CYAN)
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
        Span::styled("Portrait: ", Style::default().fg(theme::BLUE)),
        Span::styled(
            portrait,
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn capsule_review_lines(capsule: &WorkCapsule, _max_rows: usize) -> Vec<Line<'static>> {
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
    vec![
        review_label_line("Goal", review_snippet(&capsule.goal, 88), theme::BLUE),
        review_label_line("State", capsule.state.clone(), theme::GOLD),
        review_label_line("Decision", decision, theme::BLUE),
        review_label_line("Todo", todo, theme::BLUE),
        review_label_line("Risk", risk, theme::RED),
    ]
}

fn target_input_lines(app: &App) -> Vec<Line<'static>> {
    let Some(preview) = app.target_command_preview() else {
        return vec![Line::from(Span::styled(
            "No target input available for the current selection.",
            Style::default().fg(theme::MUTED),
        ))];
    };
    let cwd = preview.cwd.unwrap_or_else(|| "terminal default".into());
    vec![
        review_label_line("Program", preview.program, theme::BLUE),
        review_label_line("Cwd", cwd, theme::BLUE),
        review_label_line(
            "Args",
            format!(
                "{} arg(s), final arg is the handoff prompt",
                preview.args.len()
            ),
            theme::BLUE,
        ),
        review_label_line(
            "Prompt",
            "shown below, passed as the final argument".into(),
            theme::BLUE,
        ),
    ]
}

fn target_prompt_lines(app: &App) -> Vec<Line<'static>> {
    let Some(preview) = app.target_command_preview() else {
        return vec![Line::from(Span::styled(
            "No prompt available for the current selection.",
            Style::default().fg(theme::MUTED),
        ))];
    };
    preview
        .prompt
        .lines()
        .map(|line| {
            Line::from(vec![
                Span::styled("> ", Style::default().fg(theme::MUTED)),
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
        Span::styled(value, Style::default().fg(theme::TEXT)),
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

fn validation_label(state: LaunchValidationState) -> &'static str {
    match state {
        LaunchValidationState::Ready => "READY",
        LaunchValidationState::Warning => "WARN",
        LaunchValidationState::Blocked => "BLOCKED",
    }
}

fn validation_color(state: LaunchValidationState) -> Color {
    match state {
        LaunchValidationState::Ready => theme::GREEN,
        LaunchValidationState::Warning => theme::GOLD,
        LaunchValidationState::Blocked => theme::RED,
    }
}

fn readiness_lines(report: Option<&VerificationReport>, _max_rows: usize) -> Vec<Line<'static>> {
    let Some(report) = report else {
        return vec![Line::from(vec![
            Span::styled(
                "BLOCKED ",
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            ),
            Span::styled("session", Style::default().fg(theme::TEXT)),
            Span::styled("  No session selected", Style::default().fg(theme::MUTED)),
        ])];
    };

    let mut lines = Vec::new();
    for group in readiness_groups() {
        lines.push(Line::from(Span::styled(
            group.title,
            Style::default()
                .fg(group.color)
                .add_modifier(Modifier::BOLD),
        )));
        let checks = grouped_checks(report, group.names);
        lines.extend(checks.into_iter().map(readiness_check_line));
    }
    lines
}

fn preflight_readiness_lines(
    report: Option<&VerificationReport>,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let Some(report) = report else {
        return readiness_lines(None, max_rows);
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
        .map(readiness_check_line)
        .collect::<Vec<_>>();
    let remaining = report.checks.len().saturating_sub(shown);
    if remaining > 0 {
        lines.push(Line::from(Span::styled(
            format!("  {remaining} more verifier checks"),
            Style::default().fg(theme::MUTED),
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
            color: theme::GREEN,
            names: &["target_support", "target_command"],
        },
        ReadinessGroup {
            title: "Workspace Restore",
            color: theme::PURPLE,
            names: &["continuation_level", "package_import", "workspace_restore"],
        },
        ReadinessGroup {
            title: "Source Health",
            color: theme::BLUE,
            names: &["source_health", "token_budget", "rewind_exists"],
        },
        ReadinessGroup {
            title: "Capsule Health",
            color: theme::GOLD,
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
            color: theme::CYAN,
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

fn readiness_check_line(check: &crate::core::model::VerificationCheck) -> Line<'static> {
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
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", check.detail),
            Style::default().fg(theme::MUTED),
        ),
    ])
}

fn verification_color(status: VerificationStatus) -> Color {
    match status {
        VerificationStatus::Pass => theme::GREEN,
        VerificationStatus::Warn => theme::GOLD,
        VerificationStatus::Fail => theme::RED,
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
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled("CLI: ", Style::default().fg(theme::BLUE)),
                Span::raw(session.cli.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Session: ", Style::default().fg(theme::BLUE)),
                Span::raw(&session.id),
            ]),
            Line::from(vec![
                Span::styled("cwd: ", Style::default().fg(theme::BLUE)),
                Span::raw(&session.cwd),
            ]),
            Line::raw(""),
            Line::from(Span::styled(
                "Will run",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                app.original_resume_display_command().unwrap_or_default(),
                Style::default().fg(theme::CYAN),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Copy wrapper",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                app.original_open_command().unwrap_or_default(),
                Style::default().fg(theme::MUTED),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Action: Moonbox hands this terminal to the original CLI.",
                Style::default().fg(theme::MUTED),
            )),
            Line::from(Span::styled(
                "enter hand off   y copy wrapper command   Esc close",
                Style::default().fg(theme::MUTED),
            )),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                "No session selected",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Adjust filter or search, then try again.",
                Style::default().fg(theme::MUTED),
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
    let color = if focused { theme::GOLD } else { theme::BORDER };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .style(Style::default().fg(theme::TEXT))
        .padding(Padding::horizontal(1))
}

fn dynamic_panel_block(title: String, focused: bool) -> Block<'static> {
    let color = if focused { theme::GOLD } else { theme::BORDER };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .style(Style::default().fg(theme::TEXT))
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
            .fg(theme::BLUE)
            .add_modifier(Modifier::BOLD),
    )
}

fn txt(label: &'static str) -> Span<'static> {
    Span::styled(label, Style::default().fg(theme::MUTED))
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
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        app::App,
        core::model::{
            CliTool, SessionStatus, SourceProvenance, TimelineAttachment, TimelineEvent,
            TimelineKind, VerificationStatus,
        },
    };

    fn render_text(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| render(frame, app)).expect("draw");
        format!("{}", terminal.backend())
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

        assert_eq!(display_width(active), 5);
        assert_eq!(display_width(inactive), 5);
        assert_eq!(display_width(active_ai_group), 5);
        assert_eq!(display_width(inactive_ai_group), 5);
    }

    #[test]
    fn neutral_status_line_is_auxiliary_not_selected() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let line = status_line(&app);
        let message = &line.spans[1];

        assert_eq!(message.style.fg, Some(theme::MUTED));
        assert!(!message.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn unselected_session_titles_are_muted() {
        assert_eq!(session_title_style(false).fg, Some(theme::MUTED));
        assert_eq!(session_title_style(true).fg, Some(theme::TEXT));
        assert!(
            session_title_style(true)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn header_tokens_do_not_show_fake_budget() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 40);

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
    fn header_brand_degrades_on_narrow_width() {
        let narrow = header_title_spans(80)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();
        let wide = header_title_spans(140)
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();

        assert_eq!(narrow, " MOONBOX ");
        assert_eq!(wide, " MOONBOX 月光宝盒");
    }

    #[test]
    fn header_collapses_preflight_signals() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
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
            theme::CONFIDENCE_STRONG
        );
        assert_eq!(
            PreflightConfidence::Medium.color(),
            theme::CONFIDENCE_MEDIUM
        );
        assert_eq!(PreflightConfidence::Weak.color(), theme::CONFIDENCE_WEAK);
    }

    #[test]
    fn session_list_secondary_uses_relative_time_with_branch() {
        let now = parse_session_timestamp("2026-06-07T13:34:00+08:00").expect("now");
        let session = test_session("2026-06-07T13:33:44+08:00", Some("dev"));

        assert_eq!(
            session_list_secondary_at(&session, now),
            "    0 events  ·  16s ago  ·  dev"
        );
    }

    #[test]
    fn session_inventory_metric_omits_unknown_tokens() {
        let mut session = test_session("2026-06-07T13:33:44+08:00", Some("dev"));
        session.event_count = 24;

        assert_eq!(session_inventory_metric(&session), "24 events");

        session.token_count = Some(42_000);
        assert_eq!(session_inventory_metric(&session), "24 events · 42K");
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
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "148 events");
        assert_screen_contains(&screen, "Portrait");
        assert_screen_contains(&screen, "user 1 / assistant 1 / tool");
        assert_screen_contains(&screen, "4 / rewind 1");
        assert!(!screen.contains("shape U"), "{screen}");
    }

    #[test]
    fn source_badges_share_one_color_mapping() {
        assert_eq!(source_tool_color(CliTool::Codex), theme::BLUE);
        assert_eq!(source_tool_color(CliTool::Claude), theme::PURPLE);
        assert_eq!(source_tool_color(CliTool::Hermes), theme::ORANGE);
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
            parse_skip_count: 0,
            provider_metadata: None,
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

        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Skill Picker");
        assert_screen_contains(&screen, "Choose compiler skill");
        assert_screen_contains(&screen, "engineering-handoff");
        assert_screen_contains(&screen, "stars:");
        assert_screen_contains(&screen, "n/a");
        assert_screen_contains(&screen, "https://github.com/Gunsio/moonbox");
        assert_screen_contains(&screen, "j/k choose");
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
        assert_eq!(rewind.style.fg, Some(theme::ROLE_REWIND));
        assert_eq!(target.style.fg, Some(theme::ROLE_TARGET));
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
        assert_eq!(timeline_group_accent(theme::BLUE, false), theme::BLUE);
        assert_eq!(timeline_group_accent(theme::GOLD, false), theme::GOLD);
        assert_eq!(timeline_group_accent(theme::BLUE, true), theme::ROLE_REWIND);

        let selected_user_prefix = timeline_prefix_style(true, theme::BLUE);
        assert_eq!(selected_user_prefix.fg, Some(theme::BLUE));
        assert!(selected_user_prefix.add_modifier.contains(Modifier::BOLD));

        let selected_ai_prefix = timeline_prefix_style(true, theme::GOLD);
        assert_eq!(selected_ai_prefix.fg, Some(theme::GOLD));
        assert!(selected_ai_prefix.add_modifier.contains(Modifier::BOLD));

        let active_cursor_marker = timeline_marker_style(true, true, false);
        assert_eq!(active_cursor_marker.fg, Some(theme::CYAN));
        assert!(active_cursor_marker.add_modifier.contains(Modifier::BOLD));

        let inactive_rewind_marker = timeline_marker_style(false, false, true);
        assert_eq!(inactive_rewind_marker.fg, Some(theme::ROLE_REWIND));
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
    fn launch_review_renders_explicit_handoff_action() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;
        let screen = render_text(&app, 120, 48);

        assert_screen_contains(&screen, "Handoff Review");
        assert_screen_contains(&screen, "Capsule Review");
        assert_screen_contains(&screen, "handoff");
        assert_screen_contains(&screen, "Path:");
        assert_screen_contains(&screen, "source Codex codex-cxcp-des...");
        assert_screen_contains(&screen, "-> rewind evt-091 -> target Hermes");
        assert_screen_contains(&screen, "Portrait:");
        assert_screen_contains(&screen, "user 1 / assistant 1 / tool 4 / rewind 1");
        assert_screen_contains(&screen, "Target receives");
        assert_screen_contains(&screen, "Prompt");
        assert_screen_contains(&screen, "Draft Work Capsule");
        assert_screen_contains(&screen, "Goal");
        assert_screen_contains(&screen, "Readiness");
        assert_screen_contains(&screen, "PASS");
        assert_screen_contains(&screen, "target_support");
        assert_screen_contains(&screen, "moonbox launch --execute");
        assert_screen_contains(&screen, "enter Handoff");
    }

    #[test]
    fn launch_review_scrolls_to_exact_target_prompt() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;
        app.modal_scroll = 38;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Prompt argument");
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
