use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

use crate::{
    app::{App, Focus},
    core::model::{SessionStatus, TimelineKind},
};

use super::theme;

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme::TEXT)),
        area,
    );

    let root = centered(area, 98, 96);
    let header_height = if root.width < 120 { 5 } else { 3 };
    let body_min = if root.height < 32 { 8 } else { 18 };
    let branch_height = if root.height < 32 { 3 } else { 4 };
    let command_height = if root.width < 120 { 4 } else { 3 };
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
        Span::raw("Source: "),
        Span::styled(
            app.data.source.to_string(),
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   Target: "),
        Span::styled(
            app.data.target.to_string(),
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
    let items = app.data.sessions.iter().map(|session| {
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
    });

    let mut state = ListState::default();
    state.select(Some(app.selected_session));

    let list = List::new(items)
        .block(panel_block(" Sessions ", app.focus == Focus::Sessions))
        .highlight_symbol("▸ ")
        .highlight_style(
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    for (idx, event) in app.data.timeline.iter().enumerate() {
        let selected = idx == app.selected_event;
        let (label, color) = match event.kind {
            TimelineKind::User => ("USER", theme::BLUE),
            TimelineKind::Assistant => ("ASSISTANT", theme::GOLD),
            TimelineKind::Tool => ("TOOL", theme::MUTED),
            TimelineKind::Compact => ("COMPACT", theme::CYAN),
            TimelineKind::Error => ("ERROR", theme::RED),
            TimelineKind::GitDiff => ("GIT DIFF", theme::GREEN),
            TimelineKind::RewindPoint => ("REWIND", theme::GOLD),
        };
        let prefix = if selected { "●" } else { "│" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{prefix} {} ", event.time),
                Style::default().fg(color),
            ),
            Span::styled(
                format!(" {label} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", event.title),
                Style::default().fg(theme::TEXT),
            ),
        ]));

        let detail_style = if event.kind == TimelineKind::RewindPoint {
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::MUTED)
        };
        lines.push(Line::from(vec![
            Span::raw("   "),
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
    let source = format!("{} / {}", capsule.source_cli, capsule.source_session);
    let target = format!("{} / {}", capsule.target_cli, capsule.target_branch);
    let mut lines = vec![
        label_line("Goal", &capsule.goal, theme::BLUE),
        Line::raw(""),
        label_line("State", &capsule.state, theme::GOLD),
        Line::raw(""),
        label_line("Source", &source, theme::CYAN),
        label_line("Rewind", &capsule.rewind_point, theme::GOLD),
        label_line("Target", &target, theme::GREEN),
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
    let mut spans = vec![Span::styled(" * ", Style::default().fg(theme::MUTED))];
    for (idx, node) in app.data.branches.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" ── ", Style::default().fg(theme::BORDER)));
        }
        let style = if node.active {
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(theme::BLUE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
        };
        spans.push(Span::styled(format!(" {} ", node.label), style));
    }
    let active = app
        .data
        .branches
        .iter()
        .find(|node| node.active)
        .map(|node| node.detail.as_str())
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
        vec![Line::from(vec![
            Span::styled(
                ":",
                Style::default()
                    .fg(theme::GOLD)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&app.command_input, Style::default().fg(theme::TEXT)),
        ])]
    } else if area.width < 120 {
        vec![
            Line::from(vec![
                key("j/k"),
                txt(" Nav  "),
                key("gg/G"),
                txt(" Jump  "),
                key("/"),
                txt(" Search  "),
                key("space"),
                txt(" Rewind  "),
                key("c"),
                txt(" Compile  "),
                key("v"),
                txt(" Verify"),
            ]),
            Line::from(vec![
                key("d"),
                txt(" Diff  "),
                key("s"),
                txt(" Skill  "),
                key("enter"),
                txt(" Launch  "),
                key(":"),
                txt(" Cmd  "),
                key("?"),
                txt(" Help  "),
                key("q"),
                txt(" Quit"),
            ]),
        ]
    } else {
        vec![Line::from(vec![
            key("j/k"),
            txt(" Nav  "),
            key("gg/G"),
            txt(" Jump  "),
            key("/"),
            txt(" Search  "),
            key("space"),
            txt(" Rewind  "),
            key("c"),
            txt(" Compile  "),
            key("v"),
            txt(" Verify  "),
            key("d"),
            txt(" Diff  "),
            key("s"),
            txt(" Skill  "),
            key("enter"),
            txt(" Launch  "),
            key(":"),
            txt(" Cmd  "),
            key("?"),
            txt(" Help  "),
            key("q"),
            txt(" Quit"),
        ])]
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
        Line::raw("space           set rewind point"),
        Line::raw("c, v, d, s      compile, verify, diff, switch skill"),
        Line::raw("enter           launch target command preview"),
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
    let area = centered(root, 62, 34);
    frame.render_widget(Clear, area);
    let capsule = &app.data.capsule;
    let lines = vec![
        Line::from(Span::styled(
            "Target launch preview",
            Style::default()
                .fg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Target: ", Style::default().fg(theme::BLUE)),
            Span::raw(capsule.target_cli.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Branch: ", Style::default().fg(theme::BLUE)),
            Span::raw(&capsule.target_branch),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "moonbox launch --target hermes --capsule ~/.moonbox/capsules/evt-091.json",
            Style::default().fg(theme::CYAN),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(theme::MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Launch ", true))
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
