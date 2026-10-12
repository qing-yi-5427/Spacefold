"""Independent production page/node-page checks on an isolated database copy.

Million rows are synthetic query evidence, not a physical filesystem scan.
Python ordering supplies the oracle; no repository ORDER BY is reused.
"""
import argparse
import hashlib
import json
import shutil
import sqlite3
import subprocess
import time
from pathlib import Path


def digest_nodes(database):
    digest = hashlib.sha256()
    count = 0
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        for row in db.execute('SELECT * FROM nodes ORDER BY scan_id,id'):
            digest.update(json.dumps(
                [value.hex() if isinstance(value, bytes) else value for value in row],
                ensure_ascii=True, separators=(',', ':')).encode())
            count += 1
        scans = db.execute('SELECT * FROM scans ORDER BY id').fetchall()
    return digest.hexdigest(), count, scans


def ascii_nocase(value):
    # SQLite NOCASE folds only ASCII, including for valid non-ASCII UTF-8 names.
    return value.translate(str.maketrans('ABCDEFGHIJKLMNOPQRSTUVWXYZ',
                                        'abcdefghijklmnopqrstuvwxyz'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-database', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--probe', type=Path,
                        default=Path('src-tauri/target/release/spacefold-probe.exe'))
    parser.add_argument('--page-size', type=int, default=200)
    parser.add_argument('--full-traversal', action='store_true')
    args = parser.parse_args()
    source, output, probe = (args.source_database.resolve(), args.output.resolve(),
                             args.probe.resolve())
    output.mkdir(parents=True, exist_ok=False)
    data = output / 'data'
    data.mkdir()
    database = data / 'spacefold.sqlite3'
    assert not Path(str(source) + '-wal').exists() or Path(str(source) + '-wal').stat().st_size == 0
    shutil.copy2(source, database)
    before = digest_nodes(database)
    results = {'probeSha256': hashlib.sha256(probe.read_bytes()).hexdigest(),
               'sourceDatabase': str(source), 'copyOnly': True, 'cases': [],
               'oracle': 'Python integers plus ASCII-only NOCASE; complete selected slices'}
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        scan_id, revision = db.execute('SELECT id,revision FROM scans ORDER BY id LIMIT 1').fetchone()
        # Keep only the small oracle tuple in memory. The raw path is read separately
        # for each asserted API slice, independently of the display payload.
        rows = db.execute('SELECT id,name,logical_total,modified FROM nodes WHERE scan_id=? AND parent_id=1',
                          (scan_id,)).fetchall()
        original_indexes = [row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='index'")]
    assert len(rows) > 1

    def request(sort, cursor=None):
        return {'scanId': scan_id, 'parentId': '1', 'revision': revision,
                'sort': sort, 'filter': None, 'cursor': cursor,
                'pageSize': args.page_size}

    def invoke(command, *parts, expected_error=None):
        start = time.perf_counter()
        run = subprocess.run([str(probe), command, str(data), *map(str, parts)],
                             capture_output=True, encoding='utf-8', check=False)
        elapsed = time.perf_counter() - start
        assert run.stdout.strip(), (run.returncode, run.stderr)
        value = json.loads(run.stdout.strip().splitlines()[-1])
        if expected_error:
            assert run.returncode == 1 and value['code'] == expected_error, value
        else:
            assert run.returncode == 0, (value, run.stderr)
        return value, elapsed

    def check_page(value, expected):
        page = value['page']
        assert int(page['totalCount']) == len(rows), page['totalCount']
        assert page['revision'] == revision
        assert [int(node['nodeId']) for node in page['nodes']] == [row[0] for row in expected]
        with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
            for node, row in zip(page['nodes'], expected):
                assert node['name'] == row[1] and int(node['logicalTotalBytes']) == row[2]
                raw = db.execute('SELECT path FROM nodes WHERE scan_id=? AND id=?',
                                 (scan_id, row[0])).fetchone()[0]
                decoded = raw.decode('utf-16le', 'surrogatepass')
                if decoded.startswith('\\\\?\\UNC\\'):
                    decoded = '\\\\' + decoded[8:]
                elif decoded.startswith('\\\\?\\'):
                    decoded = decoded[4:]
                assert node['displayPath'] == decoded, (node['displayPath'], decoded)

    # First constructor open performs the real obsolete-index migration.
    initial, initial_wall = invoke('page', json.dumps(request('size_desc')))
    repeated, repeat_wall = invoke('page', json.dumps(request('size_desc')))
    results['firstOpenPlusPageWallSeconds'] = initial_wall
    results['secondOpenPlusPageWallSeconds'] = repeat_wall
    results['initialPageOnlyMs'] = initial['pageElapsedMs']
    results['migrationBoundary'] = 'First Service constructor + page + process exit; not isolated DROP duration'
    results['oldAscIndexBefore'] = 'nodes_parent_size_asc_v2' in original_indexes

    def cursor_for(sort, previous, key):
        return json.dumps({'key': key, 'id': previous[0], 'name': previous[1],
                           'size': previous[2], 'modified': previous[3] or ''},
                          ensure_ascii=True, separators=(',', ':'))

    sort_keys = {
        'size_desc': (lambda row: (-row[2], ascii_nocase(row[1]), row[0]), False),
        'size_asc': (lambda row: (-row[2], ascii_nocase(row[1]), row[0]), True),
        'name_asc': (lambda row: (ascii_nocase(row[1]), row[0]), False),
        'name_desc': (lambda row: (ascii_nocase(row[1]), row[0]), True),
        # Two stable sorts provide descending modified, ascending name/id ties.
        'modified_desc': (None, False),
    }
    for sort, (key, reverse) in sort_keys.items():
        if key:
            ordered = sorted(rows, key=key, reverse=reverse)
        else:
            ordered = sorted(sorted(rows, key=lambda row: (ascii_nocase(row[1]), row[0])),
                             key=lambda row: row[3] or '', reverse=True)
        first, wall = invoke('page', json.dumps(request(sort)))
        check_page(first, ordered[:args.page_size])
        page = first['page']
        assert page['nextCursor'], 'Need at least two pages'
        cursor_key = json.loads(page['nextCursor'])['key']
        assert json.loads(cursor_key)[0] == 2
        offsets = sorted(set([args.page_size, len(rows) // 2,
                              max(args.page_size, len(rows) - args.page_size)]))
        timings = [{'offset': 0, 'pageMs': first['pageElapsedMs'], 'wallSeconds': wall}]
        for offset in offsets:
            cursor = cursor_for(sort, ordered[offset - 1], cursor_key)
            value, wall = invoke('page', json.dumps(request(sort, cursor)))
            check_page(value, ordered[offset:offset + args.page_size])
            timings.append({'offset': offset, 'pageMs': value['pageElapsedMs'], 'wallSeconds': wall})
        locates = []
        if sort != 'modified_desc':
            for offset in [0, len(rows) // 2, len(rows) - 1]:
                value, wall = invoke('node-page', scan_id, ordered[offset][0], sort, args.page_size)
                check_page(value, ordered[offset:offset + args.page_size])
                locates.append({'targetOffset': offset, 'nodeId': ordered[offset][0],
                                'pageMs': value['pageElapsedMs'], 'wallSeconds': wall})
        else:
            invoke('node-page', scan_id, ordered[0][0], sort, args.page_size,
                   expected_error='INVALID_SORT')
        old = json.loads(page['nextCursor'])
        old_key = json.loads(old['key']); old_key[0] = 1
        old['key'] = json.dumps(old_key, separators=(',', ':'))
        invoke('page', json.dumps(request(sort, json.dumps(old))), expected_error='INVALID_CURSOR')
        for field, bad in [('size', -(2**63)), ('id', 0), ('id', -1)]:
            invalid = json.loads(page['nextCursor']); invalid[field] = bad
            invoke('page', json.dumps(request(sort, json.dumps(invalid))), expected_error='INVALID_CURSOR')
        if args.full_traversal:
            seen = [int(node['nodeId']) for node in page['nodes']]
            cursor = page['nextCursor']
            while cursor:
                value, _ = invoke('page', json.dumps(request(sort, cursor)))
                check_page(value, ordered[len(seen):len(seen) + args.page_size])
                seen.extend(int(node['nodeId']) for node in value['page']['nodes'])
                cursor = value['page']['nextCursor']
            assert seen == [row[0] for row in ordered] and len(set(seen)) == len(seen)
        results['cases'].append({'sort': sort, 'pages': timings, 'locates': locates,
                                 'oldCursorRejected': True, 'invalidCursorRejected': True,
                                 'fullTraversal': args.full_traversal})
    assert digest_nodes(database) == before, 'Persisted historical records changed'
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not db.execute('PRAGMA foreign_key_check').fetchall()
        indexes = [row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='index'")]
        assert 'nodes_parent_size_asc_v2' not in indexes
        results['indexesAfter'] = indexes
        results['queryPlans'] = {}
        for direction in ['ASC', 'DESC']:
            sql = ('EXPLAIN QUERY PLAN SELECT id FROM nodes WHERE scan_id=? AND parent_id=1 '
                   f'ORDER BY sort_size {direction},name COLLATE NOCASE {direction},id {direction} LIMIT 200')
            plan = db.execute(sql, (scan_id,)).fetchall()
            assert any('nodes_parent_size_v3' in row[3] for row in plan)
            assert not any('TEMP B-TREE' in row[3] for row in plan)
            results['queryPlans'][direction] = plan
        results['pageSize'] = db.execute('PRAGMA page_size').fetchone()[0]
    results.update({'rows': len(rows), 'historyAllColumnsUnchanged': True,
                    'historyNodeDigest': before[0], 'passed': True})
    (output / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'rows': len(rows), 'passed': True, 'output': str(output)}))


if __name__ == '__main__':
    main()
