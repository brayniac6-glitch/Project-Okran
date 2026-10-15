"""Recount docs/PARITY.md: done 1, partial 0.5, missing 0, per section and overall, written
under its `<!-- parity:score -->` marker. `python tools/parity.py`"""
import re
from pathlib import Path

path = Path(__file__).resolve().parent.parent / 'docs' / 'PARITY.md'
text = path.read_text(encoding='utf-8')
marker = '<!-- parity:score -->'
head, rest = text.split(marker, 1)
# drop the score block written last time (up to the first section heading)
rest = rest[rest.index('\n## '):]

sections = []
for block in re.split(r'\n(?=## )', rest.strip('\n')):
    title = block.splitlines()[0].lstrip('# ').strip()
    marks = re.findall(r'^- \[( |~|x)\]', block, re.M)
    if marks:
        sections.append((title, marks))

def score(marks):
    return sum({'x': 1.0, '~': 0.5, ' ': 0.0}[m] for m in marks)

all_marks = [m for _, ms in sections for m in ms]
total = score(all_marks) / len(all_marks)
lines = [
    '',
    f'**Overall: {total:.0%}** ({all_marks.count("x")} done, {all_marks.count("~")} partial, '
    f'{all_marks.count(" ")} missing of {len(all_marks)} features)',
    '',
    '| Section | Score | Done | Partial | Missing |',
    '|---|---|---|---|---|',
]
for title, ms in sections:
    lines.append(f'| {title} | {score(ms) / len(ms):.0%} | {ms.count("x")} | {ms.count("~")} | {ms.count(" ")} |')
path.write_text(head + marker + '\n' + '\n'.join(lines) + '\n' + rest, encoding='utf-8')
print('\n'.join(lines))
