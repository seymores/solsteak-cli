use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ssteak::{
    cli::Options,
    domain::Report,
    tui::{Action, Overlay, State},
};
fn state() -> State {
    State::new(
        &Options {
            address: "11111111111111111111111111111111".into(),
            json: false,
            offline: false,
            refresh: false,
            no_color: true,
        },
        "/tmp/observations.sqlite",
    )
}
fn report() -> Report {
    serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap()
}
fn key(state: &mut State, code: KeyCode) -> Action {
    state.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}
#[test]
fn text_shortcuts_generation_and_overlay_precedence() {
    let mut s = state();
    s.apply_report(report());
    key(&mut s, KeyCode::Char('a'));
    for c in "qrsa?".chars() {
        assert_eq!(key(&mut s, KeyCode::Char(c)), Action::None);
    }
    assert!(matches!(&s.overlay, Some(Overlay::Address(v)) if v == "qrsa?"));
    key(&mut s, KeyCode::Enter);
    assert!(s.notice.is_some());
    assert!(s.report.is_some());
    key(&mut s, KeyCode::Esc);
    key(&mut s, KeyCode::Char('a'));
    s.handle_event(Event::Paste("11111111111111111111111111111111".into()));
    assert!(matches!(key(&mut s, KeyCode::Enter), Action::Address(_)));
    assert_eq!(s.generation, 1);
    assert!(s.report.is_none());
    assert!(!s.apply_generation(0, report()));
    assert!(s.apply_generation(1, report()));
    // Account detail is always visible, so Enter does nothing on the table.
    assert_eq!(key(&mut s, KeyCode::Enter), Action::None);
    key(&mut s, KeyCode::Char('?'));
    assert_eq!(key(&mut s, KeyCode::Char('q')), Action::None);
    assert_eq!(key(&mut s, KeyCode::Char('q')), Action::Quit(0));
    assert_eq!(
        s.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ))),
        Action::Quit(130)
    );
}
#[test]
fn thousand_rows_selection_search_sort_and_failed_refresh() {
    let mut s = state();
    let mut r = report();
    let data = r.data.as_mut().unwrap();
    let account = data.accounts[0].clone();
    data.accounts = (0..1000)
        .map(|i| {
            let mut a = account.clone();
            a.address = bs58::encode([i as u8; 32]).into_string() + &format!("{i:04}");
            a.balance_lamports = Some(i.to_string());
            a
        })
        .collect();
    s.apply_report(r.clone());
    assert_eq!(
        s.selected_account().unwrap().balance_lamports.as_deref(),
        Some("999")
    );
    key(&mut s, KeyCode::End);
    assert_eq!(s.selected, 999);
    assert_eq!(s.offset, 990);
    let address = s.selected_account().unwrap().address.clone();
    assert_eq!(key(&mut s, KeyCode::Char('r')), Action::Refresh);
    assert_eq!(key(&mut s, KeyCode::Char('r')), Action::None);
    r.data.as_mut().unwrap().accounts.reverse();
    s.apply_report(r);
    assert_eq!(s.selected_account().unwrap().address, address);
    key(&mut s, KeyCode::Char('/'));
    s.handle_event(Event::Paste(address));
    key(&mut s, KeyCode::Enter);
    assert_eq!(s.visible_count(), 1);
    let failed: Report =
        serde_json::from_str(include_str!("../docs/contracts/examples/error.json")).unwrap();
    s.apply_report(failed);
    assert!(s.report.as_ref().unwrap().data.as_ref().unwrap().stale);
    s.offline = true;
    assert_eq!(key(&mut s, KeyCode::Char('r')), Action::None);
    assert!(s.notice.as_ref().unwrap().contains("Offline"));
}

#[test]
fn validator_filter_and_sort_do_not_change_account_totals_or_filter() {
    use ssteak::{domain::Validator, tui::Focus};
    let mut s = state();
    let mut r = report();
    r.data.as_mut().unwrap().validators = vec![
        Validator {
            vote_address: "vote-a".into(),
            name: Some("Alpha".into()),
            commission: None,
            state: "unknown".into(),
            delegated_lamports: None,
            concentration_denominator_lamports: None,
            source_id: None,
        },
        Validator {
            vote_address: "vote-b".into(),
            name: Some("Beta".into()),
            commission: Some(5),
            state: "current".into(),
            delegated_lamports: Some("0".into()),
            concentration_denominator_lamports: Some("0".into()),
            source_id: None,
        },
    ];
    s.apply_report(r);
    s.focus = Focus::Validators;
    key(&mut s, KeyCode::Char('s'));
    key(&mut s, KeyCode::Char('2'));
    assert_eq!(s.validator_rows()[0].vote_address, "vote-b");
    key(&mut s, KeyCode::Char('/'));
    s.handle_event(Event::Paste("alpha".into()));
    key(&mut s, KeyCode::Enter);
    assert_eq!(s.validator_rows().len(), 1);
    assert_eq!(s.visible_count(), 1);
    assert!(s.search.is_empty());
    assert_eq!(
        s.report
            .as_ref()
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .summary
            .balance_lamports
            .as_deref(),
        Some("18446744073709551615")
    );
}
