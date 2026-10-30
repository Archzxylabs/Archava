"""SUPERSEDED by per-package-coverage.mjs — kept for provenance only.

This was the first pass at the per-scope table. It rounds where istanbul
floors, so its percentages sit 0.01 above the gate on most scopes; the .mjs
port uses istanbul-lib-coverage directly and is the authoritative one.
"""

#!/usr/bin/env python3
"""Aggregate vitest v8 coverage-final.json per workspace package.

Usage: python3 per-package-coverage.py <coverage-final.json> [--md]

Prints lines/functions/branches/statements percentages for every
`packages/<name>` and `apps/<name>` directory, plus repo totals. Used to derive
the threshold values in vitest.config.ts (A6) from a real `vitest run
--coverage` rather than guessed numbers.
"""
import collections
import json
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..', '..'))


def pct(hit, total):
    return (100.0 * hit / total) if total else 0.0


def main():
    path = sys.argv[1]
    as_md = '--md' in sys.argv[2:]
    data = json.load(open(path))
    agg = collections.defaultdict(
        lambda: collections.Counter(
            files=0, l_total=0, l_hit=0, f_total=0, f_hit=0, b_total=0, b_hit=0,
            s_total=0, s_hit=0,
        )
    )
    for fname, cov in data.items():
        rel = os.path.relpath(fname, ROOT)
        parts = rel.split(os.sep)
        key = '/'.join(parts[:2]) if len(parts) > 1 else rel
        a = agg[key]
        a['files'] += 1
        lines = {}
        statements = cov.get('statementMap', {})
        s = cov.get('s', {})
        for sid, loc in statements.items():
            ln = loc['start']['line']
            lines[ln] = lines.get(ln, False) or (s.get(sid, 0) > 0)
        a['l_total'] += len(lines)
        a['l_hit'] += sum(1 for v in lines.values() if v)
        a['s_total'] += len(s)
        a['s_hit'] += sum(1 for v in s.values() if v > 0)
        fns = cov.get('f', {})
        a['f_total'] += len(fns)
        a['f_hit'] += sum(1 for v in fns.values() if v > 0)
        for branches in cov.get('b', {}).values():
            a['b_total'] += len(branches)
            a['b_hit'] += sum(1 for v in branches if v > 0)

    rows = []
    for key, a in sorted(agg.items()):
        rows.append(
            (
                key,
                a['files'],
                pct(a['l_hit'], a['l_total']),
                pct(a['f_hit'], a['f_total']),
                pct(a['b_hit'], a['b_total']),
                pct(a['s_hit'], a['s_total']),
                a['l_total'],
                a['f_total'],
                a['b_total'],
                a['s_total'],
            )
        )

    total = collections.Counter()
    for a in agg.values():
        total.update(a)

    if as_md:
        print('| scope | files | lines % | functions % | branches % | statements % | lines | functions | branches |')
        print('| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |')
        for r in rows:
            print(
                f'| `{r[0]}` | {r[1]} | {r[2]:.2f} | {r[3]:.2f} | {r[4]:.2f} | {r[5]:.2f} | '
                f'{r[6]} | {r[7]} | {r[8]} |'
            )
        print(
            f'| **TOTAL** | {total["files"]} | '
            f'{pct(total["l_hit"], total["l_total"]):.2f} | '
            f'{pct(total["f_hit"], total["f_total"]):.2f} | '
            f'{pct(total["b_hit"], total["b_total"]):.2f} | '
            f'{pct(total["s_hit"], total["s_total"]):.2f} | '
            f'{total["l_total"]} | {total["f_total"]} | {total["b_total"]} |'
        )
    else:
        print(f'{"scope":34}{"files":>6}{"lines%":>9}{"fn%":>8}{"br%":>8}{"stmt%":>8}{"L":>8}{"F":>7}{"B":>8}')
        for r in rows:
            print(
                f'{r[0]:34}{r[1]:6}{r[2]:9.2f}{r[3]:8.2f}{r[4]:8.2f}{r[5]:8.2f}'
                f'{r[6]:8}{r[7]:7}{r[8]:8}'
            )
        print(
            f'\nTOTAL files={total["files"]} lines={pct(total["l_hit"], total["l_total"]):.2f} '
            f'functions={pct(total["f_hit"], total["f_total"]):.2f} '
            f'branches={pct(total["b_hit"], total["b_total"]):.2f} '
            f'statements={pct(total["s_hit"], total["s_total"]):.2f}'
        )


if __name__ == '__main__':
    main()
