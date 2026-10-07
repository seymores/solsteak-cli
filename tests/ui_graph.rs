use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use ssteak::{
    cli::Options,
    domain::{Report, Validator},
    tui::{Focus, State, draw},
};

const V1: &str = "Vote1111111111111111111111111111111111111";
const V2: &str = "Vote2222222222222222222222222222222222222";

fn address(byte: u8) -> String {
    bs58::encode([byte; 32]).into_string()
}
/// Three accounts: two under the first validator, one under the second. Every
/// account repeats the partial example's 15-epoch mix of recorded and gap states.
fn report() -> Report {
    let mut r: Report =
        serde_json::from_str(include_str!("../docs/contracts/examples/partial.json")).unwrap();
    let data = r.data.as_mut().unwrap();
    let base = data.accounts[0].clone();
    data.accounts = [(1, V1), (2, V1), (3, V2)]
        .iter()
        .map(|(byte, vote)| {
            let mut a = base.clone();
            a.address = address(*byte);
            a.vote_address = Some((*vote).into());
            a.balance_lamports = Some((1000 - u32::from(*byte)).to_string());
            a
        })
        .collect();
    data.validators = [V1, V2]
        .iter()
        .map(|vote| Validator {
            vote_address: (*vote).into(),
            name: None,
            commission: Some(5),
            state: "current".into(),
            delegated_lamports: None,
            concentration_denominator_lamports: None,
            source_id: None,
        })
        .collect();
    for epoch in &mut data.rewards {
        let template = epoch.entries[0].clone();
        epoch.entries = (1..=3)
            .map(|byte| {
                let mut e = template.clone();
                e.address = address(byte);
                e
            })
            .collect();
    }
    data.comparisons = ssteak::rewards::compare(&data.rewards, &data.accounts)
        .unwrap()
        .0;
    r
}
fn state(no_color: bool) -> State {
    let mut state = State::new(
        &Options {
            address: address(1),
            json: false,
            offline: true,
            refresh: false,
            no_color,
        },
        "/tmp/observations.sqlite",
    );
    state.now = 1791360010;
    state.apply_report(report());
    state
}
fn key(state: &mut State, code: KeyCode) {
    state.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}
fn frame(state: &mut State, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, state)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
fn epoch(s: &State) -> String {
    let g = s.graph().unwrap();
    s.report.as_ref().unwrap().data.as_ref().unwrap().rewards[g.epoch_pos]
        .epoch
        .clone()
}

#[test]
fn chart_follows_the_selected_row_and_pair_keys_clamp() {
    let mut s = state(true);
    assert_eq!(s.graph().unwrap().account.address, address(1));
    assert_eq!(epoch(&s), "899");
    // Right is a newer pair and already newest; Left is older, and the oldest clamps at
    // the 15th current-period epoch rather than running into the previous period.
    key(&mut s, KeyCode::Right);
    assert_eq!(epoch(&s), "899");
    for _ in 0..3 {
        key(&mut s, KeyCode::Left);
    }
    assert_eq!(epoch(&s), "896");
    for _ in 0..40 {
        key(&mut s, KeyCode::Left);
    }
    assert_eq!(epoch(&s), "885");
    // Moving the table row changes the account but keeps the pair; no extra keys.
    key(&mut s, KeyCode::Down);
    let g = s.graph().unwrap();
    assert_eq!(g.account.address, s.selected_account().unwrap().address);
    assert_eq!(g.validator.unwrap().vote_address, V1);
    assert_eq!(epoch(&s), "885");
    key(&mut s, KeyCode::Down);
    assert_eq!(s.graph().unwrap().validator.unwrap().vote_address, V2);
    // Pair keys also work from the detail panel, which scrolls with j/k.
    key(&mut s, KeyCode::Tab);
    assert_eq!(s.focus, Focus::Detail);
    key(&mut s, KeyCode::Right);
    assert_eq!(epoch(&s), "886");
    key(&mut s, KeyCode::Char('j'));
    assert_eq!(s.detail_scroll, 1);
    // Other regions do not take the pair keys.
    key(&mut s, KeyCode::Tab);
    key(&mut s, KeyCode::Left);
    assert_eq!(epoch(&s), "886");
}

#[test]
fn pair_selection_survives_refresh_and_address_change_resets_it() {
    let mut s = state(true);
    for _ in 0..4 {
        key(&mut s, KeyCode::Left);
    }
    assert_eq!(epoch(&s), "895");
    s.apply_report(report());
    assert_eq!(epoch(&s), "895");
    // The window slides by one on rollover: the selected pair keeps its identity.
    let mut slid = report();
    slid.data.as_mut().unwrap().rewards.remove(0);
    s.apply_report(slid);
    assert_eq!(epoch(&s), "895");
    // A remembered epoch that left the window, or moved into the previous period,
    // falls back to the newest pair.
    s.graph_epoch = Some("1".into());
    assert_eq!(epoch(&s), "898");
    s.graph_epoch = Some("880".into());
    assert_eq!(epoch(&s), "898");
    key(&mut s, KeyCode::Char('a'));
    s.handle_event(Event::Paste(address(7)));
    key(&mut s, KeyCode::Enter);
    assert!(s.graph_epoch.is_none());
}

#[test]
fn paired_chart_is_always_in_account_detail_at_supported_sizes_in_monochrome() {
    for (w, h) in [(80, 24), (120, 35)] {
        let mut s = state(true);
        // No key presses: the chart is on screen for the selected row.
        let text = frame(&mut s, w, h);
        assert!(text.contains("Account Detail"), "{text}");
        assert!(text.contains("Validator Vote11"), "{text}");
        assert!(text.contains("Change vs previous 15 epochs"), "{text}");
        assert!(text.contains("9 of 15 pairs compared"), "{text}");
        assert!(text.contains("Pair 899 vs 884: not compared"), "{text}");
        assert!(!text.contains("15-Epoch Rewards"));
        // Both periods are drawn with different glyphs, so no color is needed.
        assert!(text.contains('░') && text.contains('█'), "{text}");
        // Gap markers share the bottom bar row; there is exactly one selection caret.
        let rows: Vec<&str> = text.lines().collect();
        let caret = rows.iter().position(|l| l.contains('^')).unwrap();
        let bottom = rows[caret - 1];
        for marker in ['-', '!', '?'] {
            assert!(bottom.contains(marker), "missing {marker:?}\n{text}");
        }
        assert_eq!(text.matches('^').count(), 1, "{text}");
        if h >= 35 {
            // Tall panels also show the legend, latest estimate and attribution.
            assert!(text.contains("Legend:"), "{text}");
            assert!(text.contains("Latest completed epoch 899"), "{text}");
            assert!(
                text.contains("Historical validator attribution unverified"),
                "{text}"
            );
        }
        // Pair 897 vs 882: the current epoch failed, so the pair is left out.
        for _ in 0..2 {
            key(&mut s, KeyCode::Left);
        }
        let text = frame(&mut s, w, h);
        assert!(text.contains("Pair 897 vs 882"), "{text}");
        assert!(text.contains("request failed"), "{text}");
        assert!(text.contains("2682000 lamports"), "{text}");
        // Pair 898 vs 883 is compared: exact signed lamport difference.
        key(&mut s, KeyCode::Right);
        let text = frame(&mut s, w, h);
        assert!(
            text.contains("Pair 898 vs 883: difference +215000 lamports"),
            "{text}"
        );
        // Selected row and chart stay in step.
        key(&mut s, KeyCode::Down);
        let text = frame(&mut s, w, h);
        assert!(
            text.contains(&format!("Account Detail | {}", short_address(&s))),
            "{text}"
        );
        assert!(text.contains("Pair 898 vs 883"), "{text}");
    }
}
fn short_address(s: &State) -> String {
    let a = &s.selected_account().unwrap().address;
    format!("{}..{}", &a[..6], &a[a.len() - 4..])
}

#[test]
fn short_chains_have_no_previous_bars_and_unknown_change() {
    let mut s = state(true);
    for (example, wanted) in [
        (
            include_str!("../docs/contracts/examples/short-window.json"),
            "Unknown (no previous period)",
        ),
        (
            include_str!("../docs/contracts/examples/short-previous.json"),
            "5 of 5 pairs compared",
        ),
    ] {
        s.apply_report(serde_json::from_str(example).unwrap());
        let text = frame(&mut s, 120, 35);
        assert!(text.contains(wanted), "{text}");
    }
}

#[test]
fn account_fields_are_reachable_by_scrolling_the_detail_panel() {
    let mut s = state(true);
    let top = frame(&mut s, 80, 24);
    assert!(!top.contains("Relationship:"), "{top}");
    key(&mut s, KeyCode::Tab);
    key(&mut s, KeyCode::End);
    let text = frame(&mut s, 80, 24);
    assert!(
        text.contains("Authorities do not establish ownership"),
        "{text}"
    );
    // The scroll offset is clamped so the panel never goes blank.
    assert!(s.detail_scroll < 200);
}

#[test]
fn loading_and_undelegated_states_do_not_panic() {
    let mut s = state(true);
    s.report = None;
    s.loading = true;
    assert!(frame(&mut s, 80, 24).contains("Loading..."));
    key(&mut s, KeyCode::Left);
    let mut direct: Report =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    direct.data.as_mut().unwrap().stale = true;
    s.apply_report(direct);
    s.loading = false;
    let text = frame(&mut s, 80, 24);
    assert!(text.contains("Validator none"), "{text}");
    assert!(text.contains("Change vs previous 15 epochs"), "{text}");
}
