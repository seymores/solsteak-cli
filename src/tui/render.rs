//! Renderer consumes only the current in-memory report and view state.
use super::{Focus, Overlay, State};
use crate::{
    domain::{Account, format_sol},
    rewards::PERIOD,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

pub fn sanitize(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}
fn amount(value: Option<&str>) -> String {
    value
        .and_then(|v| v.parse::<u128>().ok())
        .map(format_sol)
        .unwrap_or_else(|| "Unknown".into())
}
fn short(value: &str) -> String {
    let value = sanitize(value);
    if value.chars().count() > 14 {
        format!(
            "{}..{}",
            value.chars().take(6).collect::<String>(),
            value
                .chars()
                .rev()
                .take(4)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
    } else {
        value
    }
}
fn block(title: impl Into<Line<'static>>, focus: bool, no_color: bool) -> Block<'static> {
    let style = if focus && !no_color {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(style)
}
fn paragraph(
    frame: &mut Frame<'_>,
    area: Rect,
    title: String,
    text: String,
    focus: bool,
    no_color: bool,
    scroll: u16,
) {
    let paragraph = Paragraph::new(text)
        .block(block(title, focus, no_color))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph.scroll((scroll, 0)), area);
}
fn focused(state: &State, focus: Focus, title: &str) -> String {
    format!("{}{}", if state.focus == focus { "> " } else { "" }, title)
}

pub fn draw(frame: &mut Frame<'_>, state: &mut State) {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        frame.render_widget(Paragraph::new(format!("SolSteak\nTerminal {}x{}; minimum 80x24.\nResize to restore the dashboard.\nq quit | Ctrl-C quit", area.width, area.height)).wrap(Wrap {trim:false}), area);
        return;
    }
    let secondary = 1 + if state.validators_expanded { 3 } else { 0 };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(secondary),
            Constraint::Length(2),
        ])
        .split(area);
    let report = state.report.as_ref();
    let data = report.and_then(|r| r.data.as_ref());
    let age = report
        .and_then(|r| r.sources.iter().map(|s| s.observed_at).min())
        .map(|t| format!("{}s", state.now.saturating_sub(t)))
        .unwrap_or_else(|| "Unknown".into());
    let epoch = data
        .map(|d| format!("{} ({})", sanitize(&d.epoch), sanitize(&d.epoch_kind)))
        .unwrap_or_else(|| "Unknown".into());
    let cache = report.is_some_and(|r| r.sources.iter().any(|s| s.cached));
    let stale = data.is_some_and(|d| d.stale);
    let header = format!(
        "SolSteak | mainnet | {}{}{} | epoch {} | age {}\n{}",
        if state.offline { "OFFLINE" } else { "ONLINE" },
        if cache { " CACHED" } else { "" },
        if stale { " STALE" } else { "" },
        epoch,
        age,
        sanitize(&state.address)
    );
    frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), areas[0]);
    let summary = data.map(|d| format!("Associated balance ({}): {} SOL\nRecorded delegation: {} SOL | {} accounts / {} validators\nCompleted epoch {} recorded {}: {} SOL\n{}",
        sanitize(&d.summary.amount_scope), amount(d.summary.balance_lamports.as_deref()), amount(d.summary.delegated_lamports.as_deref()), d.summary.account_count, d.summary.validator_count,
        d.requested_epochs.first().map(String::as_str).unwrap_or("None"),
        if report.is_some_and(|r| r.status == "complete") {"total"} else {"subtotal"}, amount(d.summary.latest_reward_lamports.as_deref()), report.and_then(|r| r.coverage.as_ref()).and_then(|c| c.rewards.first()).map(|c| format!("Coverage: recorded {} / no data {} / failed {} / not queried {}",c.recorded,c.no_data,c.failed,c.not_queried)).unwrap_or_else(|| "Coverage: Unknown".into())))
        .unwrap_or_else(|| if state.loading {"Loading observations...\nBalance: Unknown | Recorded delegation: Unknown\nRewards: No data yet".into()} else {"No usable observations.\nUse r to retry, a to change address, ? for help.\nBalance and rewards: Unknown".into()});
    frame.render_widget(Paragraph::new(summary).wrap(Wrap { trim: false }), areas[1]);
    let warnings = report.map_or(0, |r| r.warnings.len());
    let errors = report.map_or(0, |r| r.errors.len());
    let mut attention = report
        .map(|r| {
            r.errors
                .iter()
                .map(|e| format!("ERROR {}: {}", sanitize(&e.code), sanitize(&e.message)))
                .chain(r.warnings.iter().map(|w| {
                    format!(
                        "{} {} {}: {}",
                        sanitize(&w.severity).to_uppercase(),
                        sanitize(&w.code),
                        w.address.as_deref().map(short).unwrap_or_default(),
                        sanitize(&w.message)
                    )
                }))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if attention.is_empty() {
        attention.push(
            if state.loading {
                "Loading checks..."
            } else if report.is_some_and(|r| r.status == "complete") {
                "No issues detected by these checks"
            } else {
                "Required checks incomplete; no all-clear assessment."
            }
            .into(),
        );
    }
    let scroll = if state.focus == Focus::Attention {
        state.section_scroll.min(u16::MAX as usize)
    } else {
        0
    };
    paragraph(
        frame,
        areas[2],
        focused(
            state,
            Focus::Attention,
            &format!("Attention: {warnings} findings / {errors} errors"),
        ),
        attention.join("\n"),
        state.focus == Focus::Attention,
        state.no_color,
        scroll as u16,
    );
    // Detail is always visible: beside the table when wide, below it otherwise.
    let content = if area.width >= 120 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(areas[3])
    } else {
        Layout::default()
            .constraints([
                Constraint::Length((areas[3].height * 35 / 100).max(4)),
                Constraint::Min(0),
            ])
            .split(areas[3])
    };
    render_accounts(frame, content[0], state);
    render_detail(frame, content[1], state);
    render_secondary(frame, areas[4], state);
    let total = state
        .report
        .as_ref()
        .and_then(|r| r.data.as_ref())
        .map_or(0, |d| d.accounts.len());
    let footer = format!(
        "j/k move Tab focus </> epoch / find s sort r refresh a address ? help q quit\n{:?} | showing {} of {} | row {} | {}",
        state.focus,
        state.visible_count(),
        total,
        if state.visible_count() == 0 {
            0
        } else {
            state.selected + 1
        },
        sanitize(state.notice.as_deref().unwrap_or(if state.loading {
            "Loading; navigation remains available"
        } else {
            "Ready"
        }))
    );
    frame.render_widget(Paragraph::new(footer), areas[5]);
    if let Some(overlay) = state.overlay.clone() {
        render_overlay(frame, area, state, overlay);
    }
}

fn render_accounts(frame: &mut Frame<'_>, area: Rect, state: &mut State) {
    state.page_height = area.height.saturating_sub(3).max(1) as usize;
    state.ensure_visible();
    let title = focused(state, Focus::Accounts, "Stake accounts (SOL)");
    let Some(data) = state.report.as_ref().and_then(|r| r.data.as_ref()) else {
        paragraph(
            frame,
            area,
            title,
            if state.loading {
                "Loading..."
            } else {
                "No data. r retry | a address"
            }
            .into(),
            state.focus == Focus::Accounts,
            state.no_color,
            0,
        );
        return;
    };
    if data.accounts.is_empty() {
        paragraph(
            frame,
            area,
            title,
            "No associated native stake accounts found.".into(),
            state.focus == Focus::Accounts,
            state.no_color,
            0,
        );
        return;
    }
    if state.rows.is_empty() {
        paragraph(
            frame,
            area,
            title,
            "No matching rows. Esc clears search; totals include all accounts.".into(),
            true,
            state.no_color,
            0,
        );
        return;
    }
    // Only construct widgets for the visible rows; the full set remains navigable.
    let wide = area.width >= 100;
    let compact = area.width < 76;
    let rows = state
        .rows
        .iter()
        .enumerate()
        .skip(state.offset)
        .take(state.page_height)
        .map(|(position, &i)| {
            let a = &data.accounts[i];
            let mut cells = vec![Cell::from(format!(
                "{}{}",
                if position == state.selected { ">" } else { " " },
                short(&a.address)
            ))];
            if wide {
                cells.push(Cell::from(
                    a.vote_address
                        .as_deref()
                        .map(short)
                        .unwrap_or_else(|| "Unknown".into()),
                ));
            }
            cells.push(Cell::from(
                Line::from(amount(a.balance_lamports.as_deref())).right_aligned(),
            ));
            if !compact {
                cells.push(Cell::from(sanitize(&a.state)));
                cells.push(Cell::from(
                    Line::from(latest_reward(data, a)).right_aligned(),
                ));
            }
            Row::new(cells).style(if position == state.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            })
        });
    let mut widths = vec![Constraint::Min(12)];
    let mut headers = vec!["Account"];
    if wide {
        widths.push(Constraint::Length(14));
        headers.push("Validator");
    }
    widths.push(Constraint::Length(21));
    headers.push("Balance SOL");
    if !compact {
        widths.extend([Constraint::Length(12), Constraint::Length(21)]);
        headers.extend(["Observed", "Reward SOL"]);
    }
    frame.render_widget(
        Table::new(rows, widths)
            .header(Row::new(headers).style(Style::default().add_modifier(Modifier::BOLD)))
            .block(block(title, state.focus == Focus::Accounts, state.no_color)),
        area,
    );
}
fn latest_reward(data: &crate::domain::Data, a: &Account) -> String {
    data.rewards
        .first()
        .and_then(|r| r.entries.iter().find(|r| r.address == a.address))
        .map(|r| {
            r.record
                .as_ref()
                .map(|r| amount(Some(&r.amount_lamports)))
                .unwrap_or_else(|| {
                    match r.state.as_str() {
                        "failed" => "Failed",
                        "not_queried" => "Not queried",
                        _ => "No data",
                    }
                    .into()
                })
        })
        .unwrap_or_else(|| "No data".into())
}
fn render_detail(frame: &mut Frame<'_>, area: Rect, state: &mut State) {
    let focus = state.focus == Focus::Detail;
    let Some(a) = state.selected_account().cloned() else {
        let text = if state.loading {
            "Loading..."
        } else {
            "No account selected."
        };
        paragraph(
            frame,
            area,
            focused(state, Focus::Detail, "Account Detail"),
            text.into(),
            focus,
            state.no_color,
            0,
        );
        return;
    };
    let report = state.report.as_ref().unwrap();
    let data = report.data.as_ref().unwrap();
    let validator = data
        .validators
        .iter()
        .find(|v| a.vote_address.as_deref() == Some(&v.vote_address));
    let title = focused(
        state,
        Focus::Detail,
        &format!(
            "Account Detail | {} | Validator {}",
            short(&a.address),
            validator.map_or_else(|| "none".into(), |v| short(&v.vote_address))
        ),
    );
    // Chart first, then the account fields; one scrolling body so small panels reach both.
    let inner = area.width.saturating_sub(2);
    let mut lines = chart_lines(state, &a, inner, usize::from(area.height.saturating_sub(2)));
    lines.push(String::new());
    lines.extend(account_fields(state, &a));
    let body_rows: usize = lines.iter().map(|l| wrapped(l, inner)).sum();
    let page = usize::from(area.height.saturating_sub(2));
    let scroll = usize::from(state.detail_scroll).min(body_rows.saturating_sub(page)) as u16;
    state.detail_scroll = scroll;
    paragraph(
        frame,
        area,
        title,
        lines.join("\n"),
        focus,
        state.no_color,
        scroll,
    );
}
fn account_fields(state: &State, a: &Account) -> Vec<String> {
    let mut lines = vec![
        format!("Account: {}", sanitize(&a.address)),
        format!(
            "Relationship: {}",
            a.relationship
                .as_deref()
                .unwrap_or("Not applicable (direct)")
        ),
        format!("Observed state: {}", sanitize(&a.state)),
        format!("Balance: {} SOL", amount(a.balance_lamports.as_deref())),
        format!(
            "Recorded delegation: {} SOL",
            amount(a.delegated_lamports.as_deref())
        ),
        format!(
            "Rent reserve: {} SOL",
            amount(a.rent_reserve_lamports.as_deref())
        ),
        format!(
            "Undelegated principal: {} SOL",
            amount(a.undelegated_lamports.as_deref())
        ),
        "Effective / activating / deactivating: Unknown".into(),
    ];
    for (name, value) in [
        ("Staker", a.staker.as_deref()),
        ("Withdrawer", a.withdrawer.as_deref()),
        ("Vote account", a.vote_address.as_deref()),
        ("Activation epoch", a.activation_epoch.as_deref()),
        ("Deactivation epoch", a.deactivation_epoch.as_deref()),
    ] {
        lines.push(format!("{name}: {}", sanitize(value.unwrap_or("Unknown"))));
    }
    if let Some(l) = &a.lockup {
        lines.push(format!(
            "Lockup: epoch {}, timestamp {}, custodian {} (observed; not a withdrawability claim)",
            sanitize(&l.epoch),
            sanitize(&l.unix_timestamp),
            sanitize(&l.custodian)
        ));
    }
    let report = state.report.as_ref().unwrap();
    let data = report.data.as_ref().unwrap();
    if let Some(v) = data
        .validators
        .iter()
        .find(|v| Some(&v.vote_address) == a.vote_address.as_ref())
    {
        lines.push(format!(
            "Validator: {} | {} | current commission {}",
            sanitize(v.name.as_deref().unwrap_or("Unknown")),
            sanitize(&v.state),
            v.commission
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "Unknown".into())
        ));
    }
    for epoch in &data.rewards {
        if let Some(r) = epoch.entries.iter().find(|r| r.address == a.address) {
            lines.push(format!(
                "Epoch {} reward: {} | latest attempt {} | account return estimate {}",
                sanitize(&epoch.epoch),
                r.record
                    .as_ref()
                    .map(|r| format!(
                        "{} SOL; slot {}; source {}",
                        amount(Some(&r.amount_lamports)),
                        sanitize(&r.effective_slot),
                        sanitize(&r.source_id)
                    ))
                    .unwrap_or_else(|| sanitize(&r.state)),
                sanitize(&r.latest_attempt),
                estimate(r)
            ));
        }
    }
    if let Some(source) = report.sources.iter().find(|s| s.id == a.source_id) {
        lines.push(format!(
            "Source: {} / {} | observed {} | slot {} | {} | age {}s",
            sanitize(&source.id),
            sanitize(&source.provider),
            source.observed_at,
            sanitize(source.slot.as_deref().unwrap_or("Unknown")),
            if source.cached { "cached" } else { "RPC" },
            state.now.saturating_sub(source.observed_at)
        ));
    }
    lines.push("Authorities do not establish ownership. Rewards cover selected current accounts, not lifetime earnings. Inflation only; no MEV, validator APY or fiat pricing.".into());
    lines
}
fn render_secondary(frame: &mut Frame<'_>, area: Rect, state: &State) {
    let title = focused(
        state,
        Focus::Validators,
        &format!(
            "{} Validators | Enter {}",
            if state.validators_expanded {
                "[-]"
            } else {
                "[+]"
            },
            if state.validators_expanded {
                "collapse"
            } else {
                "expand"
            }
        ),
    );
    if !state.validators_expanded {
        frame.render_widget(Paragraph::new(title), area);
        return;
    }
    let offset = if state.focus == Focus::Validators {
        state.section_scroll
    } else {
        0
    };
    let rows = state.validator_rows();
    let lines = rows
        .iter()
        .skip(offset.min(rows.len().saturating_sub(1)))
        .take(3)
        .map(|v| {
            format!(
                "{} {} | {} | commission {} | recorded delegation ratio {} / {} SOL",
                short(&v.vote_address),
                sanitize(v.name.as_deref().unwrap_or("")),
                sanitize(&v.state),
                v.commission
                    .map(|c| format!("{c}%"))
                    .unwrap_or_else(|| "Unknown".into()),
                amount(v.delegated_lamports.as_deref()),
                amount(v.concentration_denominator_lamports.as_deref())
            )
        })
        .collect::<Vec<_>>();
    paragraph(
        frame,
        area,
        title,
        if lines.is_empty() {
            "No data".into()
        } else {
            lines.join("\n")
        },
        state.focus == Focus::Validators,
        state.no_color,
        0,
    );
}
fn estimate(entry: &crate::domain::RewardEntry) -> String {
    entry
        .account_return
        .as_ref()
        .map(|r| format!("{}%", sanitize(&r.annualized_percent)))
        .unwrap_or_else(|| "Unknown".into())
}
const BARS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Rows a line occupies once word-wrapped at `width`.
fn wrapped(line: &str, width: u16) -> usize {
    let width = usize::from(width).max(1);
    let (mut rows, mut used) = (1, 0);
    for word in line.split(' ') {
        let len = word.chars().count();
        if used == 0 {
            used = len;
        } else if used + 1 + len <= width {
            used += 1 + len;
        } else {
            rows += 1;
            used = len;
        }
        while used > width {
            rows += 1;
            used -= width;
        }
    }
    rows
}
fn signed(value: i128) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}
fn gap_words(entry: Option<&crate::domain::RewardEntry>) -> &'static str {
    match entry.map(|e| e.state.as_str()) {
        Some("failed") => "request failed",
        Some("not_queried") => "not queried",
        None => "no epoch",
        _ => "no data returned",
    }
}
/// One side of the selected pair: exact lamports and estimate, or why there is no record.
fn pair_side(entry: Option<&crate::domain::RewardEntry>) -> String {
    match entry.and_then(|e| e.record.as_ref().map(|r| (e, r))) {
        Some((e, r)) => format!(
            "{} lamports est {}{}",
            sanitize(&r.amount_lamports),
            estimate(e),
            if e.latest_attempt == "recorded" {
                String::new()
            } else {
                format!(" (latest attempt {})", sanitize(&e.latest_attempt))
            }
        ),
        None => gap_words(entry).into(),
    }
}
/// The selected row's current-vs-previous 15 epochs: one column per pair, oldest left,
/// previous bar (shade) beside current bar (blocks) on one scale. Bars take the height
/// left in `avail` rows after the priority lines; the legend, latest estimate and
/// attribution follow and scroll into view when the panel is short.
fn chart_lines(state: &State, account: &Account, inner: u16, avail: usize) -> Vec<String> {
    let (Some(data), Some(view)) = (
        state.report.as_ref().and_then(|r| r.data.as_ref()),
        state.graph(),
    ) else {
        return vec!["15-epoch rewards: no data".into()];
    };
    if view.epochs == 0 {
        return vec!["15-epoch rewards: no completed epochs".into()];
    }
    let wide = inner >= 75;
    let entry = |pos: usize| {
        data.rewards
            .get(pos)
            .and_then(|r| r.entries.iter().find(|e| e.address == account.address))
    };
    let recorded = |pos: usize| {
        entry(pos)
            .and_then(|e| e.record.as_ref())
            .and_then(|r| r.amount_lamports.parse::<u128>().ok())
    };
    let max = (0..view.epochs).filter_map(recorded).max().unwrap_or(0);
    let pairs = view.epochs.saturating_sub(PERIOD).min(PERIOD);
    let summary = match data
        .comparisons
        .iter()
        .find(|c| c.address == account.address)
    {
        _ if pairs == 0 => "Change vs previous 15 epochs: Unknown (no previous period)".to_string(),
        Some(c) if c.compared_pairs > 0 => {
            let diff = c
                .difference_lamports
                .as_deref()
                .and_then(|d| d.parse::<i128>().ok());
            format!(
                "Change vs previous 15 epochs: {} {} lamports ({}) | est {} pp | {} of {} pairs compared",
                match diff {
                    Some(d) if d > 0 => "higher",
                    Some(d) if d < 0 => "lower",
                    _ => "unchanged",
                },
                diff.map_or_else(|| "Unknown".into(), signed),
                c.percent_change
                    .as_deref()
                    .map_or_else(|| "Unknown".into(), |p| format!("{}%", sanitize(p))),
                c.estimate_difference_pp
                    .as_deref()
                    .map_or_else(|| "Unknown".into(), sanitize),
                c.compared_pairs,
                c.compared_pairs + c.left_out_pairs
            )
        }
        _ => format!("Change vs previous 15 epochs: Unknown (0 of {pairs} pairs compared)"),
    };
    let current = data.rewards.get(view.epoch_pos);
    let previous = data.rewards.get(PERIOD + view.epoch_pos);
    let (cur, prev) = (entry(view.epoch_pos), entry(PERIOD + view.epoch_pos));
    let difference = match (recorded(view.epoch_pos), recorded(PERIOD + view.epoch_pos)) {
        (Some(c), Some(p)) => format!("difference {} lamports", signed(c as i128 - p as i128)),
        _ if previous.is_none() => "no previous epoch".into(),
        _ => "not compared (needs a recorded reward in both periods)".into(),
    };
    let pair_title = format!(
        "Pair {} vs {}: {difference}",
        current.map_or_else(|| "?".into(), |e| sanitize(&e.epoch)),
        previous.map_or_else(|| "none".into(), |e| sanitize(&e.epoch))
    );
    let pair_detail = format!("Current {} | Previous {}", pair_side(cur), pair_side(prev));
    let priority =
        wrapped(&summary, inner) + wrapped(&pair_title, inner) + wrapped(&pair_detail, inner) + 1;
    let bars = avail.saturating_sub(priority).clamp(1, 8);
    let mut lines = vec![summary];
    // Bar cells: wide panels use two cells per bar, narrow ones one; columns end in a gap.
    let cell = |glyph: char, marker: bool| match (wide, marker) {
        (true, false) => format!("{glyph}{glyph}"),
        (true, true) => format!("{glyph} "),
        (false, _) => glyph.to_string(),
    };
    let gap = " ";
    for row in 0..bars {
        let bottom = row == bars - 1;
        let mut line = String::new();
        for i in (0..view.columns).rev() {
            // Previous period: shade over whole rows, at least one for a nonzero reward.
            let has_prev = PERIOD + i < view.epochs;
            let prev_cell = if !has_prev {
                cell(' ', false)
            } else {
                match (
                    recorded(PERIOD + i),
                    entry(PERIOD + i).map(|e| e.state.as_str()),
                ) {
                    (Some(v), _) if v > 0 && max > 0 => {
                        let rows = (v * bars as u128).div_ceil(max).max(1) as usize;
                        cell(if bars - 1 - row < rows { '░' } else { ' ' }, false)
                    }
                    (Some(_), _) if bottom => cell('0', true),
                    (None, Some("failed")) if bottom => cell('!', true),
                    (None, Some("not_queried")) if bottom => cell('?', true),
                    (None, Some("no_data")) if bottom => cell('-', true),
                    _ => cell(' ', false),
                }
            };
            // Current period: eighth blocks, at least one eighth for a nonzero reward.
            let units = recorded(i).map_or(0, |v| {
                if v == 0 || max == 0 {
                    0
                } else {
                    (v * (bars as u128 * 8)).div_ceil(max).max(1)
                }
            });
            let level = units.saturating_sub((bars - 1 - row) as u128 * 8).min(8) as usize;
            let cur_cell = match (recorded(i), entry(i).map(|e| e.state.as_str())) {
                (Some(0), _) if bottom => cell('0', true),
                (None, Some("failed")) if bottom => cell('!', true),
                (None, Some("not_queried")) if bottom => cell('?', true),
                (None, Some("no_data")) if bottom => cell('-', true),
                _ => cell(BARS[level], false),
            };
            line.push_str(&prev_cell);
            line.push_str(&cur_cell);
            line.push_str(gap);
        }
        lines.push(line);
    }
    let blank = cell(' ', false);
    lines.push(
        (0..view.columns)
            .rev()
            .map(|i| {
                let mark = if i == view.epoch_pos { '^' } else { ' ' };
                // The caret sits under the current bar of its pair.
                format!("{blank}{}{gap}", cell(mark, true))
            })
            .collect(),
    );
    lines.push(pair_title);
    lines.push(pair_detail);
    lines.push(
        "Legend: ░ previous 15  █ current 15  - no data  ! failed  ? not queried  0 zero".into(),
    );
    lines.push(match data.rewards.first().zip(entry(0)) {
        Some((latest, e)) => format!(
            "Latest completed epoch {}: Annualized account return estimate {}",
            sanitize(&latest.epoch),
            estimate(e)
        ),
        None => "Annualized account return estimate: Unknown".into(),
    });
    lines.push("Historical validator attribution unverified (current-validator grouping).".into());
    lines
}
fn render_overlay(frame: &mut Frame<'_>, area: Rect, state: &State, overlay: Overlay) {
    let popup = Rect::new(area.x + 2, area.y + 2, area.width - 4, area.height - 4);
    let (title,text,scroll)=match overlay {
        Overlay::Address(text)=>("Inspect another address",format!("{}\n\nEnter confirms | Esc cancels\n{}",sanitize(&text),sanitize(state.notice.as_deref().unwrap_or("Paste a Base58 public address. No wallet connection."))),0),
        Overlay::Search(text)=>(if state.focus == Focus::Validators {"Search displayed validators"} else {"Search displayed accounts"},format!("{}\n\nSearch account / vote address / available validator name.\nEnter applies | Esc cancels. Summary totals are unchanged.",sanitize(&text)),0),
        Overlay::Sort if state.focus == Focus::Validators => ("Sort validators", "1 Recorded delegation descending\n2 Recorded delegation ascending\n3 Vote address ascending\n4 Available name ascending\nUnknown values always last; vote address breaks ties.\nChoose 1-4 | Esc cancels".into(), 0),
        Overlay::Sort=>("Sort accounts", "1 Balance descending (default)\n2 Balance ascending\n3 Account address ascending\n4 Validator vote address ascending\nUnknown amounts always last; account address breaks ties.\nChoose 1-4 | Esc cancels".into(),0),
        Overlay::Help=>("Help | j/k scroll | q or Esc close",format!("KEYS\nj/k or arrows: move; Tab/Shift-Tab: focus region\nEnter: expand/collapse validators; Esc: close input or clear search\nPageUp/PageDown/Home/End: navigate account list\nLeft/Right: epoch shown in the Account Detail chart, which follows the selected row; Tab to Account Detail, then j/k scroll its fields\n/: search displayed rows; s: labeled sort options\nr: manual refresh (disabled offline); a: inspect another address\nq: close help first, otherwise quit; Ctrl-C: quit from anywhere\nText entry: printable shortcuts insert text; paste supported\n\nFIELDS AND SCOPE\nAssociated balance: selected accounts, not proof of ownership.\nRecorded delegation overlaps balance; effective stake is Unknown.\nSOL amounts use nine exact decimal places; Unknown is not zero.\nRewards: selected current accounts, not lifetime wallet earnings.\nOnly recorded inflation rewards; no MEV, validator APY or fiat pricing.\nAnnualized account return estimate: per account and epoch from total pre-reward balance, a nominal two-day epoch; not validator or staking APY.\nGraph markers: - no data  ! failed  ? not queried  0 recorded zero. Gaps are not zero.\nGraph groups use the current validator; historical attribution is unverified.\nNo data differs from a recorded zero; coverage counts expose gaps.\nCurrent commission is not historical commission.\nSource ages are from individual observations; cached/stale is labeled.\nNo background polling: r refreshes manually.\n\nLOCAL DATABASE\n{}\nOnly public data is stored; API keys are never saved.",sanitize(&state.db_path.display().to_string())),state.detail_scroll),
    };
    frame.render_widget(Clear, popup);
    paragraph(
        frame,
        popup,
        title.into(),
        text,
        true,
        state.no_color,
        scroll,
    );
}
