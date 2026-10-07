//! Pure input transitions. Worker events carry a generation; drawing never starts work.
use std::path::PathBuf;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::{
    cli::Options,
    domain::{Account, Report, Validator, validate_address},
    rewards::PERIOD,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Refresh,
    Address(String),
    Quit(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Accounts,
    Attention,
    Validators,
    Detail,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    BalanceDescending,
    BalanceAscending,
    Address,
    Validator,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Address(String),
    Search(String),
    Sort,
}

/// The chart's subject: the selected table row, its current validator and the epoch.
pub struct Graph<'a> {
    pub account: &'a Account,
    pub validator: Option<&'a Validator>,
    /// Selected pair: an index into `data.rewards` within the current period (0 newest).
    pub epoch_pos: usize,
    /// Number of rewards positions in the window (up to 30).
    pub epochs: usize,
    /// Number of pair columns: the current period's length (up to 15).
    pub columns: usize,
}

pub struct State {
    pub address: String,
    pub offline: bool,
    pub no_color: bool,
    pub db_path: PathBuf,
    pub report: Option<Report>,
    pub loading: bool,
    pub generation: u64,
    pub notice: Option<String>,
    pub focus: Focus,
    pub overlay: Option<Overlay>,
    pub detail_scroll: u16,
    pub validators_expanded: bool,
    /// Selected epoch number, kept by identity so refresh and rollover preserve it.
    pub graph_epoch: Option<String>,
    pub selected: usize,
    pub offset: usize,
    pub page_height: usize,
    pub search: String,
    pub validator_search: String,
    pub validator_sort: Sort,
    pub sort: Sort,
    pub section_scroll: usize,
    pub now: u64,
    pub(crate) rows: Vec<usize>,
}

impl State {
    pub fn new(options: &Options, db_path: impl Into<PathBuf>) -> Self {
        Self {
            address: options.address.clone(),
            offline: options.offline,
            no_color: options.no_color,
            db_path: db_path.into(),
            report: None,
            loading: true,
            generation: 0,
            notice: None,
            focus: Focus::Accounts,
            overlay: None,
            detail_scroll: 0,
            validators_expanded: false,
            graph_epoch: None,
            selected: 0,
            offset: 0,
            page_height: 10,
            search: String::new(),
            validator_search: String::new(),
            validator_sort: Sort::BalanceDescending,
            sort: Sort::BalanceDescending,
            section_scroll: 0,
            now: 0,
            rows: vec![],
        }
    }
    pub fn selected_account(&self) -> Option<&Account> {
        self.report
            .as_ref()?
            .data
            .as_ref()?
            .accounts
            .get(*self.rows.get(self.selected)?)
    }
    pub fn visible_count(&self) -> usize {
        self.rows.len()
    }
    pub fn begin_refresh(&mut self) -> bool {
        if self.offline {
            self.notice =
                Some("Offline: refresh disabled. Relaunch online to fetch observations.".into());
            return false;
        }
        if self.loading {
            return false;
        }
        self.loading = true;
        self.generation = self.generation.wrapping_add(1);
        self.notice = None;
        true
    }
    pub fn apply_generation(&mut self, generation: u64, report: Report) -> bool {
        if generation != self.generation {
            return false;
        }
        self.apply_report(report);
        true
    }
    pub fn apply_report(&mut self, mut report: Report) {
        let selected = self.selected_account().map(|a| a.address.clone());
        if report.data.is_none()
            && let Some(previous) = self.report.as_ref()
        {
            report.data = previous.data.clone();
            if let Some(data) = report.data.as_mut() {
                data.stale = true;
            }
            report.sources = previous.sources.clone();
            report.coverage = previous.coverage.clone();
            report.warnings.extend(previous.warnings.clone());
        }
        self.loading = false;
        self.report = Some(report);
        self.rebuild_rows();
        if let Some(address) = selected {
            let accounts = &self
                .report
                .as_ref()
                .unwrap()
                .data
                .as_ref()
                .unwrap()
                .accounts;
            if let Some(index) = self
                .rows
                .iter()
                .position(|&i| accounts[i].address == address)
            {
                self.selected = index;
            } else {
                self.notice = Some(
                    "Selected account is no longer in the displayed set; moved to nearest row."
                        .into(),
                );
            }
        }
        self.ensure_visible();
    }
    pub fn rebuild_rows(&mut self) {
        self.rows.clear();
        if let Some(data) = self.report.as_ref().and_then(|r| r.data.as_ref()) {
            let needle = self.search.to_lowercase();
            self.rows.extend(
                data.accounts
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        a.address.to_lowercase().contains(&needle)
                            || a.vote_address
                                .as_ref()
                                .is_some_and(|v| v.to_lowercase().contains(&needle))
                            || data.validators.iter().any(|v| {
                                a.vote_address.as_ref() == Some(&v.vote_address)
                                    && v.name
                                        .as_ref()
                                        .is_some_and(|n| n.to_lowercase().contains(&needle))
                            })
                    })
                    .map(|(i, _)| i),
            );
            self.rows.sort_by(|&a, &b| {
                let (a, b) = (&data.accounts[a], &data.accounts[b]);
                let order = match self.sort {
                    Sort::Address => a.address.cmp(&b.address),
                    Sort::Validator => match (&a.vote_address, &b.vote_address) {
                        (Some(a), Some(b)) => a.cmp(b),
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, None) => std::cmp::Ordering::Equal,
                    },
                    Sort::BalanceAscending | Sort::BalanceDescending => {
                        let a = a
                            .balance_lamports
                            .as_ref()
                            .and_then(|v| v.parse::<u128>().ok());
                        let b = b
                            .balance_lamports
                            .as_ref()
                            .and_then(|v| v.parse::<u128>().ok());
                        match (a, b) {
                            (Some(a), Some(b)) => {
                                if self.sort == Sort::BalanceAscending {
                                    a.cmp(&b)
                                } else {
                                    b.cmp(&a)
                                }
                            }
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    }
                };
                order.then_with(|| a.address.cmp(&b.address))
            });
        }
        self.ensure_visible();
    }
    pub fn validator_rows(&self) -> Vec<&crate::domain::Validator> {
        let Some(data) = self.report.as_ref().and_then(|r| r.data.as_ref()) else {
            return vec![];
        };
        let needle = self.validator_search.to_lowercase();
        let mut rows: Vec<_> = data
            .validators
            .iter()
            .filter(|v| {
                v.vote_address.to_lowercase().contains(&needle)
                    || v.name
                        .as_ref()
                        .is_some_and(|n| n.to_lowercase().contains(&needle))
            })
            .collect();
        rows.sort_by(|a, b| {
            let order = match self.validator_sort {
                Sort::Address => a.vote_address.cmp(&b.vote_address),
                Sort::Validator => match (&a.name, &b.name) {
                    (Some(a), Some(b)) => a.cmp(b),
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Equal,
                },
                _ => match (
                    a.delegated_lamports
                        .as_ref()
                        .and_then(|a| a.parse::<u128>().ok()),
                    b.delegated_lamports
                        .as_ref()
                        .and_then(|b| b.parse::<u128>().ok()),
                ) {
                    (Some(a), Some(b)) => {
                        if self.validator_sort == Sort::BalanceAscending {
                            a.cmp(&b)
                        } else {
                            b.cmp(&a)
                        }
                    }
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Equal,
                },
            };
            order.then_with(|| a.vote_address.cmp(&b.vote_address))
        });
        rows
    }
    /// Resolve the chart subject from memory; never does I/O.
    pub fn graph(&self) -> Option<Graph<'_>> {
        let data = self.report.as_ref()?.data.as_ref()?;
        let account = self.selected_account()?;
        let validator = data
            .validators
            .iter()
            .find(|v| account.vote_address.as_deref() == Some(&v.vote_address));
        let epoch_pos = self
            .graph_epoch
            .as_ref()
            .and_then(|id| data.rewards.iter().position(|r| &r.epoch == id))
            // After a rollover a remembered epoch may have moved to the previous period.
            .filter(|&pos| pos < PERIOD)
            .unwrap_or(0);
        Some(Graph {
            account,
            validator,
            epoch_pos,
            epochs: data.rewards.len(),
            columns: data.rewards.len().min(PERIOD),
        })
    }
    /// Left is an older pair, Right a newer one; clamped, no wrap.
    fn epoch_key(&mut self, code: KeyCode) {
        let Some(view) = self.graph() else {
            return;
        };
        let pos = if code == KeyCode::Left {
            (view.epoch_pos + 1).min(view.columns.saturating_sub(1))
        } else {
            view.epoch_pos.saturating_sub(1)
        };
        self.graph_epoch = self
            .report
            .as_ref()
            .and_then(|r| r.data.as_ref())
            .and_then(|d| d.rewards.get(pos))
            .map(|r| r.epoch.clone());
    }
    pub fn ensure_visible(&mut self) {
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.offset = self.offset.min(self.selected);
        if self.selected >= self.offset + self.page_height.max(1) {
            self.offset = self.selected + 1 - self.page_height.max(1);
        }
    }
    pub fn handle_event(&mut self, event: Event) -> Action {
        if let Event::Paste(text) = &event {
            if let Some(Overlay::Address(input) | Overlay::Search(input)) = &mut self.overlay {
                input.extend(
                    text.chars()
                        .filter(|c| !c.is_control())
                        .take(256usize.saturating_sub(input.chars().count())),
                );
            }
            return Action::None;
        }
        let Event::Key(key) = event else {
            return Action::None;
        };
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit(130);
        }
        if let Some(overlay) = &mut self.overlay {
            match overlay {
                Overlay::Address(input) | Overlay::Search(input) => match key.code {
                    KeyCode::Esc => {
                        self.overlay = None;
                        self.notice = None;
                    }
                    KeyCode::Enter => {
                        let text = input.trim().to_owned();
                        if matches!(self.overlay, Some(Overlay::Address(_))) {
                            if !validate_address(&text) {
                                self.notice =
                                    Some("Enter a Base58 address decoding to 32 bytes.".into());
                            } else {
                                self.address = text.clone();
                                self.report = None;
                                self.rows.clear();
                                self.selected = 0;
                                self.offset = 0;
                                self.search.clear();
                                self.validator_search.clear();
                                self.validators_expanded = false;
                                self.graph_epoch = None;
                                self.section_scroll = 0;
                                self.overlay = None;
                                self.notice = None;
                                self.generation = self.generation.wrapping_add(1);
                                self.loading = true;
                                return Action::Address(text);
                            }
                        } else {
                            if self.focus == Focus::Validators {
                                self.validator_search = text;
                                self.section_scroll = 0;
                            } else {
                                self.search = text;
                            }
                            self.overlay = None;
                            self.selected = 0;
                            self.rebuild_rows();
                        }
                    }
                    KeyCode::Backspace => {
                        input.pop();
                    }
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && !c.is_control()
                            && input.chars().count() < 256 =>
                    {
                        input.push(c)
                    }
                    _ => {}
                },
                Overlay::Sort => match key.code {
                    KeyCode::Char('1'..='4') => {
                        let sort = match key.code {
                            KeyCode::Char('1') => Sort::BalanceDescending,
                            KeyCode::Char('2') => Sort::BalanceAscending,
                            KeyCode::Char('3') => Sort::Address,
                            _ => Sort::Validator,
                        };
                        if self.focus == Focus::Validators {
                            self.validator_sort = sort;
                            self.section_scroll = 0;
                        } else {
                            self.sort = sort;
                        }
                        self.overlay = None;
                        self.rebuild_rows();
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
                    _ => {}
                },
                Overlay::Help => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                        self.overlay = None;
                        self.detail_scroll = 0;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.detail_scroll = self.detail_scroll.saturating_add(1)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.detail_scroll = self.detail_scroll.saturating_sub(1)
                    }
                    _ => {}
                },
            }
            return Action::None;
        }
        match key.code {
            KeyCode::Char('q') => return Action::Quit(0),
            KeyCode::Esc => {
                if self.focus == Focus::Validators {
                    self.validator_search.clear();
                } else {
                    self.search.clear();
                }
                self.rebuild_rows();
            }
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                self.detail_scroll = 0;
            }
            KeyCode::Char('a') => self.overlay = Some(Overlay::Address(String::new())),
            KeyCode::Char('/') | KeyCode::Char('s') => {
                if matches!(self.focus, Focus::Accounts | Focus::Validators) {
                    self.overlay = Some(if key.code == KeyCode::Char('s') {
                        Overlay::Sort
                    } else {
                        Overlay::Search(if self.focus == Focus::Validators {
                            self.validator_search.clone()
                        } else {
                            self.search.clone()
                        })
                    });
                } else {
                    self.notice = Some("Search and sort are available in Accounts and Validators; Tab changes focus.".into());
                }
            }
            KeyCode::Char('r') => {
                if self.begin_refresh() {
                    return Action::Refresh;
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let focuses = [
                    Focus::Accounts,
                    Focus::Detail,
                    Focus::Attention,
                    Focus::Validators,
                ];
                let index = focuses.iter().position(|f| *f == self.focus).unwrap_or(0);
                let backwards =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.focus = focuses[(index + if backwards { 3 } else { 1 }) % 4];
                self.section_scroll = 0;
            }
            KeyCode::Enter => {
                if self.focus == Focus::Validators {
                    self.validators_expanded = !self.validators_expanded;
                }
            }
            // The chart follows the selected row, so epochs move from table or detail focus.
            KeyCode::Left | KeyCode::Right
                if matches!(self.focus, Focus::Accounts | Focus::Detail) =>
            {
                self.epoch_key(key.code)
            }
            KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End => {
                let count = if matches!(key.code, KeyCode::PageDown | KeyCode::PageUp) {
                    self.page_height.max(1)
                } else {
                    1
                };
                let down = matches!(
                    key.code,
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::PageDown
                );
                if self.focus == Focus::Detail {
                    self.detail_scroll = if key.code == KeyCode::Home {
                        0
                    } else if key.code == KeyCode::End {
                        u16::MAX
                    } else if down {
                        self.detail_scroll.saturating_add(count as u16)
                    } else {
                        self.detail_scroll.saturating_sub(count as u16)
                    };
                } else if self.focus == Focus::Accounts {
                    self.selected = match key.code {
                        KeyCode::Home => 0,
                        KeyCode::End => self.rows.len().saturating_sub(1),
                        _ if down => self.selected.saturating_add(count),
                        _ => self.selected.saturating_sub(count),
                    };
                    self.detail_scroll = 0;
                    self.ensure_visible();
                } else {
                    self.section_scroll = if key.code == KeyCode::Home {
                        0
                    } else if key.code == KeyCode::End {
                        u16::MAX as usize
                    } else if down {
                        self.section_scroll.saturating_add(count)
                    } else {
                        self.section_scroll.saturating_sub(count)
                    };
                }
            }
            _ => {}
        }
        Action::None
    }
}
