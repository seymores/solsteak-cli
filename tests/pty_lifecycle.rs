//! A real pseudo-terminal verifies termios and emitted cleanup commands.
use ssteak::tui::Session;
use std::{process::Command, time::Duration};
#[test]
#[ignore = "subprocess helper"]
fn session_child() {
    let mode = std::env::var("SSTEAK_PTY_MODE").unwrap();
    let outcome = std::panic::catch_unwind(|| {
        let session = Session::enter().unwrap();
        println!("SESSION_READY");
        if mode == "panic" {
            panic!("fixture panic");
        }
        if mode == "signal" {
            while session.interrupted().is_none() {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(session.interrupted(), Some(143));
        }
    });
    assert_eq!(outcome.is_err(), mode == "panic");
    println!("SESSION_DONE");
    std::thread::sleep(Duration::from_millis(300));
}
#[test]
fn restores_real_terminal_after_quit_panic_and_signal() {
    let result = Command::new("python3").arg("-c").arg(r#"
import os, pty, subprocess, sys, termios, select, signal, time, fcntl
for mode in ['quit', 'panic', 'signal']:
    master, slave = pty.openpty()
    before = termios.tcgetattr(slave)
    def attach():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    p = subprocess.Popen([sys.argv[1], '--ignored', '--exact', 'session_child', '--nocapture'], stdin=slave, stdout=slave, stderr=slave, env={**os.environ, 'SSTEAK_PTY_MODE':mode}, preexec_fn=attach)
    output = b''
    deadline = time.monotonic() + 10
    sent = False
    restored = False
    while p.poll() is None and time.monotonic() < deadline:
        if select.select([master], [], [], .05)[0]:
            output += os.read(master, 65536)
        if b'SESSION_DONE' in output and not restored:
            after = termios.tcgetattr(slave)
            # Darwin may set PENDIN when restoring canonical input.
            before[3] &= ~getattr(termios, 'PENDIN', 0)
            after[3] &= ~getattr(termios, 'PENDIN', 0)
            assert after == before, (mode, before, after)
            restored = True
        if mode == 'signal' and b'SESSION_READY' in output and not sent:
            p.send_signal(signal.SIGTERM); sent = True
    if p.poll() is None: p.kill(); raise AssertionError('terminal child hung')
    while select.select([master], [], [], .05)[0]:
        chunk = os.read(master, 65536)
        if not chunk: break
        output += chunk
    assert p.returncode == 0, output
    assert restored, output
    for sequence in [b'\x1b[?1049l', b'\x1b[?25h', b'\x1b[?2004l', b'\x1b[0m']:
        assert sequence in output, (mode, sequence, output)
    os.close(master); os.close(slave)
"#).arg(std::env::current_exe().unwrap()).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
