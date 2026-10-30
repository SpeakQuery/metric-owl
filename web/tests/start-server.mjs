import {rmSync,mkdirSync} from 'node:fs';import {spawn} from 'node:child_process';
const dir=new URL('../../.test-data/browser',import.meta.url);rmSync(dir,{recursive:true,force:true});mkdirSync(dir,{recursive:true,mode:0o700});
const child=spawn('../target/debug/server-guard',['--data-dir','../.test-data/browser','serve','--port','4897'],{stdio:'inherit'});
process.on('SIGTERM',()=>child.kill('SIGTERM'));process.on('SIGINT',()=>child.kill('SIGINT'));child.on('exit',code=>process.exit(code??0));
