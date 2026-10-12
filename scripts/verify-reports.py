"""Independently parse real export files without using application writers."""
import argparse, csv, json, re, zipfile
from pathlib import Path
from xml.etree import ElementTree as ET
from html.parser import HTMLParser

class ReportHTML(HTMLParser):
    def __init__(self): super().__init__(); self.rows=0; self.scripts=0
    def handle_starttag(self, tag, attrs):
        if tag=='tr': self.rows+=1
        if tag=='script': self.scripts+=1

def verify(directory):
    folder=Path(directory)
    files={ext:folder/f'report.{ext}' for ext in ('csv','xlsx','json','html')}
    doc=json.loads(files['json'].read_text(encoding='utf-8-sig'))
    assert doc['schemaVersion']==1
    metadata=doc['metadata']; rows=doc['rows']
    count=len(rows)
    assert str(metadata['recordCount'])==str(count)
    assert isinstance(metadata['totalLogicalBytes'],str)
    for row in rows:
        for field in ('logical_bytes','own_logical_bytes','allocated_bytes'):
            if field in row and row[field] is not None: assert isinstance(row[field],str) and row[field].isdigit(),(field,row[field])
    with files['csv'].open(encoding='utf-8-sig',newline='') as stream:
        reader=csv.DictReader(stream); csv_rows=list(reader)
        assert reader.fieldnames[:2]==['record_type','metadata_json']
    assert csv_rows[0]['record_type']=='metadata'
    csv_metadata=json.loads(csv_rows[0]['metadata_json'])
    entries=[r for r in csv_rows if r['record_type']=='entry']
    assert len(entries)==count
    assert str(csv_metadata['recordCount'])==str(count)
    for expected,actual in zip(rows,entries):
        for field in ('logical_bytes','own_logical_bytes','allocated_bytes'):
            if field in expected: assert actual[field]==(expected[field] or ''),(field,actual[field],expected[field])
        if 'name' in expected and expected['name'].startswith(('=','+','-','@','＝','＋','－','＠')): assert actual['name'].startswith("'")
    ns={'m':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
    with zipfile.ZipFile(files['xlsx']) as archive:
        assert archive.testzip() is None
        workbook=ET.fromstring(archive.read('xl/workbook.xml'))
        sheets=workbook.findall('m:sheets/m:sheet',ns)
        assert len(sheets)>=2
        data=ET.fromstring(archive.read('xl/worksheets/sheet2.xml'))
        xlsx_rows=data.findall('m:sheetData/m:row',ns)
        assert len(xlsx_rows)==count+1,(len(xlsx_rows),count)
        assert data.find('m:autoFilter',ns) is not None
        assert data.find('m:sheetViews/m:sheetView/m:pane',ns) is not None
        shared=[]
        if 'xl/sharedStrings.xml' in archive.namelist():
            shared=[''.join(item.itertext()) for item in ET.fromstring(archive.read('xl/sharedStrings.xml')).findall('m:si',ns)]
        def text(cell):
            if cell.get('t')=='s': return shared[int(cell.find('m:v',ns).text)]
            return ''.join(node.text or '' for node in cell.findall('.//m:t',ns))
        headers={re.sub(r'\d','',cell.get('r')):text(cell) for cell in xlsx_rows[0].findall('m:c',ns)}
        for expected,row in zip(rows,xlsx_rows[1:]):
            for cell in row.findall('m:c',ns):
                field=headers[re.sub(r'\d','',cell.get('r'))]
                if field in ('logical_bytes','own_logical_bytes','allocated_bytes'):
                    assert cell.get('t') in ('s','inlineStr'),(field,cell.attrib)
                    assert text(cell)==expected[field]
        for row in xlsx_rows[1:]:
            for cell in row.findall('m:c',ns):
                assert cell.find('m:f',ns) is None, 'Formula found in exported data'
    html=files['html'].read_text(encoding='utf-8')
    parsed=ReportHTML();parsed.feed(html)
    assert parsed.scripts==0
    assert parsed.rows==count+1,(parsed.rows,count)
    assert 'Spacefold' in html
    return {'independentParse':True,'rows':count,'metadata':metadata,'files':{k:str(v) for k,v in files.items()}}

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('directory');parser.add_argument('--output');args=parser.parse_args()
    result=verify(args.directory)
    rendered=json.dumps(result,ensure_ascii=False,indent=2)
    if args.output: Path(args.output).write_text(rendered,encoding='utf-8')
    print(rendered)
