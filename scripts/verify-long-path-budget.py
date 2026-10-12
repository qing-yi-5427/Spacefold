"""One real long-path budget diagnostic, excluded from performance comparisons."""
import argparse
import ast
import hashlib
import json
import os
import sqlite3
import subprocess
from contextlib import closing
from pathlib import Path


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--probe', type=Path,
                    default=Path('src-tauri/target/release/spacefold-probe.exe'))
args = parser.parse_args()
output, probe = args.output.resolve(), args.probe.resolve()
output.mkdir(parents=True, exist_ok=False)
source = output / 'source'
leaf = source
for depth in range(16):
    leaf /= f'layer-{depth:02}-' + 'p' * 64
leaf.mkdir(parents=True)
for index in range(8000):
    file = leaf / f'file-{index:05}.txt'
    with file.open('xb') as handle:
        handle.write(b'123')
assert len(str(file)) > 600
# Reuse the independently audited full-tree verifier without executing the
# benchmark's argparse/tool loop, and explicitly close all read connections.
tree = ast.parse(Path('scripts/benchmark.py').read_text(encoding='utf-8'))
functions = ast.Module(body=[node for node in tree.body
    if isinstance(node, ast.FunctionDef) and node.name in ('expected', 'verify_spacefold')],
    type_ignores=[])
scope = dict(os=os, json=json, sqlite3=sqlite3, Path=Path, closing=closing)
exec(compile(functions, 'independent-benchmark-verifier', 'exec'), scope)
expected, manifest = scope['expected'](source)
environment = dict(os.environ, SPACEFOLD_PROFILE='1')
run = subprocess.run([str(probe), 'scan', str(output / 'data'), str(source)],
                     env=environment, capture_output=True, encoding='utf-8', check=False)
(output / 'stdout.jsonl').write_text(run.stdout, encoding='utf-8')
(output / 'stderr.jsonl').write_text(run.stderr, encoding='utf-8')
assert run.returncode == 0, (run.stdout, run.stderr)
summary = json.loads(run.stdout.strip().splitlines()[-1])
profile = next(json.loads(line)['scanProfile'] for line in run.stderr.splitlines()
               if line.startswith('{') and 'scanProfile' in json.loads(line))
assert profile['peakBatchPathBytes'] >= 4 * 1024 * 1024, profile
assert profile['peakBatchEntries'] < 8000, profile
verified = scope['verify_spacefold'](output / 'data', summary['scan'], expected, manifest, 'current')
result = {'passed': True, 'profileEnabled': True, 'notPerformanceEvidence': True,
          'sourceFiles': 8000, 'longestFilePathCharacters': len(str(file)),
          'probeSha256': hashlib.sha256(probe.read_bytes()).hexdigest(),
          'batchBytesTriggerObserved': True, 'profile': profile,
          'budgetBoundary': '4 MiB threshold covers path/metadata/encoded owned capacities; '
                            'post-add trigger may overshoot by one bounded row. '
                            'This diagnostic does not independently measure that row or total RAM.',
          'persistenceVerification': verified, 'scan': summary['scan']}
(output / 'results.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({key: result[key] for key in ('passed', 'sourceFiles',
                  'longestFilePathCharacters', 'batchBytesTriggerObserved')}))
