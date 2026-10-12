import {mkdirSync,writeFileSync,readdirSync,lstatSync} from 'node:fs';
import path from 'node:path';
const root=path.resolve('artifacts/benchmark-source');mkdirSync(root,{recursive:true});
const start=performance.now(),content=Buffer.alloc(128,65);
for(let folder=0;folder<100;folder++){
  const directory=path.join(root,`dir-${String(folder).padStart(3,'0')}`);mkdirSync(directory,{recursive:true});
  for(let file=0;file<1000;file++)writeFileSync(path.join(directory,`file-${String(file).padStart(4,'0')}.txt`),content);
}
const expected={source:root,files:100000,directories:100,logicalBytes:'12800000',buildMs:Math.round(performance.now()-start),hardlinks:false,symlinks:false,kind:'real small-file NTFS fixture'};
writeFileSync('artifacts/benchmark-source.json',JSON.stringify(expected,null,2));console.log(JSON.stringify(expected));
