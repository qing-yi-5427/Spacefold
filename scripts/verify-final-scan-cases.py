"""One-time final correctness confirmations, not another performance matrix."""
import argparse
import ast
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import time
from contextlib import closing
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--probe', type=Path, default=Path('src-tauri/target/release/spacefold-probe.exe'))
args = parser.parse_args()
root = Path.cwd()
output, probe = args.output.resolve(), args.probe.resolve()
output.mkdir(parents=True, exist_ok=False)
assert not os.environ.get('SPACEFOLD_PROFILE')
tree = ast.parse((root / 'scripts/benchmark.py').read_text(encoding='utf-8'))
names = ('expected', 'verify_spacefold', 'history_snapshot', 'verify_history')
subset = ast.Module(body=[node for node in tree.body
    if isinstance(node, ast.FunctionDef) and node.name in names], type_ignores=[])
scope = dict(os=os, json=json, sqlite3=sqlite3, Path=Path, closing=closing, hashlib=hashlib)
exec(compile(subset, 'independent-full-tree-verifier', 'exec'), scope)
seed = root / 'artifacts/benchmarks-final-42/small100k-round1-spacefold-sqlite-db/spacefold.sqlite3'
shapes = json.loads((root / 'artifacts/perf-round2/shapes/sources.json').read_text(encoding='utf-8'))
cases = {'first-upgrade': root / 'artifacts/benchmark-source',
         'real-msvc-headers': Path('C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/include'),
         'deep-dirs': Path(shapes['deep-dirs'])}
results = {'probeSha256': hashlib.sha256(probe.read_bytes()).hexdigest(),
           'notFormalPerformanceComparison': True, 'cases': {}}
for name, source in cases.items():
    source = source.resolve()
    data = output / (name + '-data')
    data.mkdir()
    history = None
    if name == 'first-upgrade':
        assert not Path(str(seed) + '-wal').exists() or Path(str(seed) + '-wal').stat().st_size == 0
        shutil.copy2(seed, data / 'spacefold.sqlite3')
        history = scope['history_snapshot'](data / 'spacefold.sqlite3')
        with closing(sqlite3.connect((data / 'spacefold.sqlite3').as_uri() + '?mode=ro', uri=True)) as db:
            assert db.execute("SELECT COUNT(*) FROM sqlite_master WHERE name='nodes_parent_size_asc_v2'").fetchone()[0] == 1
    expected, manifest = scope['expected'](source)
    started = time.perf_counter()
    run = subprocess.run([str(probe), 'scan', str(data), str(source)],
                         capture_output=True, encoding='utf-8', check=False)
    wall = time.perf_counter() - started
    (output / (name + '-stdout.jsonl')).write_text(run.stdout, encoding='utf-8')
    (output / (name + '-stderr.log')).write_text(run.stderr, encoding='utf-8')
    assert run.returncode == 0, (run.stdout, run.stderr)
    summary = json.loads(run.stdout.strip().splitlines()[-1])
    wal = Path(str(data / 'spacefold.sqlite3') + '-wal')
    assert not wal.exists() or wal.stat().st_size == 0
    with closing(sqlite3.connect((data / 'spacefold.sqlite3').as_uri() + '?mode=ro&immutable=1', uri=True)) as db:
        state = db.execute('SELECT status,discovered FROM scans WHERE id=?', (summary['scan']['scanId'],)).fetchone()
        assert state == ('completed', expected['entriesExcludingRoot'] + 1)
    case = {'source': str(source), 'expected': expected,
            'observedProcessExitSeconds': wall, 'comparisonBoundary': 'Single observation, not a five-round performance claim',
            'walBytesAfterExit': 0, 'immutableComplete': True, 'probeSummary': summary,
            'persistenceVerification': scope['verify_spacefold'](data, summary['scan'], expected, manifest, 'current')}
    if history:
        case['historyVerification'] = scope['verify_history'](data / 'spacefold.sqlite3', history)
        case['firstUpgradeIncludesConstructorDropMigration'] = True
    results['cases'][name] = case
results['passed'] = True
(output / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({'passed': True, 'cases': list(results['cases'])}))
