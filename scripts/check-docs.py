#!/usr/bin/env python3
"""Checks the markdown docs against the repo (stdlib only; run from anywhere):

  (a) relative links `[x](path#anchor)` resolve, and the #anchor exists in the target
  (b) backtick paths that look like repo paths exist (names that are generated or historical go in scripts/check-docs-allow.txt)
  (c) every `rail-extract <cmd> --flag` and `$RE <cmd> --flag` in the current docs exists in `rail-extract <cmd> --help`
      (skipped, with a note, when the binary is not built; pass --no-binary to skip it on purpose)

Exit code 1 on any failure. Docs that describe the past are not checked for (c): docs/archive and docs/reference/pipeline-timings.md.
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ALLOW_FILE = os.path.join(ROOT, 'scripts', 'check-docs-allow.txt')
BINARIES = ['pipeline/rail-extract/target/release/rail-extract', 'pipeline/rail-extract/target/debug/rail-extract']
NO_CMD_CHECK = ('docs/archive/', 'docs/reference/pipeline-timings.md')
# a backtick token is a path candidate when it ends with one of these, or with a slash
EXTS = ('.md', '.rs', '.sh', '.py', '.json', '.mjs', '.ts', '.tsx', '.html', '.txt', '.toml', '.geojson', '.geojsonl', '.yml', '.css')
# directories a doc may name a file relative to, besides its own folder and the repo root
BASES = ['docs', 'pipeline', 'pipeline/rail-extract', 'pipeline/rail-extract/src', 'pipeline/config', 'app', 'app/src', 'r2', 'shared']
# never in the repo: ignored data and local folders
SKIP_PREFIX = ('data/', 'app/public/data', 'http', '/', '~', '.omc', '.claude', 'node_modules', 'target/', 'dist/', '--')


def sh(*a):
    return subprocess.run(a, cwd=ROOT, capture_output=True, text=True).stdout


def read(p):
    with open(os.path.join(ROOT, p), encoding='utf-8') as f:
        return f.read()


def load_allow():
    exact, prefix = set(), []
    if os.path.exists(ALLOW_FILE):
        for line in read('scripts/check-docs-allow.txt').splitlines():
            line = line.split('  #')[0].strip()
            if not line or line.startswith('#'):
                continue
            if line.endswith('*'):
                prefix.append(line[:-1])
            else:
                exact.add(line)
    return exact, prefix


def strip_code_fences(text):
    return re.sub(r'^```.*?^```', lambda m: '\n' * m.group(0).count('\n'), text, flags=re.S | re.M)


def slug(h):
    h = re.sub(r'`', '', h).strip().lower()
    return re.sub(r'[^\w\- ]', '', h).replace(' ', '-')


_anchors = {}


def anchors(p):
    if p not in _anchors:
        t = strip_code_fences(read(p))
        a = set(re.findall(r'<a id="([^"]+)"', t))
        seen = {}
        for h in re.findall(r'^#+\s+(.*)$', t, re.M):
            s = slug(h)
            n = seen.get(s, 0)
            a.add(s if n == 0 else f'{s}-{n}')
            seen[s] = n + 1
        _anchors[p] = a
    return _anchors[p]


def check_links(f, text, errs):
    for m in re.finditer(r'\]\(([^)\s]*)\)', strip_code_fences(text)):
        u = m.group(1)
        if u.startswith(('http', 'mailto')) or not u:
            continue
        path, _, frag = u.partition('#')
        tgt = os.path.normpath(os.path.join(os.path.dirname(f), path)) if path else f
        if not os.path.exists(os.path.join(ROOT, tgt)):
            errs.append(f'{f}: link target missing: {u}')
        elif frag and tgt.endswith('.md') and frag not in anchors(tgt):
            errs.append(f'{f}: anchor missing: {u}')


_TRACKED = None


def known_path(p):
    """`p` (repo-relative) is a tracked file, a folder holding tracked files, or a path git ignores (build output)."""
    global _TRACKED
    if _TRACKED is None:
        files = sh('git', '-C', ROOT, 'ls-files').split('\n')
        _TRACKED = set(files)
        for x in files:
            d = os.path.dirname(x)
            while d:
                _TRACKED.add(d)
                d = os.path.dirname(d)
    if p.rstrip('/') in _TRACKED:
        return True
    return subprocess.run(['git', '-C', ROOT, 'check-ignore', '-q', '--no-index', p], capture_output=True).returncode == 0


def check_paths(f, text, files, basenames, allow, errs):
    exact, prefix = allow
    for t in sorted(set(re.findall(r'`([^`\s]+)`', strip_code_fences(text)))):
        t = t.rstrip('.,:;')
        if not (t.endswith(EXTS) or t.endswith('/')) or t.startswith(SKIP_PREFIX):
            continue
        if any(c in t for c in '<>*{}$|=()[]') or t in exact or any(t.startswith(p) for p in prefix):
            continue
        t = t.split(':')[0] if re.search(r':\d+(-\d+)?$', t) else t
        # normpath drops a trailing slash, which git needs to match a folder-only ignore rule (app/dist/)
        cands = [os.path.normpath(os.path.join(b, t)) + ('/' if t.endswith('/') else '') for b in [os.path.dirname(f), ''] + BASES]
        # tracked by git, or a generated path git ignores: the same answer on a fresh clone (CI) as locally
        if any(known_path(c) for c in cands):
            continue
        if '/' not in t and t in basenames:  # a bare file name: any tracked file of that name
            continue
        errs.append(f'{f}: path not in the repo: {t}')


def find_binary():
    env = os.environ.get('RAIL_EXTRACT')
    for b in ([env] if env else []) + [os.path.join(ROOT, p) for p in BINARIES]:
        if b and os.path.isfile(b):
            return b
    return None


def check_commands(files, errs):
    binary = find_binary()
    if binary is None:
        print('note: rail-extract is not built: skipping the command flag check (c)')
        return
    help_cache = {}

    def help_of(*args):
        if args not in help_cache:
            p = subprocess.run([binary, *args, '--help'], capture_output=True, text=True)
            help_cache[args] = p.stdout if p.returncode == 0 else None
        return help_cache[args]

    top = help_of() or ''
    cmds = set(re.findall(r'^  ([a-z][a-z0-9-]*)\s', top.split('Commands:')[-1], re.M)) - {'help'}
    for f in files:
        if f.startswith(NO_CMD_CHECK):
            continue
        for n, line in enumerate(read(f).splitlines(), 1):
            for m in re.finditer(r'(?:rail-extract|\$RE)\s+([a-z][a-z0-9-]*)([^`\n]*)', line):
                cmd, rest = m.group(1), m.group(2)
                if cmd not in cmds:
                    continue
                h = help_of(cmd)
                for flag in re.findall(r'(?<![\w-])(--[a-z][a-z0-9-]*)', rest):
                    if flag not in (h or ''):
                        errs.append(f'{f}:{n}: `{cmd}` has no {flag}')


def main():
    files = [p for p in sh('git', 'ls-files', '*.md').split('\n') if p and not p.startswith('app/node_modules')]
    tracked = [p for p in sh('git', 'ls-files').split('\n') if p]
    basenames = {os.path.basename(p) for p in tracked}
    allow = load_allow()
    errs = []
    for f in files:
        text = read(f)
        check_links(f, text, errs)
        check_paths(f, text, tracked, basenames, allow, errs)
    if '--no-binary' not in sys.argv:
        check_commands(files, errs)
    for e in errs:
        print(e)
    print(f'check-docs: {len(files)} files, {len(errs)} problems')
    return 1 if errs else 0


if __name__ == '__main__':
    sys.exit(main())
