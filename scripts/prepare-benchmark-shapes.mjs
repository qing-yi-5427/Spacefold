// Fresh QA-owned real NTFS fixtures; never overwrite an existing folder.
import {mkdirSync,writeFileSync,existsSync} from 'node:fs';
import path from 'node:path';
const output=path.resolve(process.argv[2]||'artifacts/perf-round2/shapes');
if(existsSync(output))throw new Error('Benchmark shape destination already exists');
mkdirSync(output,{recursive:true});
const datasets={};
for(const [name,depth,width,filesPerDirectory] of [['wide-dirs',1,2000,2],['deep-dirs',64,1,20]]){
  const root=path.join(output,name);mkdirSync(root);let parent=root,files=0,directories=0;
  for(let level=0;level<depth;level++){
    for(let branch=0;branch<width;branch++){
      const folder=path.join(parent,`d${level.toString().padStart(2,'0')}-${branch.toString().padStart(4,'0')}`);
      mkdirSync(folder);directories++;
      for(let file=0;file<filesPerDirectory;file++){
        writeFileSync(path.join(folder,`f${file.toString().padStart(2,'0')}.bin`),Buffer.alloc(128,65));files++;
      }
      if(width===1)parent=folder;
    }
  }
  datasets[name]=root;
  writeFileSync(path.join(output,`${name}-fixture.json`),JSON.stringify({source:root,files,directories,logicalBytes:String(files*128),ordinaryFiles:true,reparsePoints:false,hardlinks:false},null,2));
}
writeFileSync(path.join(output,'sources.json'),JSON.stringify(datasets,null,2));
console.log(JSON.stringify(datasets));
