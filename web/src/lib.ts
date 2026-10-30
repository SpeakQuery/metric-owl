export let csrf='';export function setCsrf(s:string){csrf=s}
export async function api<T=any>(path:string,method='GET',body?:unknown):Promise<T>{const r=await fetch('/api/v1/'+path,{method,credentials:'same-origin',headers:{'Content-Type':'application/json',...(method==='GET'?{}:{'X-CSRF-Token':csrf})},...(body===undefined?{}:{body:JSON.stringify(body)})});const v=await r.json();if(!r.ok)throw new Error(v.error||'The request could not be completed');return v}
export function bytes(v:number|undefined|null){if(v==null||!Number.isFinite(v))return'—';const units=['B','KiB','MiB','GiB','TiB'];let i=0;while(v>=1024&&i<4){v/=1024;i++}return`${v.toFixed(i>1?1:0)} ${units[i]}`}
export function percent(v:number|undefined|null){return v==null?'—':v.toFixed(1)+'%'}
export function time(v:number|undefined|null){return v?new Date(v*1000).toLocaleString('en-GB',{day:'2-digit',month:'short',hour:'2-digit',minute:'2-digit',second:'2-digit'}):'—'}
export function duration(v:number){const h=Math.floor(v/3600);return h>=24?`${Math.floor(h/24)} d ${h%24} h`:`${h} h ${Math.floor(v%3600/60)} min`}
export const severity:Record<string,string>={critical:'Critical',warning:'Warning',info:'Info',ok:'Healthy',error:'Issue detected',unavailable:'Unavailable',partial:'Partial data'};
