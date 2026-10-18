/* THROWAWAY. No production API calls, no browser storage, no model inference. */
(() => {
  'use strict';
  const P = window.PrototypeState, $ = id => document.getElementById(id);
  const clamp = (x,a,b) => Math.min(b,Math.max(a,x));
  const esc = x => String(x).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const round = x => Math.round(x*100)/100;
  const fmt = x => { x=Math.max(0,x||0);return `${String(Math.floor(x/60)).padStart(2,'0')}:${(x%60).toFixed(2).padStart(5,'0')}`; };
  const span = r => `${fmt(r.start)} — ${fmt(r.end)}`;
  const variant='A';
  if(new URLSearchParams(location.search).get('variant')!=='A'){const url=new URL(location.href);url.searchParams.set('variant','A');history.replaceState(null,'',url);}
  let state=P.createState(), selectedClip=null, sourceTime=10, sequenceTime=10, timelineSpan=120;
  let zoom={start:0,end:120}, folded=false, drag=null, saved=null, programPlaying=false, playingClip=null;
  let playbackRAF=0,lastPlaybackFrame=0;
  let analysis={status:'ready',progress:1,queue:0,track:'游戏与队伍声音'}, analysisTimer=null, imported=[];
  let lastAction='从 A1 的声音子行拖入剪辑轨，声画自动接到末尾。', guide={name:'trim',step:0}, localURL=null;
  const sourceVideo=$('source-video'), programVideo=$('program-video');
  const sequence=()=>P.activeSequence(state), event=()=>state.events.find(e=>e.id===state.selectedId);
  const range=()=>state.ranges[state.selectedId];
  const clip=()=>sequence().clips.find(c=>c.id===selectedClip);
  const editable=()=>!sequence().readOnly;
  const labelKind=k=>k==='gun'?'枪声候选':k==='voice'?'人声候选':'手动补片';
  function note(message,showToast=false){lastAction=message;$('last-action').textContent=message;$('footer-status').textContent=message;if(showToast){$('toast').textContent=message;$('toast').hidden=false;clearTimeout(note.timer);note.timer=setTimeout(()=>$('toast').hidden=true,3200);}}
  function wave(seed=1){let bars='';for(let i=0;i<90;i++){const h=4+Math.abs(Math.sin(i*.79+seed)*Math.cos(i*.21+seed))*24;bars+=`<path d="M${i*5+2} ${20-h/2}v${h}"/>`;}return `<svg class="waveform" viewBox="0 0 450 40" preserveAspectRatio="none" aria-hidden="true"><g stroke="currentColor" stroke-width="2">${bars}</g></svg>`;}
  function pct(t,v){return (t-v.start)/(v.end-v.start)*100;}
  function ruler(v,cls='ruler'){let html='';for(let i=0;i<=4;i++){const t=v.start+(v.end-v.start)*i/4;html+=`<span style="left:${i*25}%">${fmt(t).split('.')[0]}</span>`;}return `<div class="${cls}">${html}</div>`;}
  function overlays(e,v){const r=state.ranges[e.id],a=Math.max(r.start,v.start),b=Math.min(r.end,v.end);if(b<=a)return '';
    const evA=clamp((e.start-a)/(b-a)*100,0,100),evB=clamp((e.end-a)/(b-a)*100,0,100);
    return `<div class="range-overlay" data-event="${e.id}" data-view-start="${v.start}" data-view-end="${v.end}" style="left:${pct(a,v)}%;width:${pct(b,v)-pct(a,v)}%" title="${esc(e.label)} · ${span(r)} · 拖到剪辑轨自动拼接" tabindex="0" role="button" aria-label="${esc(e.label)}待剪范围，${span(r)}，拖动到剪辑轨或按 Shift Enter 追加到末尾"><span class="evidence-mark" style="left:${evA}%;width:${evB-evA}%"></span><span class="range-handle left" data-edge="start" role="slider" tabindex="0" aria-label="${esc(e.label)}起点" aria-valuenow="${r.start}" aria-valuemin="0" aria-valuemax="${r.end}">Ⅰ</span><span class="range-handle right" data-edge="end" role="slider" tabindex="0" aria-label="${esc(e.label)}终点" aria-valuenow="${r.end}" aria-valuemin="${r.start}" aria-valuemax="${state.source.duration}">Ⅰ</span></div>`;
  }
  function lane(kind,v,options={}){const events=state.events.filter(e=>e.kind===kind),sel=event();let content='';
    if(options.cards){content=events.map(e=>`<div class="event-chip ${e.id===state.selectedId?'selected':''}" data-event="${e.id}" tabindex="0" role="button" title="${esc(e.description)}">${fmt(e.start).split('.')[0]} <span>${P.overlapInfo(state,e.id).status==='unused'?'': '· '+P.overlapInfo(state,e.id).label}</span></div>`).join('');return `<div class="lane cards-row ${kind}"><div class="lane-label"><i></i>${kind==='gun'?'枪声':kind==='voice'?'人声':'补片'}</div><div class="event-cards">${content}</div></div>`;}
    for(const e of events){if(e.end<v.start||e.start>v.end)continue;const a=Math.max(e.start,v.start),b=Math.min(e.end,v.end);const used=P.overlapInfo(state,e.id).status!=='unused';content+=`<div class="event ${e.id===state.selectedId?'selected':''} ${used?'used':''}" data-event="${e.id}" style="left:${pct(a,v)}%;width:${Math.max(.5,pct(b,v)-pct(a,v))}%" tabindex="0" role="button" aria-label="${esc(e.label)} ${span(e)}" title="${esc(e.label)} · 识别区间 ${span(e)}"></div>`;}
    if(sel&&sel.kind===kind)content+=overlays(sel,v);
    return `<div class="lane ${kind}"><div class="lane-label"><i></i>${kind==='gun'?'枪声':kind==='voice'?'人声':'补片'}</div><div class="lane-track" data-kind="${kind}" data-view-start="${v.start}" data-view-end="${v.end}">${content}</div></div>`;
  }
  function renderRecognition(){
    $('recognition-content').hidden=folded||analysis.status!=='ready';
    $('fold-arrow').textContent=folded?'›':'⌄';
    const hasManual=state.events.some(e=>e.kind==='manual');
    $('recognition').innerHTML=lane('gun',zoom)+lane('voice',zoom)+(hasManual?lane('manual',zoom):'');
    const r=range();$('range-description').textContent=r?`待剪 ${span(r)} · ${(r.end-r.start).toFixed(2)} s${r.edited?' · 已调整':''}`:'请选择声音子行';
    $('zoom-label').textContent=zoom.start===0&&zoom.end>=state.source.duration?'全片':`${Math.round(state.source.duration/(zoom.end-zoom.start))}×`;
    $('range-reset').disabled=!r||!r.edited;
    $('analysis-progress').hidden=!['running','waiting','cancelled'].includes(analysis.status);
    const badge=$('analysis-badge');if(badge){badge.textContent=analysis.status==='running'?'模拟分析中':analysis.status==='cancelled'?'已中断':analysis.status==='waiting'?'待选音轨':'2 类声音 · 演示';badge.className=`status-pill ${analysis.status==='running'?'analyzing':analysis.status==='cancelled'?'cancelled':''}`;}
    $('analysis-text').textContent=analysis.status==='cancelled'?'分析已中断 · 可以从导入菜单重试':analysis.status==='waiting'?'等待选择用于分析的音轨':`正在分析演示素材 ${Math.round(analysis.progress*100)}%${analysis.queue>1?` · 还有 ${analysis.queue-1} 个排队`:''}`;
    $('cancel-analysis').hidden=analysis.status!=='running';$('progress-fill').style.width=`${analysis.progress*100}%`;
  }
  function renderInspector(){const e=event(),r=range(),c=clip(),isClip=!!c,used=P.overlapInfo(state,e.id);let html='';
    if(c){html=`<div class="selection-type">时间线片段 ${sequence().readOnly?'· 原稿只读':''}</div><h3>${esc(c.label)}</h3><div class="info-row"><span>轨道</span><span>剪辑轨 · 声画关联</span></div><div class="info-row"><span>时间线位置</span><span>${fmt(c.start)} → ${fmt(c.start+c.duration)}</span></div><div class="range-fields"><label>原片起点 / 秒<input id="clip-start" type="number" min="0" max="${state.source.duration}" step="0.1" value="${round(c.sourceStart)}" ${!editable()?'disabled':''}></label><label>原片终点 / 秒<input id="clip-end" type="number" min="0" max="${state.source.duration}" step="0.1" value="${round(c.sourceEnd)}" ${!editable()?'disabled':''}></label></div><div class="info-row"><span>声画</span><span>已关联 · ${c.duration.toFixed(2)} s</span></div><p class="scope-note">这里的修改只影响当前时间线。<br>原片待剪范围保持不变。</p><button id="back-to-source" class="plain small" style="padding:5px 0;color:#9bbbf2">↖ 回到原片选片</button>`;}
    else{html=`<div class="selection-type ${e.kind}">${labelKind(e.kind)} · 演示标记</div><h3>${esc(e.label)}</h3><div class="info-row"><span>识别事件</span><span>${span(e)}</span></div><div class="range-fields"><label>待剪起点 / 秒<input id="range-start" type="number" step="0.1" min="0" max="${state.source.duration}" value="${round(r.start)}"></label><label>待剪终点 / 秒<input id="range-end" type="number" step="0.1" min="0" max="${state.source.duration}" value="${round(r.end)}"></label></div><div class="info-row"><span id="range-summary-kind">${r.edited?'人工调整':'默认留白'}</span><span id="range-summary-value">${(r.end-r.start).toFixed(2)} s</span></div><div class="info-row"><span>当前时间线</span><span class="used-label" id="range-usage">${esc(used.label)}</span></div><p class="scope-note">识别事件与待剪范围分别保留。<br>拖入后，已剪片段与这里独立修改。</p>`;}
    $('inspector-content').innerHTML=html;
    if(!isClip){for(const key of ['start','end']){$(`range-${key}`).oninput=()=>{const a=Number($('range-start').value),b=Number($('range-end').value);if(!Number.isFinite(a)||!Number.isFinite(b)||!(b>a))return;P.setRange(state,e.id,a,b);seekSource(key==='start'?a:b);renderRecognition();renderState();const rr=state.ranges[e.id];$('range-summary-kind').textContent=rr.edited?'人工调整':'默认留白';$('range-summary-value').textContent=`${(rr.end-rr.start).toFixed(2)} s`;$('range-usage').textContent=P.overlapInfo(state,e.id).label;note(`已调整 ${e.label}：${span(state.ranges[e.id])}。识别事件不变。`);};$(`range-${key}`).onchange=()=>{const a=Number($('range-start').value),b=Number($('range-end').value);if(!(b>a)){note('终点需要晚于起点。',true);renderInspector();return;}P.setRange(state,e.id,a,b);seekSource(key==='start'?a:b);render();note(`已调整 ${e.label}：${span(state.ranges[e.id])}。识别事件不变。`);};}}
    else{for(const key of ['start','end']){$(`clip-${key}`).onchange=()=>{const a=Number($('clip-start').value),b=Number($('clip-end').value);if(!(b>a)){renderInspector();return;}P.trimClip(state,c.id,a,b);render();note('时间线片段已调整；原片待剪范围没有变化。');};}$('back-to-source').onclick=()=>{selectedClip=null;const id=c.eventId||(c.eventIds&&c.eventIds[0]);if(id)P.selectEvent(state,id);seekSource(c.sourceStart);render();};}
    $('create-draft').disabled=analysis.status!=='ready';$('last-action').textContent=lastAction;
  }
  function renderTimeline(){
    const seq=sequence(),duration=P.sequenceDuration(seq);timelineSpan=zoom.end-zoom.start;
    $('sequence-select').innerHTML=state.sequences.map(s=>`<option value="${s.id}" ${s.id===seq.id?'selected':''}>${esc(s.name)}</option>`).join('');
    $('readonly-badge').hidden=!seq.readOnly;$('delete-clip').disabled=!clip()||seq.readOnly;$('undo').disabled=!state.history.length;
    $('timeline-stats').textContent=`1 条剪辑轨 · ${seq.clips.length} 段 · 自动拼接`;
    $('timeline-ruler').innerHTML=Array.from({length:7},(_,i)=>`<span style="left:${i/6*100}%">${fmt(zoom.start+timelineSpan*i/6).split('.')[0]}</span>`).join('');
    const originLeft=pct(0,zoom),originWidth=state.source.duration/timelineSpan*100;
    $('video-track').innerHTML=`<div class="source-clip" style="left:${originLeft}%;width:${originWidth}%"><span class="filmstrip"></span><span class="source-label">${esc(state.source.name)}</span><span class="source-reference">原片参考 · 保留</span></div>`;
    $('audio-track').innerHTML=`<div class="source-clip" style="left:${originLeft}%;width:${originWidth}%">${wave(13)}<span class="source-label">原片声音 · 分类结果展开在下方</span></div><span id="analysis-badge" class="status-pill">2 类声音 · 演示</span>`;
    const clips=seq.clips.filter(c=>c.start<zoom.end&&c.start+c.duration>zoom.start).map(c=>`<div class="timeline-clip linked-clip ${c.kind} ${selectedClip===c.id?'selected':''}" data-clip="${c.id}" tabindex="0" role="button" aria-label="声画片段 ${esc(c.label)} 原片${fmt(c.sourceStart)}至${fmt(c.sourceEnd)} 成片${fmt(c.start)}至${fmt(c.start+c.duration)}" style="left:${pct(c.start,zoom)}%;width:${c.duration/timelineSpan*100}%"><span class="clip-name">${esc(c.label)} · ${c.duration.toFixed(1)}s</span><span class="filmstrip"></span><span class="clip-audio">${wave(c.sourceStart)}</span><span class="clip-link" aria-hidden="true">∞</span>${!seq.readOnly?'<span class="clip-trim-handle left" data-clip-edge="start"></span><span class="clip-trim-handle right" data-clip-edge="end"></span>':''}</div>`).join('');
    const tail=pct(Math.max(duration,zoom.start),zoom);
    const hint=seq.readOnly?'组稿原稿只读，请切换到编辑副本':seq.clips.length?'拖到这里，自动接到末尾':'拖入声音高亮，声画一起从 0 秒拼接';
    $('edit-track').innerHTML=clips+(tail<100?`<div class="append-hint" style="left:${Math.max(0,tail)}%;width:${100-Math.max(0,tail)}%"><span>${hint}</span></div>`:'');
    $('edit-track').classList.toggle('readonly-track',seq.readOnly);
    $('program-empty').hidden=seq.clips.length>0;$('program-video').parentElement.classList.toggle('empty',!seq.clips.length);
    $('program-label').textContent=seq.name;$('sequence-duration').textContent=`剪辑 ${fmt(duration)}`;
    $('selection-info').textContent=clip()?`${clip().label} · 声画关联 · 后续自动衔接`:'原片保留 · 声画一起编辑';
    $('program-note').textContent=seq.readOnly?'组稿原稿只读；原片轨只作定位参考。':'一条剪辑轨，声画关联、顺序拼接；原片只作参考。';
    updatePlayhead();
  }
  function renderState(){const rows=state.events.map(e=>{const r=state.ranges[e.id],u=P.overlapInfo(state,e.id);return `<tr><td>${esc(e.label)}</td><td>${span(e)}</td><td>${span(r)}</td><td>${r.edited?'已调整':'默认'}</td><td>${esc(u.label)}</td></tr>`;}).join('');
    $('state-body').innerHTML=`<div class="state-summary"><div>素材时长<strong>${fmt(state.source.duration)}</strong></div><div>任务状态<strong>${analysis.status==='ready'?'演示完成':analysis.status==='running'?'模拟分析中':analysis.status==='waiting'?'待选音轨':'已中断'}</strong></div><div>内存快照<strong>${saved?'已保存':'未保存'}</strong></div></div><p class="scope-note">${esc(state.source.name)} · 标记为预设演示数据 · A1 下的识别子行 · V1/A1 原片参考 · 不代表模型识别效果</p><h3 class="state-section">原片事件与当前待剪范围</h3><table class="state-table"><thead><tr><th>类别</th><th>识别事件</th><th>待剪范围</th><th>状态</th><th>使用情况</th></tr></thead><tbody>${rows}</tbody></table>${state.sequences.map(s=>`<h3 class="state-section">${esc(s.name)} ${s.readOnly?'· 原稿只读':''} ${s.id===state.activeSequenceId?'· 当前打开':''}</h3><table class="state-table"><thead><tr><th>片段 / 轨道</th><th>原片范围</th><th>成片范围</th><th>关联声画</th></tr></thead><tbody>${s.clips.map(c=>`<tr><td>${esc(c.label)} · 声画剪辑</td><td>${fmt(c.sourceStart)}–${fmt(c.sourceEnd)}</td><td>${fmt(c.start)}–${fmt(c.start+c.duration)}</td><td>是</td></tr>`).join('')||'<tr><td colspan="4">没有片段</td></tr>'}</tbody></table>`).join('')}`;
  }
  function render(){renderTimeline();renderRecognition();renderInspector();$('source-name').textContent=state.source.name;$('asset-name').textContent=state.source.name;$('asset-name').title=state.source.name;$('restore-sim').disabled=!saved;renderState();}
  function select(id,seek=true){const e=P.selectEvent(state,id);if(!e)return;selectedClip=null;if(seek)seekSource(e.start);if(zoom.end-zoom.start<state.source.duration&&!(e.start>=zoom.start&&e.end<=zoom.end))focusZoom();render();note(`${e.label}：识别 ${span(e)}，待剪 ${span(state.ranges[id])}。`);}
  function seekSource(t,moveTimeline=true){sourceVideo.pause();if(programPlaying)stopProgram();sourceTime=clamp(t,0,state.source.duration-.02);if(moveTimeline)seekSequence(sourceTime);if(sourceVideo.readyState>=1)sourceVideo.currentTime=sourceTime;$('source-time').textContent=fmt(sourceTime);}
  function updatePlayhead(){
    const rect=$('video-track').getBoundingClientRect(),parent=$('timeline-scroll').getBoundingClientRect(),scroll=$('timeline-scroll');
    const within=sequenceTime>=zoom.start&&sequenceTime<=zoom.end;$('timeline-playhead').hidden=!within;
    $('timeline-playhead').style.left=`${rect.left-parent.left+(sequenceTime-zoom.start)/(zoom.end-zoom.start)*rect.width}px`;
    $('timeline-playhead').style.top=`${scroll.scrollTop+21}px`;$('timeline-playhead').style.bottom='auto';$('timeline-playhead').style.height=`${Math.max(0,scroll.clientHeight-21)}px`;
    $('timeline-time').textContent=fmt(sequenceTime);$('sequence-time').textContent=fmt(sequenceTime);
  }
  function seekSequence(t){sequenceTime=clamp(t,0,Math.max(state.source.duration,P.sequenceDuration(sequence())));const c=P.clipAtTime(state,sequenceTime);playingClip=c||null;if(c){const src=c.sourceStart+sequenceTime-c.start;if(programVideo.readyState>=1)programVideo.currentTime=clamp(src,0,state.source.duration-.02);$('program-source').textContent=`原片 ${fmt(src)}`;programVideo.style.visibility='visible';}else{programVideo.pause();programVideo.style.visibility=sequence().clips.length?'hidden':'visible';$('program-source').textContent=sequence().clips.length?'时间线空白':'';}updatePlayhead();}
  function insert(id,at){
    if(analysis.status!=='ready'){note('演示分析尚未完成，请先完成或重试。',true);return;}
    if(!editable()){note('原稿只读，请先切换到编辑副本。',true);return;}
    stopProgram();const used=P.overlapInfo(state,id),result=P.insertClip(state,id,at);if(!result)return;
    selectedClip=result.clip.id;sequenceTime=result.clip.start;
    if((zoom.start===0&&zoom.end>=state.source.duration)||result.clip.start<zoom.start||result.clip.start+result.clip.duration>zoom.end)zoom={start:0,end:Math.max(state.source.duration,P.sequenceDuration(sequence()))};
    render();seekSequence(sequenceTime);
    note(`已${result.clip.start+result.clip.duration>=P.sequenceDuration(sequence())-.001?'追加':'插入'} ${result.clip.duration.toFixed(2)} 秒声画，片段自动拼接。${used.status!=='unused'?'重复的原片内容已保留。':''}`,true);
  }
  function focusZoom(){const r=range(),pad=Math.max(3,(r.end-r.start)*.25);zoom={start:Math.max(0,r.start-pad),end:Math.min(state.source.duration,r.end+pad)};}
  function cancelDrag(){if(drag?.type==='range')P.setRange(state,drag.id,drag.original.start,drag.original.end);drag=null;document.querySelectorAll('.active-drop').forEach(e=>e.classList.remove('active-drop'));$('drag-ghost').hidden=true;$('drop-line').hidden=true;document.body.style.cursor='';render();}
  function pointerTime(e,rect,v){return clamp(v.start+(e.clientX-rect.left)/rect.width*(v.end-v.start),v.start,v.end);}
  $('recognition').addEventListener('pointerdown',ev=>{if(ev.button!==0||analysis.status!=='ready')return;ev.stopPropagation();const edge=ev.target.closest('[data-edge]'),node=ev.target.closest('[data-event]');
    if(node){ev.preventDefault();const id=node.dataset.event;const r=state.ranges[id];
      if(edge){selectedClip=null;P.selectEvent(state,id);const host=node.parentElement;const rect=host.getBoundingClientRect();const v={start:Number(node.dataset.viewStart??host.dataset.viewStart??zoom.start),end:Number(node.dataset.viewEnd??host.dataset.viewEnd??zoom.end)};drag={type:'range',id,edge:edge.dataset.edge,rect,v,original:{...r}};document.body.style.cursor='ew-resize';}
      else{drag={type:'event',id,x:ev.clientX,y:ev.clientY,moved:false};select(id);}
    }else{const track=ev.target.closest('.lane-track,.focus-lens');if(track){ev.preventDefault();const rect=track.getBoundingClientRect(),v={start:Number(track.dataset.viewStart),end:Number(track.dataset.viewEnd)};drag={type:'manual',rect,v,start:pointerTime(ev,rect,v),x:ev.clientX,y:ev.clientY};}}
  });
  window.addEventListener('pointermove',ev=>{if(!drag)return;
    if(drag.type==='range'){const t=round(pointerTime(ev,drag.rect,drag.v));const r=state.ranges[drag.id];const a=drag.edge==='start'?Math.min(t,r.end-.25):r.start,b=drag.edge==='end'?Math.max(t,r.start+.25):r.end;P.setRange(state,drag.id,a,b);seekSource(drag.edge==='start'?a:b);renderRecognition();renderInspector();return;}
    if(drag.type==='clip-trim'){const delta=(ev.clientX-drag.x)/$('video-track').getBoundingClientRect().width*timelineSpan;drag.value=clamp(drag.original[drag.edge==='start'?'sourceStart':'sourceEnd']+delta,0,state.source.duration);seekSource(drag.value,false);sequenceTime=drag.original.start+(drag.edge==='end'?drag.value-drag.original.sourceStart:0);updatePlayhead();$('selection-info').textContent=`调整${drag.edge==='start'?'起点':'终点'}预览：${fmt(drag.value)}（松手提交）`;return;}
    if(drag.type==='manual'){const end=pointerTime(ev,drag.rect,drag.v);$('range-description').textContent=`手动划选 ${fmt(Math.min(drag.start,end))} — ${fmt(Math.max(drag.start,end))}`;seekSource(end);return;}
    if(drag.type==='event'){
      if(Math.hypot(ev.clientX-drag.x,ev.clientY-drag.y)<5&&!drag.moved)return;
      drag.moved=true;const e=state.events.find(e=>e.id===drag.id),r=state.ranges[drag.id];
      $('drag-ghost').hidden=false;$('drag-ghost').style.left=`${ev.clientX+15}px`;$('drag-ghost').style.top=`${ev.clientY+12}px`;
      $('drag-ghost').innerHTML=`${esc(e.label)} · ${(r.end-r.start).toFixed(2)} s<small>${span(r)} · 复制画面与声音</small>`;document.body.style.cursor='grabbing';
      document.querySelectorAll('.active-drop').forEach(e=>e.classList.remove('active-drop'));
      const target=document.elementFromPoint(ev.clientX,ev.clientY)?.closest('[data-drop-track]');
      drag.valid=!!target&&editable();$('drop-line').hidden=!drag.valid;
      if(drag.valid){
        const rect=target.getBoundingClientRect(),scroll=$('timeline-scroll'),parent=scroll.getBoundingClientRect();
        const t=zoom.start+(ev.clientX-rect.left)/rect.width*(zoom.end-zoom.start);
        drag.at=P.resolveInsertTime(sequence(),t);target.classList.add('active-drop');
        const inView=drag.at>=zoom.start&&drag.at<=zoom.end;
        $('drop-line').hidden=!inView;
        if(!inView)$('drag-ghost').innerHTML+=`<small>追加到成片 ${fmt(drag.at)} · 末尾在当前视野外</small>`;
        const x=clamp((drag.at-zoom.start)/(zoom.end-zoom.start)*rect.width,0,rect.width);
        $('drop-line').style.left=`${rect.left-parent.left+x}px`;
        $('drop-line').style.top=`${rect.top-parent.top+scroll.scrollTop}px`;$('drop-line').style.height=`${rect.height}px`;
        $('drop-line').firstElementChild.textContent=`${drag.at>=P.sequenceDuration(sequence())?'接到末尾':'插入片段之间'} ${fmt(drag.at)}`;
      }
    }
  });
  window.addEventListener('pointerup',ev=>{if(!drag)return;const d=drag;drag=null;document.querySelectorAll('.active-drop').forEach(e=>e.classList.remove('active-drop'));document.body.style.cursor='';$('drag-ghost').hidden=true;$('drop-line').hidden=true;
    if(d.type==='event'){if(d.moved&&d.valid)insert(d.id,d.at);else if(d.moved)note('未落入可编辑时间线，本次拖动已取消。');}
    if(d.type==='range'){render();note(`待剪范围已调整为 ${span(state.ranges[d.id])}，识别事件与已剪片段不变。`);}
    if(d.type==='manual'){const end=pointerTime(ev,d.rect,d.v);if(Math.abs(ev.clientX-d.x)>6&&Math.abs(end-d.start)>=.25){const e=P.addManualRange(state,d.start,end);selectedClip=null;render();note(`已创建${e.label}：${span(state.ranges[e.id])}，可直接拖到时间线。`);}else{seekSource(end);}}
    if(d.type==='clip-trim'){const a=d.edge==='start'?Math.min(d.value,d.original.sourceEnd-.25):d.original.sourceStart,b=d.edge==='end'?Math.max(d.value,d.original.sourceStart+.25):d.original.sourceEnd;if(Number.isFinite(a)&&Number.isFinite(b))P.trimClip(state,d.id,a,b);render();note('已调整时间线片段，原片待剪范围保持不变。');}
  });
  $('timeline-scroll').addEventListener('pointerdown',ev=>{if(ev.button!==0||ev.target.closest('#recognition,button,select'))return;const node=ev.target.closest('[data-clip]'),edge=ev.target.closest('[data-clip-edge]');if(node){stopProgram();sourceVideo.pause();selectedClip=node.dataset.clip;const c=clip();if(edge&&editable()){drag={type:'clip-trim',id:c.id,edge:edge.dataset.clipEdge,x:ev.clientX,original:{...c},value:c[edge.dataset.clipEdge==='start'?'sourceStart':'sourceEnd']};ev.preventDefault();}sequenceTime=c.start;render();seekSequence(sequenceTime);}else{const rect=$('video-track').getBoundingClientRect();if(ev.clientX>=rect.left){selectedClip=null;const t=zoom.start+(ev.clientX-rect.left)/rect.width*(zoom.end-zoom.start);if(ev.target.closest('.source-canvas,#timeline-ruler')){stopProgram();seekSource(t);}else{seekSequence(t);}renderInspector();}}});
  $('timeline-scroll').addEventListener('dblclick',ev=>{const node=ev.target.closest('[data-clip]');if(!node)return;selectedClip=node.dataset.clip;const c=clip();const id=c.eventId||c.eventIds[0];if(id)select(id,false);seekSource(c.sourceStart);render();note(`已回到原片 ${fmt(c.sourceStart)}。`);});
  $('recognition').addEventListener('keydown',ev=>{const node=ev.target.closest('[data-event]');if(!node)return;if(ev.target.dataset.edge&&['ArrowLeft','ArrowRight'].includes(ev.key)){ev.preventDefault();ev.stopPropagation();const r=state.ranges[node.dataset.event],edge=ev.target.dataset.edge,delta=(ev.key==='ArrowRight'?1:-1)*(ev.shiftKey?1:.1);P.setRange(state,node.dataset.event,edge==='start'?Math.min(r.start+delta,r.end-.25):r.start,edge==='end'?Math.max(r.end+delta,r.start+.25):r.end);seekSource(state.ranges[node.dataset.event][edge]);render();document.querySelector(`[data-event="${node.dataset.event}"] [data-edge="${edge}"]`)?.focus();note('已通过键盘调整边界。');}else if(ev.key==='Enter'&&!ev.shiftKey){ev.preventDefault();select(node.dataset.event);}});
  window.addEventListener('keydown',ev=>{
    if(ev.key==='Escape'&&drag){ev.preventDefault();cancelDrag();note('拖动已取消。');return;}
    if(ev.target.closest('input,textarea,select,[contenteditable=true]')||document.querySelector('dialog[open]'))return;
    const focusedClip=ev.target.closest('[data-clip]');
    if(ev.key==='Enter'&&ev.shiftKey){ev.preventDefault();insert(ev.target.closest('[data-event]')?.dataset.event||state.selectedId);}
    else if(focusedClip&&(ev.key==='Enter'||ev.key===' ')){
      ev.preventDefault();stopProgram();sourceVideo.pause();selectedClip=focusedClip.dataset.clip;
      const c=clip();render();seekSequence(c.start);document.querySelector(`[data-clip="${c.id}"]`)?.focus();
    }else if(ev.key==='ArrowLeft'||ev.key==='ArrowRight'){
      ev.preventDefault();const t=clamp(sequenceTime+(ev.key==='ArrowRight'?1:-1)/60,0,Math.max(state.source.duration,P.sequenceDuration(sequence())));
      if(P.clipAtTime(state,t)){seekSequence(t);}else{seekSource(t,false);sequenceTime=t;updatePlayhead();}
    }else if((ev.ctrlKey||ev.metaKey)&&ev.key==='z'){ev.preventDefault();undo();}
    else if(ev.key==='Delete'||ev.key==='Backspace'){
      if(focusedClip)selectedClip=focusedClip.dataset.clip;
      if(clip()&&editable()){ev.preventDefault();remove();}
    }
  });
  function undo(){stopProgram();const label=P.undo(state);selectedClip=null;sequenceTime=0;render();seekSequence(0);note(label?`已撤销：${label}。原片待剪范围保留。`:'没有可撤销的时间线操作。');}
  function remove(){stopProgram();const c=clip();if(!c||!editable())return;P.removeClip(state,c.id);selectedClip=null;render();seekSequence(Math.min(sequenceTime,P.sequenceDuration(sequence())));note('片段已删除，声画一起移除，后续片段自动合拢；可撤销。');}
  function draft(){stopProgram();const result=P.createDraft(state,$('include-voice').checked);if(!result)return;selectedClip=null;sequenceTime=0;zoom={start:0,end:Math.max(state.source.duration,P.sequenceDuration(result.copy))};render();seekSequence(0);note(`已按当前范围生成 ${result.mergedRanges.length} 段草稿。原稿只读，当前打开编辑副本；手动选片保留。`,true);}
  $('undo').onclick=undo;$('delete-clip').onclick=remove;$('create-draft').onclick=draft;
  $('sequence-select').onchange=()=>{stopProgram();P.switchSequence(state,$('sequence-select').value);selectedClip=null;sequenceTime=0;zoom={start:0,end:Math.max(state.source.duration,P.sequenceDuration(sequence()))};render();seekSequence(0);note(`已打开 ${sequence().name}${sequence().readOnly?'，此版本只读。':'。'}`);};
  function fitTimeline(){zoom={start:0,end:Math.max(state.source.duration,P.sequenceDuration(sequence()))};render();}
  $('fit-timeline').onclick=fitTimeline;$('zoom-reset').onclick=fitTimeline;
  $('range-reset').onclick=()=>{const e=event(),pad=e.kind==='gun'?5:e.kind==='voice'?1:0;P.setRange(state,e.id,Math.max(0,e.start-pad),Math.min(state.source.duration,e.end+pad));render();note('待剪范围已恢复默认，已拖入片段不变。');};
  $('toggle-lanes').onclick=()=>{folded=!folded;renderRecognition();};
  $('zoom-in').onclick=()=>{focusZoom();render();note('原片、识别子行和剪辑轨使用同一放大时间尺。');};
  $('zoom-out').onclick=()=>{const mid=(zoom.start+zoom.end)/2,width=(zoom.end-zoom.start)*2,max=Math.max(state.source.duration,P.sequenceDuration(sequence()));zoom={start:Math.max(0,mid-width/2),end:Math.min(max,mid+width/2)};render();};
  function nextEvent(d){const list=state.events.slice().sort((a,b)=>a.start-b.start),i=list.findIndex(e=>e.id===state.selectedId);select(list[(i+d+list.length)%list.length].id);}
  $('prev-event').onclick=()=>nextEvent(-1);$('next-event').onclick=()=>nextEvent(1);
  sourceVideo.addEventListener('loadedmetadata',()=>seekSource(sourceTime));sourceVideo.addEventListener('timeupdate',()=>{sourceTime=sourceVideo.currentTime;$('source-time').textContent=fmt(sourceTime);if(!sourceVideo.paused&&!programPlaying)seekSequence(sourceTime);});
  sourceVideo.addEventListener('play',()=>$('play-source').textContent='Ⅱ');sourceVideo.addEventListener('pause',()=>$('play-source').textContent='▶');
  $('play-source').onclick=()=>{stopProgram();sourceVideo.paused?sourceVideo.play().catch(()=>note('当前视频无法播放，请使用演示素材。',true)):sourceVideo.pause();};
  $('program-start').onclick=()=>seekSequence(0);
  function stopProgram(){programPlaying=false;cancelAnimationFrame(playbackRAF);programVideo.pause();$('play-program').textContent='▶';}
  function playbackFrame(now){
    if(!programPlaying)return;
    const dt=Math.min(.12,(now-lastPlaybackFrame)/1000);lastPlaybackFrame=now;
    const duration=P.sequenceDuration(sequence()),t=sequenceTime+dt;
    if(t>=duration){seekSequence(Math.max(0,duration-.08));sequenceTime=duration;updatePlayhead();stopProgram();return;}
    const next=P.clipAtTime(state,t);
    if(next?.id!==playingClip?.id){seekSequence(t);if(next)programVideo.play().catch(()=>stopProgram());}
    else if(next){sequenceTime=t;const target=next.sourceStart+t-next.start;if(programVideo.paused)programVideo.play().catch(()=>stopProgram());if(Math.abs(programVideo.currentTime-target)>.3)programVideo.currentTime=target;}
    else{sequenceTime=t;programVideo.pause();programVideo.style.visibility='hidden';}
    updatePlayhead();playbackRAF=requestAnimationFrame(playbackFrame);
  }
  $('play-program').onclick=()=>{
    if(!sequence().clips.length){note('请先拖入一个片段。',true);return;}
    if(programPlaying){stopProgram();return;}
    sourceVideo.pause();if(sequenceTime>=P.sequenceDuration(sequence())-.05)sequenceTime=0;
    seekSequence(sequenceTime);programPlaying=true;lastPlaybackFrame=performance.now();
    if(playingClip)programVideo.play().catch(()=>stopProgram());
    $('play-program').textContent='Ⅱ';playbackRAF=requestAnimationFrame(playbackFrame);
  };
  programVideo.addEventListener('timeupdate',()=>{if(programPlaying&&playingClip)$('program-source').textContent=`原片 ${fmt(programVideo.currentTime)}`;});
  function saveSnapshot(){saved={project:P.snapshot(state),analysis:{...analysis},imported:structuredClone(imported),includeVoice:$('include-voice').checked,zoom:{...zoom}};render();note('快照已保存到本页内存，可用“模拟重开”恢复；刷新页面会清空。',true);}
  function restoreSnapshot(){if(!saved)return;stopProgram();clearInterval(analysisTimer);state=P.restoreSnapshot(saved.project);analysis={...saved.analysis};if(analysis.status==='running')analysis.status='cancelled';imported=structuredClone(saved.imported);$('include-voice').checked=saved.includeVoice;zoom={...saved.zoom};selectedClip=null;sequenceTime=0;render();seekSource(event().start);seekSequence(0);note('已恢复本页快照。已完成分析和人工范围保留，中断任务不会自动继续。',true);}
  $('save-sim').onclick=saveSnapshot;$('restore-sim').onclick=restoreSnapshot;
  $('state-open').onclick=()=>{renderState();$('state-dialog').showModal();};$('switcher-state').onclick=$('state-open').onclick;
  document.querySelectorAll('.close-dialog').forEach(b=>b.onclick=()=>b.closest('dialog').close());
  document.querySelectorAll('dialog').forEach(d=>d.addEventListener('click',e=>{if(e.target===d){const r=d.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)d.close();}}));
  function reset(){clearInterval(analysisTimer);sourceVideo.pause();stopProgram();if(localURL){URL.revokeObjectURL(localURL);localURL=null;}sourceVideo.src='demo-preview.mp4';programVideo.src='demo-preview.mp4';state=P.createState();selectedClip=null;sourceTime=10;sequenceTime=0;timelineSpan=120;zoom={start:0,end:120};analysis={status:'ready',progress:1,queue:0,track:'游戏与队伍声音'};imported=[];saved=null;$('include-voice').checked=false;$('import-items').textContent='';render();seekSource(10);note('原型已重置。所有数据仅在本次页面中。');}
  $('reset-all').onclick=()=>{$('state-dialog').close();reset();};
  function openImport(){$('audio-choice').hidden=true;$('import-dialog').showModal();}
  $('import-trigger').onclick=openImport;$('import-demo').onclick=openImport;
  function startAnalysis(count=1){clearInterval(analysisTimer);analysis={status:'running',progress:0,queue:count,track:analysis.track};imported=Array.from({length:count},(_,i)=>({name:`演示素材 ${i+1}`,status:i===0?'模拟分析中':'排队等待'}));render();note('开始演示自动分析；现有时间线与人工调整保持不变。');analysisTimer=setInterval(()=>{analysis.progress=Math.min(1,analysis.progress+.08);if(analysis.progress>=1){const index=imported.findIndex(x=>x.status==='模拟分析中');if(index>=0)imported[index].status='演示完成';analysis.queue--;if(analysis.queue>0){analysis.progress=0;const next=imported.findIndex(x=>x.status==='排队等待');if(next>=0)imported[next].status='模拟分析中';}else{clearInterval(analysisTimer);analysis.status='ready';note('演示分析完成：已展示预设声音标记，没有自动填入时间线。',true);}}$('import-items').innerHTML=imported.map(x=>`<div>${esc(x.name)} · ${x.status}</div>`).join('');renderRecognition();renderInspector();renderState();},280);}
  document.querySelectorAll('[data-import]').forEach(b=>b.onclick=()=>{const kind=b.dataset.import;if(kind==='multi'){$('audio-choice').hidden=false;analysis.status='waiting';renderRecognition();}else{$('import-dialog').close();startAnalysis(kind==='queue'?2:1);}});
  $('confirm-audio').onclick=()=>{analysis.track=$('audio-track-choice').value;$('import-dialog').close();startAnalysis(1);};
  $('cancel-analysis').onclick=()=>{clearInterval(analysisTimer);analysis.status='cancelled';imported.forEach(x=>{if(x.status!=='演示完成')x.status='已中断';});$('import-items').innerHTML=imported.map(x=>`<div>${esc(x.name)} · ${x.status}</div>`).join('');render();note('已取消演示分析。现有剪辑保持不变。',true);};
  $('import-local').onclick=()=>$('file-input').click();$('file-input').onchange=()=>{const file=$('file-input').files[0];if(!file)return;if(localURL)URL.revokeObjectURL(localURL);localURL=URL.createObjectURL(file);sourceVideo.onloadedmetadata=()=>{state=P.createState();const scale=sourceVideo.duration/120;state.source.duration=sourceVideo.duration;state.source.name=file.name;state.source.id='local-preview';state.events.forEach(e=>{e.start*=scale;e.end*=scale;const pad=e.kind==='gun'?5:1;state.ranges[e.id]={start:Math.max(0,e.start-pad),end:Math.min(state.source.duration,e.end+pad),edited:false};});zoom={start:0,end:state.source.duration};selectedClip=null;sourceTime=0;sequenceTime=0;timelineSpan=120;saved=null;sourceVideo.onloadedmetadata=null;startAnalysis();render();note('本地视频已载入。识别标记仍是演示数据，不是该视频的模型结果。',true);};sourceVideo.onerror=()=>{sourceVideo.onloadedmetadata=null;reset();note('浏览器不支持此视频格式，已恢复演示素材。原文件没有修改。',true);};sourceVideo.src=localURL;programVideo.src=localURL;};
  const guides={trim:{title:'观察三个独立对象',text:'声音事件是 10–12 秒；待剪范围可改为 8–16 秒；拖入后再调整，时间线片段应保持 8–16 秒。',steps:[['选中第一段枪声',()=>select(state.events[0].id)],['把待剪范围调整为 8–16 秒',()=>{P.setRange(state,state.events[0].id,8,16);render();note('待剪范围 8–16 秒，声音事件仍是 10–12 秒。');}],['模拟拖到剪辑轨，自动从 0 秒开始',()=>insert(state.events[0].id,0)],['再把待剪范围改成 7–18 秒',()=>{P.setRange(state,state.events[0].id,7,18);selectedClip=null;render();note('待剪已变成 7–18 秒；时间线保留 8–16 秒。');}]]},overlap:{title:'手动保留重复，一键组稿合并',text:'枪声 02 和人声 02 有重叠。手动拖入允许重复；按当前范围组稿会合并重叠，保留之前的手动序列。',steps:[['拖入枪声 02',()=>insert(state.events.find(e=>e.label==='枪声 02').id,0)],['追加重叠人声，自动接在枪声后面',()=>insert(state.events.find(e=>e.label==='人声 02').id)],['包含人声，按当前范围生成草稿',()=>{$('include-voice').checked=true;draft();}],['查看所有序列的完整状态',()=>{renderState();$('guide-dialog').close();$('state-dialog').showModal();}]]},restore:{title:'模拟重开，不依赖磁盘保存',text:'快照仅在本页内存中。保存后修改范围，再模拟重开，检查范围与时间线是否一同恢复。',steps:[['拖入第一段枪声',()=>insert(state.events[0].id,0)],['保存当前快照',saveSnapshot],['改变待剪范围并删除片段',()=>{P.setRange(state,state.events[0].id,8,14);selectedClip=sequence().clips[0]?.id;remove();}],['模拟重开并恢复',restoreSnapshot]]}};
  function renderGuide(){const g=guides[guide.name],step=g.steps[guide.step];$('guide-body').innerHTML=`<b>${g.title}</b><p>${g.text}</p><div class="guide-step">${step?`第 ${guide.step+1} / ${g.steps.length} 步：${step[0]}`:'体验完成。关闭导览，在原型里自由拖动试试。'}</div>`;$('guide-next').textContent=step?'执行这一步':'完成';document.querySelectorAll('[data-guide]').forEach(b=>b.classList.toggle('active',b.dataset.guide===guide.name));}
  $('guide-open').onclick=()=>{renderGuide();$('guide-dialog').showModal();};document.querySelectorAll('[data-guide]').forEach(b=>b.onclick=()=>{guide={name:b.dataset.guide,step:0};reset();renderGuide();});$('guide-reset').onclick=()=>{reset();guide.step=0;renderGuide();};$('guide-next').onclick=()=>{const step=guides[guide.name].steps[guide.step];if(!step){$('guide-dialog').close();return;}step[1]();guide.step++;renderGuide();};
  $('timeline-scroll').addEventListener('scroll',updatePlayhead);window.addEventListener('resize',updatePlayhead);
  render();seekSequence(0);seekSource(10);
  console.info('[PROTOTYPE ready] A1 recognition child rows and one continuous linked audiovisual lane, fixture markers only',P.snapshot(state));
})();
