//! Renderer consumes only the current in-memory report and view state.
use super::{Focus, Overlay, State};
use crate::domain::{Account, format_sol};
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
    let secondary = 2
        + if state.validators_expanded { 3 } else { 0 }
        + if state.rewards_expanded { 3 } else { 0 };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
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
    let content = if state.detail {
        Layout::default()
            .direction(if area.width >= 120 {
                Direction::Horizontal
            } else {
                Direction::Vertical
            })
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(areas[3])
    } else {
        Layout::default()
            .constraints([Constraint::Percentage(100)])
            .split(areas[3])
    };
    render_accounts(frame, content[0], state);
    if state.detail {
        render_detail(frame, content[1], state);
    }
    render_secondary(frame, areas[4], state);
    let total = state
        .report
        .as_ref()
        .and_then(|r| r.data.as_ref())
        .map_or(0, |d| d.accounts.len());
    let footer = format!(
        "j/k move Tab focus Enter detail / find s sort r refresh a address ? help q quit\n{:?} | showing {} of {} | row {} | {}",
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
fn render_detail(frame: &mut Frame<'_>, area: Rect, state: &State) {
    let Some(a) = state.selected_account() else {
        return;
    };
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
                "Epoch {} reward: {} | latest attempt {}",
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
                sanitize(&r.latest_attempt)
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
    lines.push("Authorities do not establish ownership. Rewards cover selected current accounts, not lifetime earnings. Inflation only; no MEV or annualized yield.".into());
    paragraph(
        frame,
        area,
        "Account detail | j/k scroll | Esc close".into(),
        lines.join("\n"),
        true,
        state.no_color,
        state.detail_scroll,
    );
}
fn render_secondary(frame: &mut Frame<'_>, area: Rect, state: &State) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if state.validators_expanded { 4 } else { 1 }),
            Constraint::Min(1),
        ])
        .split(area);
    let data = state.report.as_ref().and_then(|r| r.data.as_ref());
    let validator_rows = state.validator_rows();
    for (index, focus, expanded, label) in [
        (
            0,
            Focus::Validators,
            state.validators_expanded,
            "Validators",
        ),
        (
            1,
            Focus::Rewards,
            state.rewards_expanded,
            "Per-epoch rewards",
        ),
    ] {
        let title = focused(
            state,
            focus,
            &format!(
                "{} {label} | Enter {}",
                if expanded { "[-]" } else { "[+]" },
                if expanded { "collapse" } else { "expand" }
            ),
        );
        if !expanded {
            frame.render_widget(Paragraph::new(title), parts[index]);
            continue;
        }
        let offset = if state.focus == focus {
            state.section_scroll
        } else {
            0
        };
        let lines = match (focus, data) {
            (Focus::Validators, Some(_)) => validator_rows
                .iter()
                .skip(offset.min(validator_rows.len().saturating_sub(1)))
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
                .collect::<Vec<_>>(),
            (Focus::Rewards, Some(d)) => d
                .rewards
                .iter()
                .skip(offset.min(d.rewards.len().saturating_sub(1)))
                .take(3)
                .map(|r| {
                    let c = state
                        .report
                        .as_ref()
                        .and_then(|r| r.coverage.as_ref())
                        .and_then(|c| c.rewards.iter().find(|c| c.epoch == r.epoch));
                    format!(
                        "Epoch {} recorded subtotal {} SOL | {}",
                        sanitize(&r.epoch),
                        amount(r.subtotal_lamports.as_deref()),
                        c.map(|c| format!(
                            "recorded {} / no data {} / failed {} / not queried {}",
                            c.recorded, c.no_data, c.failed, c.not_queried
                        ))
                        .unwrap_or_else(|| "Coverage Unknown".into())
                    )
                })
                .collect(),
            _ => vec![],
        };
        paragraph(
            frame,
            parts[index],
            title,
            if lines.is_empty() {
                "No data".into()
            } else {
                lines.join("\n")
            },
            state.focus == focus,
            state.no_color,
            0,
        );
    }
}
fn render_overlay(frame: &mut Frame<'_>, area: Rect, state: &State, overlay: Overlay) {
    let popup = Rect::new(area.x + 2, area.y + 2, area.width - 4, area.height - 4);
    let (title,text,scroll)=match overlay {
        Overlay::Address(text)=>("Inspect another address",format!("{}\n\nEnter confirms | Esc cancels\n{}",sanitize(&text),sanitize(state.notice.as_deref().unwrap_or("Paste a Base58 public address. No wallet connection."))),0),
        Overlay::Search(text)=>(if state.focus == Focus::Validators {"Search displayed validators"} else {"Search displayed accounts"},format!("{}\n\nSearch account / vote address / available validator name.\nEnter applies | Esc cancels. Summary totals are unchanged.",sanitize(&text)),0),
        Overlay::Sort if state.focus == Focus::Validators => ("Sort validators", "1 Recorded delegation descending\n2 Recorded delegation ascending\n3 Vote address ascending\n4 Available name ascending\nUnknown values always last; vote address breaks ties.\nChoose 1-4 | Esc cancels".into(), 0),
        Overlay::Sort=>("Sort accounts", "1 Balance descending (default)\n2 Balance ascending\n3 Account address ascending\n4 Validator vote address ascending\nUnknown amounts always last; account address breaks ties.\nChoose 1-4 | Esc cancels".into(),0),
        Overlay::Help=>("Help | j/k scroll | q or Esc close",format!("KEYS\nj/k or arrows: move; Tab/Shift-Tab: focus region\nEnter: expand/collapse; Esc: close detail/input or clear search\nPageUp/PageDown/Home/End: navigate account list\n/: search displayed rows; s: labeled sort options\nr: manual refresh (disabled offline); a: inspect another address\nq: close help/detail first, otherwise quit; Ctrl-C: quit from anywhere\nText entry: printable shortcuts insert text; paste supported\n\nFIELDS AND SCOPE\nAssociated balance: selected accounts, not proof of ownership.\nRecorded delegation overlaps balance; effective stake is Unknown.\nSOL amounts use nine exact decimal places; Unknown is not zero.\nRewards: selected current accounts, not lifetime wallet earnings.\nOnly recorded inflation rewards; no MEV, APY or fiat pricing.\nNo data differs from a recorded zero; coverage counts expose gaps.\nCurrent commission is not historical commission.\nSource ages are from individual observations; cached/stale is labeled.\nNo background polling: r refreshes manually.\n\nLOCAL DATABASE\n{}\nOnly public data is stored; API keys are never saved.",sanitize(&state.db_path.display().to_string())),state.detail_scroll),
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
