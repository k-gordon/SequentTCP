//! TUI rendering with `ratatui`.
//!
//! Each screen gets its own draw function.  All rendering reads from
//! the immutable `App` state — no side effects here.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::app::{App, Screen};

// ════════════════════════════════════════════════════════════════════════
// Main dispatch
// ════════════════════════════════════════════════════════════════════════

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title bar
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(f.area());

    draw_title(f, chunks[0], app);

    match app.screen {
        Screen::BoardSelect => draw_board_select(f, chunks[1], app),
        Screen::BoardConfig => draw_board_config(f, chunks[1], app),
        Screen::ServerSettings => draw_server_settings(f, chunks[1], app),
        Screen::I2cSettings => draw_i2c_settings(f, chunks[1], app),
        Screen::Review => draw_review(f, chunks[1], app),
    }

    draw_status(f, chunks[2], app);
}

// ════════════════════════════════════════════════════════════════════════
// Title bar
// ════════════════════════════════════════════════════════════════════════

fn draw_title(f: &mut Frame, area: Rect, app: &App) {
    let step = match app.screen {
        Screen::BoardSelect => "1/5 · Board Selection",
        Screen::BoardConfig => "2/5 · Board Configuration",
        Screen::ServerSettings => "3/5 · Server Settings",
        Screen::I2cSettings => "4/5 · I²C & Logging",
        Screen::Review => "5/5 · Review & Save",
    };
    let title = format!("  Sequent Gateway Configuration — {step}");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title_alignment(Alignment::Left);
    let para = Paragraph::new(title)
        .block(block)
        .style(Style::default().fg(Color::White).bold());
    f.render_widget(para, area);
}

// ════════════════════════════════════════════════════════════════════════
// Status bar
// ════════════════════════════════════════════════════════════════════════

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let para = Paragraph::new(format!("  {}", app.status))
        .block(block)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(para, area);
}

// ════════════════════════════════════════════════════════════════════════
// Screen 1: Board Selection
// ════════════════════════════════════════════════════════════════════════

fn draw_board_select(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    // Left: board list
    let items: Vec<ListItem> = app
        .available
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let selected = app.selected_boards.iter().any(|s| s.slug == b.slug);
            let marker = if selected { "◉" } else { "○" };
            let exp = if b.experimental { " ⚗" } else { "" };
            let cursor = if i == app.board_cursor { "▸ " } else { "  " };

            let style = if i == app.board_cursor {
                Style::default().fg(Color::Cyan).bold()
            } else if selected {
                Style::default().fg(Color::Green)
            } else if b.experimental {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(vec![
                Span::styled(cursor, style),
                Span::styled(format!("{marker} "), style),
                Span::styled(&b.slug, style),
                Span::styled(format!("{exp}"), Style::default().fg(Color::Yellow)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(" Available Boards "),
    );
    f.render_widget(list, chunks[0]);

    // Right: details panel
    let detail = if app.board_cursor < app.available.len() {
        let b = &app.available[app.board_cursor];
        let selected = app.selected_boards.iter().any(|s| s.slug == b.slug);
        let mut lines = vec![
            Line::from(Span::styled(
                &b.display_name,
                Style::default().fg(Color::Cyan).bold(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Protocol: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&b.def.board.protocol, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("I²C Base: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("0x{:02X}", b.def.address.base),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Addr Mode: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&b.def.address.mode, Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Capabilities:",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        if b.capabilities.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none listed)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for cap in b.capabilities.split(", ") {
                lines.push(Line::from(vec![
                    Span::raw("  • "),
                    Span::styled(cap, Style::default().fg(Color::Green)),
                ]));
            }
        }

        lines.push(Line::from(""));
        if b.experimental {
            lines.push(Line::from(Span::styled(
                "⚗ EXPERIMENTAL — not validated",
                Style::default().fg(Color::Yellow),
            )));
        }
        lines.push(Line::from(Span::styled(
            if selected { "✓ SELECTED" } else { "  Not selected" },
            Style::default().fg(if selected { Color::Green } else { Color::DarkGray }),
        )));

        Text::from(lines)
    } else {
        Text::from("No board selected")
    };

    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .title(" Board Details ");
    let detail_para = Paragraph::new(detail).block(detail_block).wrap(Wrap { trim: true });
    f.render_widget(detail_para, chunks[1]);
}

// ════════════════════════════════════════════════════════════════════════
// Screen 2: Board Configuration
// ════════════════════════════════════════════════════════════════════════

fn draw_board_config(f: &mut Frame, area: Rect, app: &App) {
    if app.selected_boards.is_empty() {
        let para = Paragraph::new("No boards selected")
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(para, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Board tabs
            Constraint::Min(8),   // Config fields
        ])
        .split(area);

    // Board tabs
    let tabs: Vec<Span> = app
        .selected_boards
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let style = if i == app.config_board_idx {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            if i == app.config_board_idx {
                Span::styled(format!(" [{}] ", b.slug), style)
            } else {
                Span::styled(format!("  {}  ", b.slug), style)
            }
        })
        .collect();

    let tabs_line = Line::from(tabs);
    let tabs_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .title(format!(
            " Board {}/{} ",
            app.config_board_idx + 1,
            app.selected_boards.len()
        ));
    let tabs_para = Paragraph::new(tabs_line).block(tabs_block);
    f.render_widget(tabs_para, chunks[0]);

    // Config fields for current board
    let board = &app.selected_boards[app.config_board_idx];
    let fields = vec![
        (
            "Stack ID",
            format!("{}", board.stack),
            "I²C stack level [0–7]",
        ),
        (
            "Slave ID",
            format!("{}", board.slave_id),
            "Modbus slave ID [1–247]",
        ),
    ];

    let mut items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, (label, value, hint))| {
            let cursor = if i == app.config_cursor { "▸ " } else { "  " };
            let style = if i == app.config_cursor {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(cursor, style),
                Span::styled(format!("{label}: "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{value}  "), style),
                Span::styled(format!("({hint})"), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    // Continue button
    let btn_style = if app.config_cursor == 2 {
        Style::default().fg(Color::Green).bold()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled(
            if app.config_cursor == 2 { "▸ " } else { "  " },
            btn_style,
        ),
        Span::styled("[ Continue → ]", btn_style),
    ])));

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(format!(" {} — {} ", board.slug, board.display_name)),
    );
    f.render_widget(list, chunks[1]);
}

// ════════════════════════════════════════════════════════════════════════
// Screen 3: Server Settings
// ════════════════════════════════════════════════════════════════════════

fn draw_server_settings(f: &mut Frame, area: Rect, app: &App) {
    draw_field_list(f, area, " Server Settings ", &app.server_fields, app.server_cursor);
}

// ════════════════════════════════════════════════════════════════════════
// Screen 4: I²C & Logging Settings
// ════════════════════════════════════════════════════════════════════════

fn draw_i2c_settings(f: &mut Frame, area: Rect, app: &App) {
    draw_field_list(f, area, " I²C & Logging ", &app.i2c_fields, app.i2c_cursor);
}

// ════════════════════════════════════════════════════════════════════════
// Shared: editable field list
// ════════════════════════════════════════════════════════════════════════

fn draw_field_list(
    f: &mut Frame,
    area: Rect,
    title: &str,
    fields: &[super::app::EditField],
    cursor: usize,
) {
    let mut items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let is_selected = i == cursor;
            let cursor_str = if is_selected { "▸ " } else { "  " };

            let value_style = if field.editing {
                Style::default().fg(Color::Yellow).bold()
            } else if is_selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };

            let display_value = if field.value.is_empty() {
                "(empty)"
            } else {
                &field.value
            };

            let edit_indicator = if field.editing { " ✎" } else { "" };

            ListItem::new(Line::from(vec![
                Span::styled(cursor_str, value_style),
                Span::styled(
                    format!("{}: ", field.label),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(display_value, value_style),
                Span::styled(edit_indicator, Style::default().fg(Color::Yellow)),
            ]))
        })
        .collect();

    // Continue button
    let btn_selected = cursor == fields.len();
    let btn_style = if btn_selected {
        Style::default().fg(Color::Green).bold()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(vec![
        Span::styled(
            if btn_selected { "▸ " } else { "  " },
            btn_style,
        ),
        Span::styled("[ Continue → ]", btn_style),
    ])));

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(title),
    );
    f.render_widget(list, area);
}

// ════════════════════════════════════════════════════════════════════════
// Screen 5: Review & Save
// ════════════════════════════════════════════════════════════════════════

fn draw_review(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Left: TOML preview
    let preview_lines: Vec<Line> = app
        .preview
        .lines()
        .skip(app.review_scroll)
        .map(|line| {
            let style = if line.starts_with('[') {
                Style::default().fg(Color::Cyan).bold()
            } else if line.contains('=') {
                Style::default().fg(Color::White)
            } else if line.starts_with('#') {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::styled(line, style)
        })
        .collect();

    let preview_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .title(" Generated TOML ");
    let preview = Paragraph::new(preview_lines)
        .block(preview_block)
        .wrap(Wrap { trim: false });
    f.render_widget(preview, chunks[0]);

    // Right: summary
    let mut summary_lines = vec![
        Line::from(Span::styled(
            "Configuration Summary",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Boards:",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    for b in &app.selected_boards {
        summary_lines.push(Line::from(vec![
            Span::raw("  • "),
            Span::styled(&b.slug, Style::default().fg(Color::Green)),
            Span::styled(
                format!(" (stack {}, slave {})", b.stack, b.slave_id),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    summary_lines.push(Line::from(""));
    summary_lines.push(Line::from(vec![
        Span::styled("Output: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.output_path.display().to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    summary_lines.push(Line::from(""));
    summary_lines.push(Line::from(""));
    summary_lines.push(Line::from(Span::styled(
        "Press 's' or Enter to save",
        Style::default().fg(Color::Green).bold(),
    )));
    summary_lines.push(Line::from(Span::styled(
        "Press Esc to go back",
        Style::default().fg(Color::DarkGray),
    )));

    let summary_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .title(" Summary ");
    let summary = Paragraph::new(summary_lines)
        .block(summary_block)
        .wrap(Wrap { trim: true });
    f.render_widget(summary, chunks[1]);
}
