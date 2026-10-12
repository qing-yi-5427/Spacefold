import json,sys,zipfile
from pathlib import Path
from xml.etree import ElementTree as ET
ns={'m':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
def rows(stream):
    stack=[];count=0;first=last=None
    for event,element in ET.iterparse(stream,events=('start','end')):
        if event=='start':stack.append(element);continue
        if element.tag.endswith('}row'):
            values=[''.join(cell.itertext()) for cell in element.findall('m:c',ns)]
            if first is None:first=values
            last=values;count+=1
            if len(stack)>1:stack[-2].remove(element)
            element.clear()
        stack.pop()
    return {'xmlRowsIncludingHeader':count,'dataRows':count-1,'first':first,'last':last}
file=Path(sys.argv[1]);expected=int(sys.argv[2]);result=[]
with zipfile.ZipFile(file) as archive:
    assert archive.testzip() is None
    workbook=ET.fromstring(archive.read('xl/workbook.xml'))
    sheets=workbook.findall('m:sheets/m:sheet',ns)
    assert len(sheets)==3,[sheet.attrib for sheet in sheets]
    for index in (2,3):
        with archive.open(f'xl/worksheets/sheet{index}.xml') as stream:result.append(rows(stream))
assert result[0]['dataRows']==1048575,result
assert result[1]['dataRows']==expected-1048575,result
assert sum(sheet['dataRows'] for sheet in result)==expected
rendered={'independentOOXMLStreamingRead':True,'file':str(file),'rows':expected,'sheets':result}
Path(str(file)+'.verification.json').write_text(json.dumps(rendered,indent=2),encoding='utf-8')
print(json.dumps(rendered,indent=2))
