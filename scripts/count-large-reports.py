import csv,json,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'artifacts/python-qa'))
import ijson
folder=Path(sys.argv[1]);counts={};totals={};metadata={}
file=folder/'million.csv'
with file.open(encoding='utf-8-sig',newline='') as stream:
    count=total=0
    for row in csv.DictReader(stream):
        if row['record_type']=='metadata':metadata[file.name]=json.loads(row['metadata_json'])
        if row['record_type']=='entry':count+=1;total+=int(row['logical_bytes'])
    counts[file.name]=count;totals[file.name]=str(total)
file=folder/'million.json'
with file.open('rb') as stream:
    count=total=0
    for row in ijson.items(stream,'rows.item'):
        assert isinstance(row['logical_bytes'],str)
        assert row['pathEncoding']=='utf16le' and isinstance(row['pathData'],str)
        count+=1;total+=int(row['logical_bytes'])
    counts[file.name]=count;totals[file.name]=str(total)
with file.open('rb') as stream:metadata[file.name]=next(ijson.items(stream,'metadata'))
expected=int(sys.argv[2])
assert counts and all(value==expected for value in counts.values()),counts
assert all(isinstance(value['totalLogicalBytes'],str) and value['recordCount']==str(expected) and value['totalLogicalBytes']==totals[name] for name,value in metadata.items())
result={'independentStreamingParse':True,'expected':expected,'rows':counts,'exactLogicalTotals':totals,'metadata':metadata}
(folder/'large-report-parse.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result,ensure_ascii=True,indent=2))
