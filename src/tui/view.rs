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
    app::{App, Focus},
    core::model::{
        CliTool, LaunchValidationState, SessionStatus, SourceProvenance, TimelineEvent,
        TimelineKind, VerificationReport, VerificationStatus,
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
    let body_min = if root.height < 32 { 8 } else { 18 };
    let branch_height = if root.height < 32 { 3 } else { 4 };
    let command_height = if root.width < 120 { 5 } else { 3 };
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
    if app.show_diff {
        render_diff(frame, root, app);
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
            Span::styled("esc", Style::default().fg(theme::BLUE)),
            Span::raw(" cancel"),
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
    let verify = if app.verify_passed {
        "PASS"
    } else {
        "NEEDS REVIEW"
    };
    let verify_color = if app.verify_passed {
        theme::GREEN
    } else {
        theme::ORANGE
    };

    let title = Line::from(vec![
        Span::styled(
            " MOONBOX ",
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("月光宝盒", Style::default().fg(theme::MUTED)),
    ]);
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
        Span::raw("   Compiler: "),
        Span::styled(
            app.compile_status,
            Style::default()
                .fg(theme::GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Skill: "),
        Span::styled(&app.data.capsule.compiler, Style::default().fg(theme::CYAN)),
    ]);
    let token_budget = app
        .current_session()
        .map(|session| format!("{} / 100K", format_token_count(session.token_count)))
        .unwrap_or_else(|| "- / 100K".into());
    let budget = Line::from(vec![
        Span::raw("Tokens: "),
        Span::styled(
            token_budget,
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Doctor: "),
        Span::styled(
            app.doctor_report.status.to_string(),
            Style::default()
                .fg(verification_color(app.doctor_report.status))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Verify: "),
        Span::styled(
            verify,
            Style::default()
                .fg(verify_color)
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

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
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
                let status = match session.status {
                    SessionStatus::Healthy => Span::raw(" "),
                    SessionStatus::Warning => Span::styled("▲", Style::default().fg(theme::GOLD)),
                    SessionStatus::Failed => Span::styled(
                        "!",
                        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
                    ),
                };
                ListItem::new(vec![
                    Line::from(vec![
                        status,
                        Span::raw(" "),
                        Span::styled(source_pill(session.cli), source_tool_style(session.cli)),
                        Span::raw("  "),
                        Span::styled(&session.title, Style::default().fg(theme::TEXT)),
                    ]),
                    Line::from(vec![Span::styled(
                        session_list_secondary(session),
                        Style::default().fg(theme::MUTED),
                    )]),
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
        .highlight_symbol("▸ ")
        .highlight_style(
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
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

fn session_list_secondary(session: &crate::core::model::SessionSummary) -> String {
    match session
        .branch
        .as_deref()
        .filter(|branch| !branch.is_empty())
    {
        Some(branch) => format!("      {}  ·  {}", session.updated, branch),
        None => format!("      {}", session.updated),
    }
}

fn source_pill(tool: CliTool) -> &'static str {
    match tool {
        CliTool::Codex => "Cdx",
        CliTool::Claude => "Clu",
        CliTool::Hermes => "Hms",
    }
}

fn source_tool_style(tool: CliTool) -> Style {
    let color = match tool {
        CliTool::Codex => theme::BLUE,
        CliTool::Claude => theme::PURPLE,
        CliTool::Hermes => theme::ORANGE,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
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
    let visible_events = visible_timeline_events(app);
    let selected_visible = selected_visible_timeline_position(&visible_events, app.selected_event);
    let mut lines = Vec::new();
    for (visible_idx, (_, event)) in visible_events.iter().enumerate() {
        let selected = visible_idx == selected_visible;
        let active = selected && app.focus == Focus::Timeline;
        let is_rewind = event.id == app.rewind_event_id;
        let (label, color) = if is_rewind {
            ("REWIND", theme::GOLD)
        } else {
            match event.kind {
                TimelineKind::User => ("USER", theme::BLUE),
                TimelineKind::Assistant => ("ASSISTANT", theme::GOLD),
                TimelineKind::Tool => ("TOOL", theme::MUTED),
                TimelineKind::Compact => ("COMPACT", theme::CYAN),
                TimelineKind::Error => ("ERROR", theme::RED),
                TimelineKind::GitDiff => ("GIT DIFF", theme::GREEN),
                TimelineKind::RewindPoint => ("REWIND", theme::GOLD),
            }
        };
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
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        let label_style = if active {
            Style::default()
                .fg(Color::Black)
                .bg(theme::GOLD)
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
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} {} ", event.time), time_style),
            Span::styled(format!(" {label} "), label_style),
            Span::styled(format!(" {}", event.title), title_style),
        ]));

        let detail_style = if active {
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD)
        } else if is_rewind || event.kind == TimelineKind::RewindPoint {
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::MUTED)
        };
        lines.push(Line::from(vec![
            Span::styled(
                if active { "  └ " } else { "   " },
                Style::default().fg(if active { theme::GOLD } else { theme::MUTED }),
            ),
            Span::styled(&event.detail, detail_style),
        ]));
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
            .scroll((timeline_scroll(app, area), 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn timeline_scroll(app: &App, area: Rect) -> u16 {
    let viewport = area.height.saturating_sub(2).max(1);
    let visible_events = visible_timeline_events(app);
    let selected_visible = selected_visible_timeline_position(&visible_events, app.selected_event);
    let selected_line = selected_visible.saturating_mul(3) as u16;
    selected_line.saturating_sub(viewport / 2)
}

fn visible_timeline_events(app: &App) -> Vec<(usize, &TimelineEvent)> {
    app.data
        .timeline
        .iter()
        .enumerate()
        .filter(|(_, event)| event.id == app.rewind_event_id || event.kind != TimelineKind::Tool)
        .collect()
}

fn selected_visible_timeline_position(
    visible_events: &[(usize, &TimelineEvent)],
    selected_event: usize,
) -> usize {
    visible_events
        .iter()
        .position(|(index, _)| *index == selected_event)
        .or_else(|| {
            visible_events
                .iter()
                .enumerate()
                .rev()
                .find(|(_, (index, _))| *index <= selected_event)
                .map(|(position, _)| position)
        })
        .unwrap_or(0)
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

    lines.extend([
        Line::raw(""),
        Line::from(Span::styled(
            capsule_preview_title(&capsule.state),
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            capsule_preview_note(&capsule.state),
            Style::default().fg(if capsule.state.contains("builtin") {
                theme::GOLD
            } else {
                theme::MUTED
            }),
        )),
        Line::raw(""),
        label_line("Goal", &capsule.goal, theme::BLUE),
        Line::raw(""),
        label_line("State", &capsule.state, theme::GOLD),
        Line::raw(""),
        label_line("Rewind", &capsule.rewind_point, theme::GOLD),
        Line::raw(""),
        Line::from(Span::styled(
            "Decisions",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        )),
    ]);

    for decision in &capsule.decisions {
        lines.push(Line::from(vec![
            Span::raw("  • "),
            Span::styled(decision, Style::default().fg(theme::TEXT)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Todo",
        Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::BOLD),
    )));
    for todo in &capsule.todo {
        let mark = if todo.done { "[x]" } else { "[ ]" };
        let color = if todo.done { theme::GREEN } else { theme::TEXT };
        lines.push(Line::from(vec![
            Span::styled(format!("  {mark} "), Style::default().fg(color)),
            Span::styled(&todo.text, Style::default().fg(color)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Evidence",
        Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::BOLD),
    )));
    for evidence in &capsule.evidence {
        lines.push(Line::from(vec![
            Span::styled("  > ", Style::default().fg(theme::MUTED)),
            Span::styled(evidence, Style::default().fg(theme::MUTED)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Risks",
        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
    )));
    for risk in &capsule.risks {
        lines.push(Line::from(vec![
            Span::styled("  ! ", Style::default().fg(theme::RED)),
            Span::styled(risk, Style::default().fg(theme::GOLD)),
        ]));
    }

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

fn capsule_preview_note(state: &str) -> &'static str {
    if state.contains("builtin") {
        "  Draft guidance. Real fields: session, cwd, title, selected rewind, source health."
    } else {
        "  Compiler-generated handoff context. Verify before launch."
    }
}

fn capsule_preview_title(state: &str) -> &'static str {
    if state.contains("builtin") {
        "Draft Work Capsule Preview"
    } else {
        "Work Capsule Preview"
    }
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
        metadata_line(
            "Updated",
            &session.updated,
            Style::default().fg(theme::BLUE),
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

fn metadata_line(label: &'static str, value: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(theme::MUTED)),
        Span::styled(value.to_owned(), style),
    ])
}

fn render_branch_tree(frame: &mut Frame, area: Rect, app: &App) {
    let original = app
        .current_session()
        .map(|session| format!("original/{}", session.id))
        .unwrap_or_else(|| "original/no-session".into());
    let rewind = format!("rewind/{}", app.rewind_event_id);
    let target = format!("handoff/{}", app.data.target.id());
    let rewind_detail = app
        .data
        .timeline
        .iter()
        .find(|event| event.id == app.rewind_event_id)
        .map(|event| event.title.as_str())
        .unwrap_or("selected rewind point");
    let nodes = [
        (original, false, "original session, read-only"),
        (rewind, false, rewind_detail),
        (target, true, "compiled by engineering-handoff"),
    ];

    let mut spans = vec![Span::styled(" * ", Style::default().fg(theme::MUTED))];
    for (idx, (label, active, _)) in nodes.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" ── ", Style::default().fg(theme::BORDER)));
        }
        let style = if *active {
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
        };
        let label = if *active {
            format!(" > {label} ")
        } else {
            format!(" {label} ")
        };
        spans.push(Span::styled(label, style));
    }
    let active = nodes
        .iter()
        .find(|(_, active, _)| *active)
        .map(|(_, _, detail)| *detail)
        .unwrap_or("no active branch");
    let lines = if area.height < 4 {
        vec![Line::from(spans)]
    } else {
        vec![
            Line::from(spans),
            Line::from(vec![
                Span::styled("   active: ", Style::default().fg(theme::MUTED)),
                Span::styled(active, Style::default().fg(theme::CYAN)),
            ]),
        ]
    };
    frame.render_widget(
        Paragraph::new(lines).block(panel_block(" Branch Tree ", app.focus == Focus::Branches)),
        area,
    );
}

fn render_command_bar(frame: &mut Frame, area: Rect, app: &App) {
    let lines = if app.command_mode {
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
            return vec![
                ("y", "Copy"),
                ("enter", "Disabled"),
                ("PgUp/Dn", "Scroll"),
                ("Esc", "Close"),
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
    if app.show_doctor {
        return vec![
            ("r", "Refresh"),
            ("y", "Copy JSON"),
            ("j/k", "Scroll"),
            ("Esc", "Close"),
        ];
    }
    if app.show_diff || app.show_help {
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
            ("a", "Clear"),
            ("o", "Original"),
            ("enter", "Open"),
            ("t", "Target"),
            ("tab", "Next"),
        ],
        Focus::Timeline => vec![
            ("j/k", "Events"),
            ("gg/G", "Jump"),
            ("space", "Rewind"),
            ("c", "Compile"),
            ("d", "Diff"),
            ("tab", "Next"),
            (":", "Cmd"),
            ("q", "Quit"),
        ],
        Focus::Capsule => vec![
            ("j/k", "Scroll"),
            ("gg/G", "Top/Bottom"),
            ("c", "Compile"),
            ("v", "Verify"),
            ("s", "Skill"),
            ("d", "Diff"),
            ("tab", "Next"),
            (":", "Cmd"),
            ("q", "Quit"),
        ],
        Focus::Branches => vec![
            ("enter", "Open"),
            ("t", "Target"),
            ("o", "Original"),
            ("space", "Rewind"),
            ("D", "Doctor"),
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
    let color = if app.status_message.contains("cancelled")
        || app.status_message.contains("No session")
        || app.status_message.contains("Unknown")
        || app.status_message.contains("NEEDS REVIEW")
    {
        theme::ORANGE
    } else if app.status_message.contains("PASS")
        || app.status_message.contains("saved")
        || app.status_message.contains("compiled")
        || app.status_message.contains("cleared")
    {
        theme::GREEN
    } else {
        theme::CYAN
    };

    Line::from(vec![
        Span::styled("Status ", Style::default().fg(theme::MUTED)),
        Span::styled(
            &app.status_message,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
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
        Line::raw("/text           filter sessions by text"),
        Line::raw("o               open original session with original CLI"),
        Line::raw("enter           open selected session with original CLI"),
        Line::raw("t               choose target and review handoff"),
        Line::raw("D               open environment doctor"),
        Line::raw("[ / ]           previous / next session source filter"),
        Line::raw("space           set rewind point"),
        Line::raw("c, v, d, s      compile, verify, diff, switch skill"),
        Line::raw(":               command mode"),
        Line::raw("q / Esc         close or quit"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Help ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_doctor(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 72, 72);
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Environment doctor",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                app.doctor_report.status.to_string(),
                Style::default()
                    .fg(verification_color(app.doctor_report.status))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Ready: ", Style::default().fg(theme::BLUE)),
            Span::styled(
                app.doctor_report.ready.to_string(),
                Style::default().fg(if app.doctor_report.ready {
                    theme::GREEN
                } else {
                    theme::RED
                }),
            ),
        ]),
        Line::raw(""),
    ];

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
            "r refresh   y copy JSON   Esc close",
            Style::default().fg(theme::MUTED),
        )),
    ]);

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Doctor ", true))
            .scroll((app.modal_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_launch(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 76, 78);
    frame.render_widget(Clear, area);
    let session = app
        .current_session()
        .map(|session| format!("{} / {}", session.cli, session.id))
        .unwrap_or_else(|| "No session selected".into());
    let target_branch = app.launch_branch();
    let pending_validation = app.validate_launch_for_target(app.pending_target);
    let pending_report = app.launch_verification_for_target(app.pending_target);
    if app.launch_review {
        let mut lines = vec![
            Line::from(Span::styled(
                "Review target handoff",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Action: ", Style::default().fg(theme::BLUE)),
                Span::styled(
                    "target handoff",
                    Style::default()
                        .fg(theme::CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Session: ", Style::default().fg(theme::BLUE)),
                Span::raw(session),
            ]),
            Line::from(vec![
                Span::styled("Target: ", Style::default().fg(theme::BLUE)),
                Span::raw(app.pending_target.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(theme::BLUE)),
                Span::raw(target_branch),
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
                "Readiness details",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        lines.extend(readiness_lines(pending_report.as_ref(), 6));
        lines.extend([
            Line::raw(""),
            Line::from(Span::styled(
                app.launch_command(),
                Style::default().fg(theme::CYAN),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "enter launch after restore   y copy execute command   Esc close",
                Style::default().fg(theme::MUTED),
            )),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block(" Launch Review ", true))
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
            "Readiness details",
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

fn readiness_lines(report: Option<&VerificationReport>, max_rows: usize) -> Vec<Line<'static>> {
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

    let mut checks = report
        .checks
        .iter()
        .filter(|check| check.status == VerificationStatus::Fail)
        .chain(
            report
                .checks
                .iter()
                .filter(|check| check.status == VerificationStatus::Warn),
        )
        .collect::<Vec<_>>();

    if checks.is_empty() {
        for name in [
            "capsule_source",
            "target_cli",
            "rewind_exists",
            "handoff_context",
            "target_support",
        ] {
            if let Some(check) = report.checks.iter().find(|check| check.name == name) {
                checks.push(check);
            }
        }
    }

    let overflow = checks.len().saturating_sub(max_rows);
    checks.truncate(max_rows);
    let mut lines = checks
        .into_iter()
        .map(|check| {
            let color = verification_color(check.status);
            Line::from(vec![
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
        })
        .collect::<Vec<_>>();

    if overflow > 0 {
        lines.push(Line::from(Span::styled(
            format!("... {overflow} more readiness signal(s)"),
            Style::default().fg(theme::MUTED),
        )));
    }
    lines
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

fn render_diff(frame: &mut Frame, root: Rect, app: &App) {
    let area = modal_area(root, 64, 42);
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "Capsule diff preview",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "+ canonical timeline schema",
            Style::default().fg(theme::GREEN),
        )),
        Line::from(Span::styled(
            "+ work capsule schema",
            Style::default().fg(theme::GREEN),
        )),
        Line::from(Span::styled(
            "+ target launcher new-branch mode",
            Style::default().fg(theme::GREEN),
        )),
        Line::from(Span::styled(
            "- raw session resume",
            Style::default().fg(theme::RED),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(theme::MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Diff ", true))
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

fn label_line<'a>(label: &'a str, value: &'a str, color: ratatui::style::Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(value, Style::default().fg(theme::TEXT)),
    ])
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
        core::model::{CliTool, VerificationStatus},
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
    fn main_timeline_hides_low_signal_tool_events() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "ASSISTANT");
        assert_screen_contains(&screen, "REWIND");
        assert!(!screen.contains("Tool: rg"), "{screen}");
    }

    #[test]
    fn builtin_capsule_preview_is_labeled_as_draft_not_mock_project_todo() {
        let app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        let screen = render_text(&app, 140, 40);

        assert_screen_contains(&screen, "Real Session Metadata");
        assert_screen_contains(&screen, "Draft Work Capsule Preview");
        assert_screen_contains(&screen, "Real fields:");
        assert_screen_contains(&screen, "draft_from_builtin_compiler");
        assert!(!screen.contains("Define canonical timeline"), "{screen}");
    }

    #[test]
    fn doctor_overlay_renders_diagnostics_and_actions() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_doctor = true;
        app.doctor_report.status = VerificationStatus::Pass;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Doctor");
        assert_screen_contains(&screen, "Environment doctor");
        assert_screen_contains(&screen, "source_codex_adapter");
        assert_screen_contains(&screen, "fixtures/adapters/codex");
        assert_screen_contains(&screen, "Copy JSON");
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
        assert_screen_contains(&screen, "Readiness details");
        assert_screen_contains(&screen, "FAIL");
        assert_screen_contains(&screen, "target_support");
        assert_screen_contains(&screen, "raw resume is known failed");
        assert_screen_contains(&screen, "enter/y blocked");
    }

    #[test]
    fn launch_review_renders_explicit_handoff_action() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes).expect("app");
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "Launch Review");
        assert_screen_contains(&screen, "target handoff");
        assert_screen_contains(&screen, "Readiness details");
        assert_screen_contains(&screen, "PASS");
        assert_screen_contains(&screen, "target_cli");
        assert_screen_contains(&screen, "moonbox launch --execute");
        assert_screen_contains(&screen, "enter launch after restore");
    }

    #[test]
    fn launch_overlay_renders_warning_readiness_signal() {
        let mut app = App::new(CliTool::Codex, CliTool::Codex).expect("app");
        app.show_launch = true;
        app.pending_target = CliTool::Codex;
        let screen = render_text(&app, 120, 36);

        assert_screen_contains(&screen, "WARN");
        assert_screen_contains(&screen, "Readiness details");
        assert_screen_contains(&screen, "target_support");
        assert_screen_contains(&screen, "Same-CLI handoff");
        assert_screen_contains(&screen, "enter review");
    }
}
