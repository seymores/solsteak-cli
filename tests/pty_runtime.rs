//! A full dashboard launch remains responsive while offline storage work starts.
use std::process::Command;

#[test]
fn offline_dashboard_draws_then_quits_and_restores_terminal() {
    let home = std::env::temp_dir().join(format!("ssteak-pty-runtime-{}", std::process::id()));
    let output = Command::new("python3")
        .arg("-c")
        .arg(r#"
import os, pty, select, subprocess, sys, termios, time
master, slave = pty.openpty()
before = termios.tcgetattr(slave)
p = subprocess.Popen([sys.argv[1], '-a', sys.argv[2], '--offline'], stdin=slave, stdout=slave, stderr=slave, env={**os.environ, 'HOME':sys.argv[3], 'TERM':'xterm-256color', 'HELIUS_API_KEY':''})
data = b''
deadline = time.monotonic() + 5
sent = False
while p.poll() is None and time.monotonic() < deadline:
    if select.select([master], [], [], .05)[0]: data += os.read(master, 65536)
    if b'\x1b[?1049h' in data and not sent:
        os.write(master, b'q'); sent = True
if p.poll() is None: p.kill(); raise AssertionError('dashboard did not quit')
while select.select([master], [], [], .05)[0]:
    chunk=os.read(master,65536)
    if not chunk: break
    data += chunk
after = termios.tcgetattr(slave)
before[3] &= ~getattr(termios, 'PENDIN', 0); after[3] &= ~getattr(termios, 'PENDIN', 0)
assert p.returncode == 0, data
assert sent, data
assert after == before, (before, after)
for control in [b'\x1b[?1049h', b'\x1b[?1049l', b'\x1b[?25h', b'\x1b[?2004l']:
    assert control in data, (control, data)
"#)
        .arg(env!("CARGO_BIN_EXE_ssteak"))
        .arg("11111111111111111111111111111111")
        .arg(home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
