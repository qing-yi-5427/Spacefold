"""Isolated SQLite write microbenchmark; never touches application source or live DB.

Replays an already verified 100k scan's raw UTF-16 paths, compact payloads and scalar
columns into fresh DBs. It excludes enumeration, Rust encoding, aggregation and UI.
All modes keep WAL/NORMAL, 16 MiB cache, 8000-page auto-checkpoint and identical rows.
"""
import argparse
import json
import sqlite3
import statistics
import time
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--source', default='artifacts/benchmarks-release-final/small100k-round1-spacefold-sqlite-db/spacefold.sqlite3')
parser.add_argument('--out', default='artifacts/sqlite-index-profile')
parser.add_argument('--batch-size', type=int, default=2000)
args = parser.parse_args()
source_path, output = Path(args.source).resolve(), Path(args.out).resolve()
output.mkdir(parents=True, exist_ok=True)
source = sqlite3.connect(source_path.as_uri() + '?mode=ro', uri=True)
node_sql = source.execute("SELECT sql FROM sqlite_master WHERE type='table' AND name='nodes'").fetchone()[0]
scan_sql = source.execute("SELECT sql FROM sqlite_master WHERE type='table' AND name='scans'").fetchone()[0]
columns = [column[1] for column in source.execute("PRAGMA table_xinfo(nodes)") if column[6] == 0]
quoted_columns = ','.join('"' + column + '"' for column in columns)
rows = source.execute(f'SELECT {quoted_columns} FROM nodes ORDER BY id').fetchall()
scan_rows = source.execute('SELECT * FROM scans').fetchall()
indexes = dict(source.execute("SELECT name,sql FROM sqlite_master WHERE type='index' AND tbl_name='nodes' AND sql IS NOT NULL"))
assert len(indexes) == 4, f'Expected baseline four indexes, got {list(indexes)}'
kind_name = 'nodes_kind'
assert kind_name in indexes
assert len(rows) == 100101 and len(scan_rows) == 1
expected = {'nodeCount': len(rows), 'rootLogicalBytes': source.execute('SELECT logical_total FROM nodes WHERE id=1').fetchone()[0], 'fileCount': source.execute("SELECT COUNT(*) FROM nodes WHERE kind='file'").fetchone()[0]}
assert expected['fileCount'] == 100000 and expected['rootLogicalBytes'] == 12800000
path_column = columns.index('path')
metadata = {'source': str(source_path), 'sqliteVersion': sqlite3.sqlite_version, 'python': __import__('sys').version, 'batchSize': args.batch_size, 'expected': expected, 'indexNames': list(indexes), 'maxRawPathBytes': max(len(row[path_column]) for row in rows), 'cachePolicy': 'Natural warm caches, rotated mode order, no cache/system changes. Read-only source; fresh destination DB per run.'}
source.close()
insert = f'INSERT INTO nodes({quoted_columns}) VALUES({",".join("?" for _ in columns)})'
result = []
modes = ['four-indexes', 'without-kind', 'deferred-kind']
for round_number in range(1, 4):
    order = modes[round_number-1:] + modes[:round_number-1]
    for mode in order:
        db_path = output / f'round-{round_number}-{mode}.sqlite3'
        if db_path.exists():
            raise RuntimeError(f'Refusing to overwrite evidence DB: {db_path}')
        before = time.perf_counter()
        connection = sqlite3.connect(db_path, isolation_level=None)
        connection.executescript('PRAGMA journal_mode=WAL;PRAGMA foreign_keys=ON;PRAGMA synchronous=NORMAL;PRAGMA wal_autocheckpoint=8000;PRAGMA cache_size=-16384;PRAGMA temp_store=FILE;')
        connection.execute(scan_sql)
        connection.execute(node_sql)
        connection.executemany(f'INSERT INTO scans VALUES({",".join("?" for _ in scan_rows[0])})', scan_rows)
        for name, sql in indexes.items():
            if mode != 'four-indexes' and name == kind_name:
                continue
            connection.execute(sql)
        setup_ms = (time.perf_counter() - before) * 1000
        insert_ms = commit_ms = 0
        replay_start = time.perf_counter()
        batches = 0
        for offset in range(0, len(rows), args.batch_size):
            connection.execute('BEGIN')
            before = time.perf_counter()
            connection.executemany(insert, rows[offset:offset+args.batch_size])
            insert_ms += (time.perf_counter() - before) * 1000
            before = time.perf_counter()
            connection.execute('COMMIT')
            commit_ms += (time.perf_counter() - before) * 1000
            batches += 1
        before = time.perf_counter()
        if mode == 'deferred-kind':
            connection.execute(indexes[kind_name])
        create_kind_ms = (time.perf_counter() - before) * 1000 if mode == 'deferred-kind' else 0
        replay_ms = (time.perf_counter() - replay_start) * 1000
        actual = {'nodeCount': connection.execute('SELECT COUNT(*) FROM nodes').fetchone()[0], 'rootLogicalBytes': connection.execute('SELECT logical_total FROM nodes WHERE id=1').fetchone()[0], 'fileCount': connection.execute("SELECT COUNT(*) FROM nodes WHERE kind='file'").fetchone()[0]}
        assert actual == expected
        assert connection.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
        before = time.perf_counter()
        checkpoint = connection.execute('PRAGMA wal_checkpoint(TRUNCATE)').fetchone()
        assert checkpoint[0] == 0
        connection.close()
        persist_ms = (time.perf_counter() - before) * 1000
        entry = {'round': round_number, 'mode': mode, 'setupMs': setup_ms, 'insertMs': insert_ms, 'commitMs': commit_ms, 'createKindMs': create_kind_ms, 'replayMs': replay_ms, 'checkpointCloseMs': persist_ms, 'durableReplayMs': replay_ms + persist_ms, 'batches': batches, 'databaseBytes': db_path.stat().st_size, 'correctnessVerified': True}
        result.append(entry)
        print(json.dumps(entry), flush=True)
summary = {}
for mode in modes:
    matching = [entry for entry in result if entry['mode'] == mode]
    summary[mode] = {key: statistics.median(entry[key] for entry in matching) for key in ['insertMs','commitMs','createKindMs','replayMs','checkpointCloseMs','durableReplayMs','databaseBytes']}
with (output / 'results.json').open('w', encoding='utf-8') as file:
    json.dump({'metadata': metadata, 'results': result, 'median': summary, 'interpretation': 'SQLite microbenchmark only, not application scan performance. Source rows already contain final totals; no original filesystem enumeration or Rust per-row encoding. Pending-dir autocommits, finish flags and incremental revision writes are excluded.'}, file, ensure_ascii=False, indent=2)
print(json.dumps({'median': summary}), flush=True)
