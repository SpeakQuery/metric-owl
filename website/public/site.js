const menu = document.querySelector('.menu-toggle');
const nav = document.querySelector('#main-nav');
menu?.addEventListener('click', () => {
  const expanded = menu.getAttribute('aria-expanded') !== 'true';
  menu.setAttribute('aria-expanded', String(expanded));
  menu.setAttribute('aria-label', expanded ? 'Close navigation' : 'Open navigation');
  nav.classList.toggle('open', expanded);
});
nav?.addEventListener('click', event => {
  if (event.target.closest('a')) {nav.classList.remove('open');menu.setAttribute('aria-expanded','false');menu.setAttribute('aria-label','Open navigation');}
});
document.addEventListener('keydown', event => {
  if(event.key==='Escape'&&nav?.classList.contains('open')) {nav.classList.remove('open');menu.setAttribute('aria-expanded','false');menu.setAttribute('aria-label','Open navigation');menu.focus();}
});
const shots = {
  dashboard: {file:'dashboard.jpg',alt:'Overview of an example Ubuntu server, including CPU, RAM, disk, network and performance history.',description:'A clear overview of live performance, processes and events.'},
  rules: {file:'rules.jpg',alt:'Ready-made Balanced, Sensitive and Sustained load alert packs in the Metric Owl application.',description:'Start with ready-made packs. Make thresholds and durations your own.'},
  chat: {file:'server-ai.jpg',alt:'Metric Owl Server AI chat with short English and Turkish answers and a missing-data notice. Synthetic example data.',description:'Ask in English or Turkish. Short answers use this server’s collected evidence.'},
  mcp: {file:'mcp.jpg',alt:'Read-only MCP connection guide with Codex configuration and client permissions. No actual credentials.',description:'Give your AI agent server context with scoped, revocable MCP access.'},
  light: {file:'dashboard-light.jpg',alt:'Metric Owl overview in a light theme with pastel metric cards and charts. Example data.',description:'The same clarity, with a lighter perspective.'},
};
const tabs = [...document.querySelectorAll('[data-shot]')];
const image = document.querySelector('#tour-image');
const dialog = document.querySelector('.image-dialog');
for(const tab of tabs) {
  tab.addEventListener('click',()=>{
    for(const button of tabs) {button.setAttribute('aria-selected',String(button===tab));button.tabIndex=button===tab?0:-1;}
    const shot=shots[tab.dataset.shot]; image.src='/assets/'+shot.file; image.alt=shot.alt;
    document.querySelector('#tour-description').textContent=shot.description;
    document.querySelector('#tour-panel').setAttribute('aria-labelledby',tab.id);
  });
  tab.addEventListener('keydown',event=>{
    const index=tabs.indexOf(tab);
    const target=event.key==='ArrowRight'?(index+1)%tabs.length:event.key==='ArrowLeft'?(index+tabs.length-1)%tabs.length:event.key==='Home'?0:event.key==='End'?tabs.length-1:null;
    if(target!==null) {event.preventDefault();tabs[target].focus();tabs[target].click();}
  });
}
document.querySelector('.screenshot-button')?.addEventListener('click',()=>{
  const target=document.querySelector('#dialog-image'); target.src=image.src;target.alt=image.alt;dialog.showModal();
});
dialog?.addEventListener('click',event=>{if(event.target===dialog)dialog.close();});
