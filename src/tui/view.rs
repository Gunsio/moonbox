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
    core::model::{CliTool, SessionStatus, TimelineKind},
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
        render_help(frame, root);
    }
    if app.show_launch {
        render_launch(frame, root, app);
    }
    if app.show_open_original {
        render_open_original(frame, root, app);
    }
    if app.show_diff {
        render_diff(frame, root);
    }
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
    let budget = Line::from(vec![
        Span::raw("Tokens: "),
        Span::styled(
            "42K / 100K",
            Style::default()
                .fg(theme::GOLD)
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
        visible
            .iter()
            .map(|index| {
                let session = &app.data.sessions[*index];
                let status = match session.status {
                    SessionStatus::Healthy => Span::styled("●", Style::default().fg(theme::GREEN)),
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
                        Span::styled(
                            format!("[{}] ", session.cli),
                            Style::default()
                                .fg(theme::CYAN)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            &session.title,
                            Style::default()
                                .fg(theme::TEXT)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![Span::styled(
                        format!("  {}", session.cwd),
                        Style::default().fg(theme::MUTED),
                    )]),
                    Line::from(vec![
                        Span::styled(
                            format!("  {}  ", session.updated),
                            Style::default().fg(theme::BLUE),
                        ),
                        Span::styled(
                            format!("{} events", session.event_count),
                            Style::default().fg(theme::MUTED),
                        ),
                    ]),
                ])
            })
            .collect()
    };

    let mut state = ListState::default();
    let selected = visible
        .iter()
        .position(|index| *index == app.selected_session)
        .unwrap_or(0);
    state.select((!visible.is_empty()).then_some(selected));

    let title = if app.search_query.is_empty() {
        format!(" Sessions · {} ", app.session_filter.label())
    } else if area.width < 28 {
        format!(" Sessions /{} ", app.search_query)
    } else {
        format!(" Sessions · {} ", filter_label(app))
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

fn filter_label(app: &App) -> String {
    if app.search_query.is_empty() {
        app.session_filter.label().to_string()
    } else {
        format!("{} · /{}", app.session_filter.label(), app.search_query)
    }
}

fn render_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    for (idx, event) in app.data.timeline.iter().enumerate() {
        let selected = idx == app.selected_event;
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

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Timeline ", app.focus == Focus::Timeline))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_capsule(frame: &mut Frame, area: Rect, app: &App) {
    let capsule = &app.data.capsule;
    let mut lines = vec![
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
    ];

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
                " Work Capsule Preview ",
                app.focus == Focus::Capsule,
            ))
            .wrap(Wrap { trim: true }),
        area,
    );
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
        return vec![
            ("j/k", "Target"),
            ("enter", "Save"),
            ("Esc", "Cancel"),
            ("q", "Cancel"),
        ];
    }
    if app.show_open_original || app.show_diff || app.show_help {
        return vec![("Esc", "Close"), ("q", "Close")];
    }

    match app.focus {
        Focus::Sessions => vec![
            ("j/k", "Sessions"),
            ("gg/G", "Jump"),
            ("/", "Search"),
            ("[ ]", "Source"),
            ("a", "Clear"),
            ("o", "Original"),
            ("enter", "Target"),
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
            ("c", "Compile"),
            ("v", "Verify"),
            ("s", "Skill"),
            ("d", "Diff"),
            ("space", "Rewind"),
            ("tab", "Next"),
            (":", "Cmd"),
            ("q", "Quit"),
        ],
        Focus::Branches => vec![
            ("enter", "Target"),
            ("o", "Original"),
            ("space", "Rewind"),
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

fn render_help(frame: &mut Frame, root: Rect) {
    let area = centered(root, 52, 48);
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
        Line::raw("[ / ]           previous / next session source filter"),
        Line::raw("space           set rewind point"),
        Line::raw("c, v, d, s      compile, verify, diff, switch skill"),
        Line::raw("enter           choose target and show handoff command"),
        Line::raw(":               command mode"),
        Line::raw("q / Esc         close or quit"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Help ", true))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_launch(frame: &mut Frame, root: Rect, app: &App) {
    let area = centered(root, 76, 78);
    frame.render_widget(Clear, area);
    let session = app
        .current_session()
        .map(|session| format!("{} / {}", session.cli, session.id))
        .unwrap_or_else(|| "No session selected".into());
    let mut target_lines = Vec::new();
    for target in CliTool::ALL {
        let selected = target == app.pending_target;
        let style = if selected {
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(theme::BLUE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
        };
        let cursor = if selected { ">" } else { " " };
        let mark = if selected { "[x]" } else { "[ ]" };
        target_lines.push(Line::from(vec![
            Span::styled(format!("{cursor} {mark} {target:<6}"), style),
            Span::styled("  handoff target", Style::default().fg(theme::MUTED)),
        ]));
    }
    let target_branch = format!(
        "moonbox/{}-rewind-{}",
        app.pending_target.id(),
        app.rewind_event_id
    );
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
            Span::styled("Branch: ", Style::default().fg(theme::BLUE)),
            Span::raw(target_branch),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            format!(
                "moonbox launch --target {} --capsule ~/.moonbox/capsules/{}.json",
                app.pending_target.id(),
                app.rewind_event_id
            ),
            Style::default().fg(theme::CYAN),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "j/k choose target   enter confirm   Esc cancel",
            Style::default().fg(theme::MUTED),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Launch ", true))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_open_original(frame: &mut Frame, root: Rect, app: &App) {
    let area = centered(root, 72, 64);
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
                &session.resume_command,
                Style::default().fg(theme::CYAN),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "Original resume only. Handoff uses Launch.",
                Style::default().fg(theme::MUTED),
            )),
            Line::from(Span::styled(
                "Press Esc to close",
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
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_diff(frame: &mut Frame, root: Rect) {
    let area = centered(root, 64, 42);
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
