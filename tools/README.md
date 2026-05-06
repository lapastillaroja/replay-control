# Tools

Development tools for replay-control. Not part of the app itself.

## Performance

| Script | Description |
|--------|-------------|
| `bench.sh` | Performance benchmark: TTFB, asset sizes, Lighthouse scores, light load test. Outputs JSON to `bench-results/`. |
| `load-test.sh` | Stress test with Apache Bench: sweeps concurrency 1-30 across 8 endpoints + mixed concurrent test. |
| `pi-cpu.sh` | Sample replay-control CPU% on the Pi over SSH. `--browse` adds a single-user browse load; `--json` for machine-readable output. |
| `pi-memory.sh` | Read VmRSS / VmHWM / RssAnon for replay-control on the Pi. `--restart` for a clean idle baseline. |

## Game Launching

| Script | Description |
|--------|-------------|
| `game_launch_autostart.py` | Launch a game on RePlayOS via autostart file + service restart. Production method used by the app. |

## Assets

| Script | Description |
|--------|-------------|
| `resize-system-icons.py` | Resize and center system controller icons from 300x300 originals. |

## Misc

| Script | Description |
|--------|-------------|
| `count-lines.sh` | Count Rust lines of code, separating production from inline test code. |
| `generate-test-fixtures/` | Rust crate that generates test fixture data. |
