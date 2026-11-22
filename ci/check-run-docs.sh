#!/usr/bin/env bash
# 「動かす」（docs/src/run.md）の container の命令が、決めた名と守りに合っているかを見る。
#   1. container を起こす・止める・消す命令（run --name・start・stop・kill・rm・delete・logs）は、名 castlist.box を指す
#      （pagetray などの container が、Apple Container の DNS で castlist.box の名で引くため）。
#   2. その命令と、data の volume を消す命令は、repo の設定（.claude/roles/config.jsonc）の infra.castlist の match の
#      どれかを含む（Product Manager だけが打てる pm-only の守りに当たる。docs に書いた命令が守りから漏れない）。
# 1 つでも外れれば、行を出して exit 1。
# 使い方: ci/check-run-docs.sh [run.md の path] [config.jsonc の path]
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
doc=${1:-$root/docs/src/run.md}
config=${2:-$root/.claude/roles/config.jsonc}

python3 - "$doc" "$config" <<'PY'
import json, re, sys

doc, config = sys.argv[1], sys.argv[2]
# JSONC: 行の注釈と、閉じ括弧の前のカンマを落とす（この file の書き方だけを読めればよい）。
text = re.sub(r'^\s*//.*$', '', open(config, encoding='utf-8').read(), flags=re.M)
text = re.sub(r',(\s*[\]}])', r'\1', text)
match = json.loads(text)['infra']['castlist']['match']

NAME = 'castlist.box'
lifecycle = re.compile(r'\bcontainer\s+(run\b[^`]*?--name\s+([A-Za-z0-9._-]+)|(start|stop|kill|rm|delete|logs)\s+([A-Za-z0-9._-]+))')
volume = re.compile(r'\bcontainer\s+volume\s+(delete|rm)\b')
bad = []
for n, line in enumerate(open(doc, encoding='utf-8'), 1):
    for m in lifecycle.finditer(line):
        name = m.group(2) or m.group(4)
        if name != NAME:
            bad.append(f'{doc}:{n}: container の名が {NAME} でない（{name}）: {line.strip()}')
        cmd = m.group(0)
        if not cmd.split()[1] == 'logs' and not any(p in line for p in match):
            bad.append(f'{doc}:{n}: pm-only の match に当たらない: {line.strip()}')
    if volume.search(line) and not any(p in line for p in match):
        bad.append(f'{doc}:{n}: pm-only の match に当たらない: {line.strip()}')
for b in bad:
    print(b, file=sys.stderr)
if bad:
    print(f'NG: 「動かす」の container の命令が、名 {NAME} か pm-only の守りに合わない（上の行）', file=sys.stderr)
    sys.exit(1)
print(f'ok: 「動かす」の container の命令は、名 {NAME} を指し、pm-only の守りに当たる')
PY
