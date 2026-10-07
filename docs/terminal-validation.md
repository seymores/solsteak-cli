# Terminal validation

Automated checks use a real pseudo-terminal for normal quit, panic and SIGTERM
restoration, plus a full offline dashboard launch that sends `q` after alternate
screen entry. They verify raw-mode restoration, cursor/style/paste cleanup and
alternate-screen exit. Frame tests cover 120×35, 80×24, below-minimum resize,
monochrome rendering, loading/failure content and 1,000 virtualized rows.

The event loop polls at 50 ms and draws only on events or worker updates. Storage
and RPC work execute on a bounded worker channel, so drawing and navigation do not
query SQLite or Helius. Manual light/dark terminal-theme inspection remains a
release check because automated terminal buffers cannot know the user's theme.
