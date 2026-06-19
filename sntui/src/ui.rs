use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
        TableState, Wrap,
    },
    Frame,
};

use crate::app::{App, BaseView, BrowserItem, ColPickerEntry, InputMode, Overlay};
use crate::tables::display_value;

// ── Palette ────────────────────────────────────────────────────────────────────

const C_ACCENT: Color = Color::Cyan;
const C_SELECTED_BG: Color = Color::Rgb(0, 80, 160);
const C_CATEGORY: Color = Color::Yellow;
const C_DIM: Color = Color::DarkGray;
const C_GOOD: Color = Color::Green;
const C_ERROR: Color = Color::Red;
const C_WARN: Color = Color::Yellow;
const C_KEY: Color = Color::Cyan;
const C_LOADING: Color = Color::Yellow;

// ── Entry point ────────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    let has_input = matches!(app.mode, InputMode::Filter | InputMode::Command);
    let mut constraints = vec![
        Constraint::Length(1), // header
        Constraint::Fill(1),   // content
    ];
    if has_input {
        constraints.push(Constraint::Length(1)); // input bar
    }
    constraints.push(Constraint::Length(1)); // footer

    let chunks = Layout::vertical(constraints).split(area);

    render_header(f, app, chunks[0]);

    match &app.base_view {
        BaseView::TableBrowser => render_browser(f, app, chunks[1]),
        BaseView::AllTablesBrowser => render_all_tables(f, app, chunks[1]),
        BaseView::RecordList => render_record_list(f, app, chunks[1]),
        BaseView::RecordDetail => render_detail(f, app, chunks[1]),
    }

    let footer_idx = if has_input {
        render_input_bar(f, app, chunks[2]);
        3
    } else {
        2
    };
    render_footer(f, app, chunks[footer_idx]);

    // Overlays drawn on top
    if let Some(ref ov) = app.overlay {
        match ov {
            Overlay::ScriptRunner => render_script_overlay(f, app, area),
            Overlay::Help => render_help_overlay(f, app, area),
            Overlay::ColumnPicker => render_col_picker_overlay(f, app, area),
        }
    }
}

// ── Header ─────────────────────────────────────────────────────────────────────

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let status_color = if app.health.is_ready() {
        C_GOOD
    } else if app.health.status == "no_session" {
        C_WARN
    } else {
        C_ERROR
    };

    let instance = if app.health.instance_name.is_empty() {
        "not connected".to_string()
    } else {
        app.health.instance_name.clone()
    };

    let breadcrumb = match &app.base_view {
        BaseView::TableBrowser => String::new(),
        BaseView::AllTablesBrowser => "  >  all tables".into(),
        BaseView::RecordList => {
            let label = if app.record_list_title.is_empty() {
                app.current_table.as_str()
            } else {
                app.record_list_title.as_str()
            };
            let count = if app.records_loading {
                " (loading...)".into()
            } else {
                format!(" ({} records)", app.records.len())
            };
            format!("  >  {label}{count}")
        }
        BaseView::RecordDetail => {
            let label = if app.record_list_title.is_empty() {
                app.current_table.as_str()
            } else {
                app.record_list_title.as_str()
            };
            format!("  >  {label}  >  {}", app.detail_sys_id)
        }
    };

    let spans = vec![
        Span::styled(" sntui ", Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" │ ", Style::default().fg(C_DIM)),
        Span::styled(&instance, Style::default().fg(Color::White)),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("[{}]", app.health.status_label()),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(breadcrumb, Style::default().fg(C_DIM)),
    ];

    let left = Line::from(spans);
    let para = Paragraph::new(left)
        .style(Style::default().bg(Color::Rgb(15, 30, 55)).fg(Color::White));
    f.render_widget(para, area);
}

// ── Footer ─────────────────────────────────────────────────────────────────────

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints: Vec<(&str, &str)> = match (&app.base_view, &app.overlay) {
        (_, Some(Overlay::Help)) => vec![("any key", "close help")],
        (_, Some(Overlay::ColumnPicker)) => vec![
            ("Space", "toggle"),
            ("K/J", "reorder"),
            ("t", "names"),
            ("Esc", "save & close"),
        ],
        (_, Some(Overlay::ScriptRunner)) => vec![
            ("i", "edit"),
            ("Ctrl+R", "run"),
            ("j/k", "scroll output"),
            ("Esc", "close"),
        ],
        (BaseView::TableBrowser, None) => vec![
            ("j/k", "navigate"),
            ("Enter", "open"),
            (":", "go to table"),
            ("s", "scripts"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (BaseView::AllTablesBrowser, None) => vec![
            ("j/k", "navigate"),
            ("Enter", "open"),
            ("/", "filter"),
            ("Esc", "back"),
        ],
        (BaseView::RecordList, None) => vec![
            ("j/k", "navigate"),
            ("Enter", "describe"),
            ("/", "filter"),
            ("r", "refresh"),
            ("c", "columns"),
            ("t", "names"),
            (":", "go to table"),
            ("s", "scripts"),
            ("Esc", "back"),
        ],
        (BaseView::RecordDetail, None) => vec![
            ("j/k", "scroll"),
            ("r", "refresh"),
            ("s", "scripts"),
            ("Esc", "back"),
        ],
    };

    let mut spans: Vec<Span> = Vec::new();
    for (key, desc) in &hints {
        spans.push(Span::styled(format!(" {key}"), Style::default().fg(C_KEY).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(":{desc}  "), Style::default().fg(C_DIM)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(Style::default().bg(Color::Rgb(15, 15, 25)).fg(Color::White)),
        area,
    );
}

// ── Input bar ─────────────────────────────────────────────────────────────────

fn render_input_bar(f: &mut Frame, app: &App, area: Rect) {
    let (prefix, buf) = match app.mode {
        InputMode::Filter => ("/", &app.filter_buf),
        InputMode::Command => (":", &app.command_buf),
        _ => return,
    };
    let text = format!("{prefix}{buf}_");
    f.render_widget(
        Paragraph::new(text).style(Style::default().bg(Color::Rgb(30, 30, 50)).fg(Color::White)),
        area,
    );
}

// ── Table browser ─────────────────────────────────────────────────────────────

fn render_browser(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .browser_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let selected = i == app.browser_cursor;
            match item {
                BrowserItem::Header(name) if name.is_empty() => {
                    ListItem::new(Line::from(Span::styled(
                        "─".repeat(area.width as usize),
                        Style::default().fg(C_DIM),
                    )))
                }
                BrowserItem::Header(name) => ListItem::new(Line::from(Span::styled(
                    format!("  {name}"),
                    Style::default().fg(C_CATEGORY).add_modifier(Modifier::BOLD),
                ))),
                BrowserItem::CustomList { name, table, query, columns: _, .. } => {
                    let style = if selected {
                        Style::default().bg(C_SELECTED_BG).fg(Color::White)
                    } else {
                        Style::default()
                    };
                    let arrow = if selected { "▶ " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("  {arrow}"), style),
                        Span::styled(
                            format!("{name:<24}"),
                            if selected {
                                style
                            } else {
                                Style::default().fg(Color::White)
                            },
                        ),
                        Span::styled(
                            format!(" {table}"),
                            if selected {
                                style.fg(Color::LightCyan)
                            } else {
                                Style::default().fg(C_DIM)
                            },
                        ),
                        Span::styled(
                            format!("  {query}"),
                            if selected {
                                style.fg(Color::LightYellow)
                            } else {
                                Style::default().fg(Color::Rgb(80, 80, 80))
                            },
                        ),
                    ]))
                }
                BrowserItem::Table { name, label } => {
                    let style = if selected {
                        Style::default().bg(C_SELECTED_BG).fg(Color::White)
                    } else {
                        Style::default()
                    };
                    let arrow = if selected { "▶ " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("  {arrow}"), style),
                        Span::styled(
                            format!("{label:<24}"),
                            if selected { style } else { Style::default().fg(Color::White) },
                        ),
                        Span::styled(
                            format!(" {name}"),
                            if selected {
                                style.fg(Color::LightCyan)
                            } else {
                                Style::default().fg(C_DIM)
                            },
                        ),
                    ]))
                }
                BrowserItem::BrowseAll => {
                    let style = if selected {
                        Style::default().bg(C_SELECTED_BG).fg(Color::White)
                    } else {
                        Style::default().fg(C_ACCENT)
                    };
                    let arrow = if selected { "▶ " } else { "  " };
                    ListItem::new(Line::from(Span::styled(
                        format!("  {arrow}Browse all tables…"),
                        style,
                    )))
                }
            }
        })
        .collect();

    let block = Block::default()
        .title(" Tables ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_DIM));

    f.render_widget(List::new(items).block(block), area);
}

// ── All-tables browser ────────────────────────────────────────────────────────

fn render_all_tables(f: &mut Frame, app: &App, area: Rect) {
    if app.all_tables_loading {
        let msg = Paragraph::new("  Loading tables…")
            .style(Style::default().fg(C_LOADING))
            .block(
                Block::default()
                    .title(" All Tables ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(C_DIM)),
            );
        f.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .all_tables
        .iter()
        .enumerate()
        .map(|(i, (name, label))| {
            let selected = i == app.all_tables_cursor;
            let style = if selected {
                Style::default().bg(C_SELECTED_BG).fg(Color::White)
            } else {
                Style::default()
            };
            let arrow = if selected { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {arrow}"), style),
                Span::styled(format!("{label:<30}"), if selected { style } else { Style::default().fg(Color::White) }),
                Span::styled(format!(" {name}"), if selected { style.fg(Color::LightCyan) } else { Style::default().fg(C_DIM) }),
            ]))
        })
        .collect();

    let filter_info = if app.all_tables_filter.is_empty() {
        String::new()
    } else {
        format!(" [/{}]", app.all_tables_filter)
    };

    let title = format!(" All Tables ({}){}  ", app.all_tables.len(), filter_info);
    let mut state = ListState::default().with_selected(Some(app.all_tables_cursor));
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_DIM));

    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

// ── Record list ────────────────────────────────────────────────────────────────

pub fn render_record_list(f: &mut Frame, app: &App, area: Rect) {
    let label: &str = if app.record_list_title.is_empty() {
        app.current_table_def
            .as_ref()
            .map(|t| t.label.as_str())
            .unwrap_or(&app.current_table)
    } else {
        &app.record_list_title
    };

    if app.records_loading {
        let msg = Paragraph::new("  Loading…")
            .style(Style::default().fg(C_LOADING))
            .block(Block::default().title(format!(" {label} ")).borders(Borders::ALL).border_style(Style::default().fg(C_DIM)));
        f.render_widget(msg, area);
        return;
    }

    if app.records.is_empty() {
        let query_info = if app.record_filter.is_empty() {
            String::new()
        } else {
            format!(" (filter: {})", app.record_filter)
        };
        let msg = Paragraph::new(format!("  No records found{query_info}."))
            .style(Style::default().fg(C_DIM))
            .block(Block::default().title(format!(" {label} ")).borders(Borders::ALL).border_style(Style::default().fg(C_DIM)));
        f.render_widget(msg, area);
        return;
    }

    let columns = app.effective_columns();

    let constraints: Vec<Constraint> = columns
        .iter()
        .map(|c| if c.width == 0 { Constraint::Fill(1) } else { Constraint::Length(c.width) })
        .collect();

    let names_hint = if app.display_names { "" } else { " [tech]" };
    let header_cells: Vec<Cell> = columns
        .iter()
        .map(|c| {
            let label = if app.display_names {
                app.current_schema
                    .as_ref()
                    .and_then(|s| s.columns.get(&c.field))
                    .map(|col| col.label.as_str())
                    .filter(|l| !l.is_empty())
                    .unwrap_or(c.header.as_str())
            } else {
                c.field.as_str()
            };
            Cell::from(label.to_string())
                .style(Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD))
        })
        .collect();
    let header = Row::new(header_cells).style(Style::default().bg(Color::Rgb(25, 25, 45)));

    let rows: Vec<Row> = app
        .records
        .iter()
        .enumerate()
        .map(|(i, rec)| {
            let selected = i == app.record_cursor;
            let cells: Vec<Cell> = columns
                .iter()
                .map(|c| {
                    let val = rec.get(&c.field).map(display_value).unwrap_or_default();
                    let style = if selected {
                        Style::default().bg(C_SELECTED_BG).fg(Color::White)
                    } else {
                        Style::default()
                    };
                    Cell::from(val).style(style)
                })
                .collect();
            Row::new(cells)
        })
        .collect();

    let filter_info = if app.record_filter.is_empty() {
        String::new()
    } else {
        format!(" [{}]", app.record_filter)
    };
    let title = format!(" {} ({}){}{}  ", label, app.records.len(), filter_info, names_hint);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_DIM));

    let mut state = TableState::default().with_selected(Some(app.record_cursor));
    let table = Table::new(rows, &constraints)
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().bg(C_SELECTED_BG));

    f.render_stateful_widget(table, area, &mut state);
}

// ── Record detail ──────────────────────────────────────────────────────────────

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let label = app
        .current_table_def
        .as_ref()
        .map(|t| t.label.as_str())
        .unwrap_or(&app.current_table);

    if app.detail_loading {
        let msg = Paragraph::new("  Loading…")
            .style(Style::default().fg(C_LOADING))
            .block(Block::default().title(format!(" {label} ")).borders(Borders::ALL).border_style(Style::default().fg(C_DIM)));
        f.render_widget(msg, area);
        return;
    }

    let Some(ref record) = app.detail_record else {
        let msg = Paragraph::new("  No record loaded.")
            .style(Style::default().fg(C_DIM))
            .block(Block::default().title(format!(" {label} ")).borders(Borders::ALL).border_style(Style::default().fg(C_DIM)));
        f.render_widget(msg, area);
        return;
    };

    let obj = match record.as_object() {
        Some(m) => m,
        None => {
            f.render_widget(
                Paragraph::new(record.to_string()).wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
    };

    // Use the pre-computed ordered key list from App state (same ordering as handler uses)
    let keys = &app.detail_field_keys;

    let rows: Vec<Row> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let val = obj.get(k).map(display_value).unwrap_or_default();
            let selected = i == app.detail_field_cursor;

            let col = app.current_schema.as_ref().and_then(|s| s.columns.get(k));
            let is_ref = col.map(|c| c.is_reference() && !c.reference.is_empty()).unwrap_or(false);

            let key_text = match col {
                Some(c) if !c.label.is_empty() && c.label != *k => {
                    if is_ref {
                        format!("{} →{}", c.label, c.reference)
                    } else {
                        c.label.clone()
                    }
                }
                _ => k.clone(),
            };
            // selected reference fields show the [↵] hint in the value column
            let display_val = if selected && is_ref && !val.is_empty() {
                format!("{val}  [↵ open]")
            } else {
                val
            };

            let key_style = if col.map(|c| c.mandatory).unwrap_or(false) {
                Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_ACCENT)
            };
            let val_style = if is_ref {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };

            let row = Row::new(vec![
                Cell::from(key_text).style(key_style),
                Cell::from(display_val).style(val_style),
            ]);
            if selected {
                row.style(Style::default().bg(C_SELECTED_BG))
            } else {
                row
            }
        })
        .collect();

    let history_hint = if !app.detail_history.is_empty() {
        format!(" [Esc: back ×{}]", app.detail_history.len())
    } else {
        String::new()
    };
    let title = format!(" {} — {}{} ", label, app.detail_sys_id, history_hint);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_DIM));

    let mut state = TableState::default().with_selected(Some(app.detail_field_cursor));
    let table = Table::new(rows, &[Constraint::Length(28), Constraint::Fill(1)])
        .block(block)
        .row_highlight_style(Style::default().bg(C_SELECTED_BG));

    f.render_stateful_widget(table, area, &mut state);
}

// ── Column picker overlay ─────────────────────────────────────────────────────

fn render_col_picker_overlay(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(68, 84, area);
    f.render_widget(Clear, popup);

    let active_count = app.col_picker_fields.iter().filter(|e| e.active).count();
    let inactive_count = app.col_picker_fields.len() - active_count;
    let name_mode = if app.display_names { "display" } else { "technical" };

    let block = Block::default()
        .title(format!(
            " Columns  {active_count} active · {inactive_count} available  \
             Space:toggle  K/J:reorder  t:names({name_mode})  Esc:save "
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_ACCENT));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Build render items, injecting a visual separator before the first inactive entry.
    let mut items: Vec<ListItem> = Vec::new();
    // render_cursor maps from col_picker_cursor (logical) to list selection index (includes separator).
    let render_cursor = if app.col_picker_cursor >= active_count && inactive_count > 0 {
        app.col_picker_cursor + 1
    } else {
        app.col_picker_cursor
    };

    for (i, entry) in app.col_picker_fields.iter().enumerate() {
        if i == active_count && inactive_count > 0 {
            items.push(ListItem::new(Line::from(Span::styled(
                format!(
                    " ── available ({inactive_count}) {}",
                    "─".repeat(popup.width.saturating_sub(20) as usize)
                ),
                Style::default().fg(C_DIM),
            ))));
        }

        let selected = i == app.col_picker_cursor;
        let bg = if selected { C_SELECTED_BG } else { Color::Reset };

        let (check, check_color) = if entry.active {
            ("✓", C_GOOD)
        } else {
            (" ", C_DIM)
        };

        let (primary, secondary) = col_picker_names(entry, app.display_names);

        let name_style = Style::default()
            .fg(if entry.active { Color::White } else { C_DIM })
            .bg(bg)
            .add_modifier(if entry.active { Modifier::BOLD } else { Modifier::empty() });

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {check} "), Style::default().fg(check_color).bg(bg)),
            Span::styled(format!("{primary:<32}"), name_style),
            Span::styled(
                format!("  {secondary}"),
                Style::default().fg(C_DIM).bg(bg),
            ),
        ])));
    }

    let mut state = ListState::default().with_selected(Some(render_cursor));
    f.render_stateful_widget(
        List::new(items).block(Block::default()),
        inner,
        &mut state,
    );
}

fn col_picker_names<'a>(entry: &'a ColPickerEntry, display_names: bool) -> (&'a str, &'a str) {
    if display_names {
        let primary = if entry.label.is_empty() { entry.field.as_str() } else { entry.label.as_str() };
        let secondary = if entry.label.is_empty() { "" } else { entry.field.as_str() };
        (primary, secondary)
    } else {
        let secondary = if entry.label.is_empty() { "" } else { entry.label.as_str() };
        (entry.field.as_str(), secondary)
    }
}

// ── Script runner overlay ─────────────────────────────────────────────────────

fn render_script_overlay(f: &mut Frame, app: &App, area: Rect) {
    // Centre a 80% × 80% popup
    let popup = centered_rect(82, 82, area);
    f.render_widget(Clear, popup);

    let is_input = matches!(app.mode, InputMode::Script);
    let title_suffix = if is_input { " [Ctrl+R: run] [Esc: stop edit]" } else { " [i: edit] [Ctrl+R: run] [Esc: close]" };
    let block = Block::default()
        .title(format!(" Script Runner{title_suffix} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_ACCENT));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Split: top = script input, bottom = output
    let chunks = Layout::vertical([
        Constraint::Length(6),  // script input area
        Constraint::Length(1),  // separator label
        Constraint::Fill(1),    // output
    ])
    .split(inner);

    // Script input
    let display_script = if is_input {
        let mut s = app.script_buf.clone();
        let pos = app.script_cursor.min(s.len());
        s.insert(pos, '█');
        s
    } else {
        app.script_buf.clone()
    };
    let script_para = Paragraph::new(display_script)
        .style(Style::default().fg(Color::White).bg(Color::Rgb(20, 20, 40)))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if is_input {
                    Style::default().fg(C_ACCENT)
                } else {
                    Style::default().fg(C_DIM)
                })
                .title(" JavaScript "),
        );
    f.render_widget(script_para, chunks[0]);

    // Output label
    let out_label = if app.script_running {
        Paragraph::new(" ⏳ Output (running…)")
            .style(Style::default().fg(C_LOADING))
    } else {
        Paragraph::new(format!(" Output ({} lines)", app.script_output.len()))
            .style(Style::default().fg(C_DIM))
    };
    f.render_widget(out_label, chunks[1]);

    // Output area
    let out_lines: Vec<Line> = app
        .script_output
        .iter()
        .map(|l| {
            let color = if l.starts_with("ERROR:") { C_ERROR } else { Color::White };
            Line::from(Span::styled(l.as_str(), Style::default().fg(color)))
        })
        .collect();

    let scroll = app.script_out_scroll as u16;
    let out_para = Paragraph::new(out_lines)
        .style(Style::default().bg(Color::Rgb(10, 10, 20)))
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(C_DIM)));
    f.render_widget(out_para, chunks[2]);
}

// ── Help overlay ──────────────────────────────────────────────────────────────

fn render_help_overlay(f: &mut Frame, _app: &App, area: Rect) {
    let popup = centered_rect(60, 70, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Help — any key to close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_ACCENT));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows: Vec<Row> = vec![
        Row::new(vec![Cell::from("Navigation").style(Style::default().fg(C_CATEGORY).add_modifier(Modifier::BOLD)), Cell::from("")]),
        keybind("j / ↓", "Move down"),
        keybind("k / ↑", "Move up"),
        keybind("g", "Go to top"),
        keybind("G", "Go to bottom"),
        keybind("Enter", "Open / describe"),
        keybind("Esc / q", "Go back / quit"),
        Row::new(vec![Cell::from(""), Cell::from("")]),
        Row::new(vec![Cell::from("Filtering & Commands").style(Style::default().fg(C_CATEGORY).add_modifier(Modifier::BOLD)), Cell::from("")]),
        keybind("/", "Filter records (SN encoded query)"),
        keybind(":", "Command mode — type a table name"),
        keybind("r", "Refresh current view"),
        Row::new(vec![Cell::from(""), Cell::from("")]),
        Row::new(vec![Cell::from("Tools").style(Style::default().fg(C_CATEGORY).add_modifier(Modifier::BOLD)), Cell::from("")]),
        keybind("s", "Open script runner"),
        keybind("c", "Column picker (record list)"),
        keybind("t", "Toggle display / technical names"),
        keybind("?", "Toggle this help"),
        keybind("Ctrl+C", "Quit"),
        Row::new(vec![Cell::from(""), Cell::from("")]),
        Row::new(vec![Cell::from("Script Runner").style(Style::default().fg(C_CATEGORY).add_modifier(Modifier::BOLD)), Cell::from("")]),
        keybind("i", "Enter edit mode"),
        keybind("Ctrl+R / F5", "Run script"),
        keybind("Esc", "Exit edit / close overlay"),
    ];

    let table = Table::new(rows, &[Constraint::Length(18), Constraint::Fill(1)])
        .column_spacing(2);

    f.render_widget(table, inner);
}

fn keybind<'a>(key: &'a str, desc: &'a str) -> Row<'a> {
    Row::new(vec![
        Cell::from(key).style(Style::default().fg(C_KEY).add_modifier(Modifier::BOLD)),
        Cell::from(desc).style(Style::default().fg(Color::White)),
    ])
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
