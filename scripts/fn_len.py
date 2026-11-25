#!/usr/bin/env python3
"""List Rust functions longer than a limit (guide A-10: 120 lines).
Method: a function starts at a line containing `fn NAME` and ends at the matching closing brace;
its length is (closing line - signature line + 1). Braces inside strings, char literals and comments
are ignored. Test code (`#[cfg(test)]` modules) is skipped. Usage: fn_len.py SRC_DIR [--limit 120]
Exit code 0 when no function exceeds the limit, 1 otherwise."""
import os, re, sys

FN = re.compile(r'\bfn\s+([A-Za-z_][A-Za-z0-9_]*)')


def strip(src):
    """Replace comments, string and char literals by spaces, keeping newlines and length."""
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            j = src.find('\n', i); j = n if j < 0 else j
            out.append(' ' * (j - i)); i = j
        elif src.startswith('/*', i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith('/*', j): depth += 1; j += 2
                elif src.startswith('*/', j): depth -= 1; j += 2
                else: j += 1
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif c == 'r' and re.match(r'r#*"', src[i:i + 8]):
            m = re.match(r'r(#*)"', src[i:]); close = '"' + m.group(1)
            j = src.find(close, i + len(m.group(0)))
            j = n if j < 0 else j + len(close)
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            j = min(n, j + 1)
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:i + 12])
            if m:
                out.append(' ' * len(m.group(0))); i += len(m.group(0))
            else:
                out.append(c); i += 1                 # a lifetime
        else:
            out.append(c); i += 1
    return ''.join(out)


def functions(path):
    src = strip(open(path, encoding='utf-8', errors='replace').read())
    lines = src.split('\n')
    starts = [0]
    for ln in lines: starts.append(starts[-1] + len(ln) + 1)
    # skip #[cfg(test)] modules: find `mod NAME {` that follows a cfg(test) attribute
    skip = []
    for m in re.finditer(r'#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*mod\s+\w+\s*\{', src):
        depth, j = 1, m.end()
        while j < len(src) and depth:
            depth += (src[j] == '{') - (src[j] == '}'); j += 1
        skip.append((m.start(), j))
    res = []
    for m in FN.finditer(src):
        if any(a <= m.start() < b for a, b in skip): continue
        j = m.end()
        while j < len(src) and src[j] not in '{;': j += 1     # signature ends at `{` (body) or `;` (declaration)
        if j >= len(src) or src[j] == ';': continue
        depth, k = 1, j + 1
        while k < len(src) and depth:
            depth += (src[k] == '{') - (src[k] == '}'); k += 1
        a = src.count('\n', 0, m.start()) + 1
        b = src.count('\n', 0, k - 1) + 1
        res.append((b - a + 1, m.group(1), a))
    return res


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else 'src'
    limit = int(sys.argv[sys.argv.index('--limit') + 1]) if '--limit' in sys.argv else 120
    found = []
    for d, _, fs in os.walk(root):
        for f in fs:
            if f.endswith('.rs'):
                p = os.path.join(d, f)
                found += [(n, os.path.relpath(p, root) + '::' + name, line) for n, name, line in functions(p) if n > limit]
    found.sort(reverse=True)
    for n, name, line in found: print(f'{n:4d}  {name}  (line {line})')
    print(f'{len(found)} function(s) longer than {limit} lines in {root}')
    return 1 if found else 0


if __name__ == '__main__':
    sys.exit(main())
