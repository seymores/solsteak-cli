use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use ssteak::{
    cli::Options,
    domain::Report,
    tui::{State, draw, render::sanitize},
};
fn state() -> State {
    let mut state = State::new(
        &Options {
            address: "11111111111111111111111111111111".into(),
            json: false,
            offline: true,
            refresh: false,
            no_color: true,
        },
        "/tmp/observations.sqlite",
    );
    state.now = 1791360010;
    state.apply_report(
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap(),
    );
    state
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
#[test]
fn exact_amounts_warnings_and_resize_survive_in_monochrome() {
    let mut s = state();
    for (w, h) in [(120, 35), (80, 24), (60, 18), (120, 35)] {
        let text = frame(&mut s, w, h);
        assert!(text.contains("SolSteak"));
        if w < 80 {
            assert!(text.contains("minimum 80x24"));
            continue;
        }
        assert!(text.contains("18446744073.709551615"), "{text}");
        assert!(text.contains("Attention: 1 findings / 0 errors"));
        assert!(text.contains("age 10s"));
        assert!(text.contains("? help q quit"));
        assert!(text.contains("showing 1 of 1"));
        assert_eq!(s.selected, 0);
    }
    // No key is needed: the selected account's detail is always on screen.
    for (w, h) in [(120, 35), (80, 24)] {
        let text = frame(&mut s, w, h);
        assert!(text.contains("Account Detail"));
        assert!(text.contains("11111111111111111111111111111111"));
    }
    s.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    assert!(frame(&mut s, 120, 35).contains("/tmp/observations.sqlite"));
}
#[test]
fn virtualizes_thousand_rows_and_preserves_totals_under_search() {
    let mut s = state();
    let mut r: Report =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    let d = r.data.as_mut().unwrap();
    let base = d.accounts[0].clone();
    d.accounts = (0u32..1000)
        .map(|i| {
            let mut a = base.clone();
            a.address =
                bs58::encode([i.to_le_bytes().as_slice(), &[1u8; 28]].concat()).into_string();
            a.balance_lamports = Some(i.to_string());
            a
        })
        .collect();
    s.apply_report(r);
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
    let text = frame(&mut s, 80, 24);
    assert_eq!(s.selected, 999);
    assert!(text.contains("row 1000"));
    assert!(s.page_height < 24);
    let selected = s.selected_account().unwrap().address.clone();
    s.search = selected;
    s.rebuild_rows();
    let text = frame(&mut s, 80, 24);
    assert!(text.contains("showing 1 of 1000"));
    assert!(text.contains("18446744073.709551615"));
}
#[test]
fn failure_loading_and_control_text_are_visible_and_safe() {
    assert_eq!(sanitize("bad\x1b[2J\r\nname"), "bad[2Jname");
    let mut s = state();
    s.report = None;
    s.loading = true;
    let text = frame(&mut s, 80, 24);
    assert!(text.contains("Balance: Unknown"));
    let r = serde_json::from_str(include_str!("../docs/contracts/examples/error.json")).unwrap();
    s.apply_report(r);
    let text = frame(&mut s, 80, 24);
    assert!(text.contains("ERROR"));
    assert!(text.contains("retry"));
}
