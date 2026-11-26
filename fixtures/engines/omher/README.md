# Recordings taken from oh-my-hermes

Recorded agent sessions that were made by the oh-my-hermes project, not by Poiesis. They are
test data: tests replay them, nothing runs them.

## Where they come from

- Repository: https://github.com/rlaope/oh-my-hermes (MIT)
- Commit: `b5cbcc97be7e110a3e5abad7a44f3ce79d5f1e9c`
- Source folders (under `apps/omher/src-tauri/tests/fixtures/`):
  - `acp/` gives `acp/gemini-signed-out.jsonl`, `qwen-signed-out.jsonl`, `kimi-signed-out.jsonl`,
    `copilot-signed-out.jsonl` and `cline-signed-out.jsonl`.
  - `real/` gives every `real/claude-*.jsonl` and `real/codex-*.jsonl` file. The files for their
    other agents (`pi`, `omp`, `omo`, `dsh`) are not copied.

## How they were converted

oh-my-hermes keeps only the agent's lines, one JSON object per line. Poiesis recordings wrap each
line as `{"dir":"out","t":<ms>,"msg":<line>}` (see `../opencode-2.1.0.jsonl`). This one-off script
did the conversion. The `t` values are made up (10 ms apart), because the originals have no times.

```python
# convert.py  usage: python -I convert.py <source.jsonl> <target.jsonl>
import json, sys, pathlib
src, dst = map(pathlib.Path, sys.argv[1:3])
rows = [l for l in src.read_text(encoding="utf-8").splitlines() if l.strip()]
with dst.open("w", encoding="utf-8", newline="\n") as f:
    for i, line in enumerate(rows):
        msg = json.loads(line)
        f.write(json.dumps({"dir": "out", "t": i * 10, "msg": msg}, ensure_ascii=False, separators=(",", ":")) + "\n")
```

## What is in them

- Agent output only. There are no `"in"` lines (what the client sent), and no `spawn` line.
- Their authors already replaced some values with `<removed>` (for example a `rate_limit_info`
  that is a string instead of an object). A parser must cope with that.
- The ACP files stop after the error of `session/new`: they show a signed-out start.

## Licence of the source

```
MIT License

Copyright (c) 2026 oh-my-hermes contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
