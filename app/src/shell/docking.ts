import type {DockviewApi,IDockviewPanel,DockviewGroupPanel} from 'dockview-react';
import {listen} from '@tauri-apps/api/event';
import {api as backend} from '../core/api';
import {bus} from '../core/bus';
import {useEditor} from '../core/store';
import {useTheme} from '../core/theme';
import {installShortcuts,setPending} from './shortcuts';
import {useContextMenu} from './ContextMenu';

type DockWindow=Window&{__PCAD_DOCK_LABEL__?:string};
const labelOf=(w:Window)=>{try{return (w as DockWindow).__PCAD_DOCK_LABEL__;}catch{return undefined;}};
export function focusPanelWindow(panel:IDockviewPanel){
  const surface=panel.api.getWindow();surface.focus();
  const label=labelOf(surface);if(label)void backend.dockFocusWindow(label).catch(()=>{});
}
/** The home tab anchors the main window; closing a detached window returns its tabs. */
export function dockPanel(api:DockviewApi,panel:IDockviewPanel){
  const target=api.getPanel('home')?.group??api.groups.find(g=>g.api.location.type==='grid'&&g!==panel.group);
  if(target){panel.api.moveTo({group:target,position:'center'});panel.api.setActive();}
}
export async function detachPanel(api:DockviewApi,panel:IDockviewPanel,position?:{left:number;top:number;width:number;height:number}){
  if(panel.id==='home')return false;
  if(panel.api.location.type==='popout'){focusPanelWindow(panel);return true;}
  try{
    const ok=await api.addPopoutGroup(panel,{popoutUrl:'/popout.html',position});
    if(!ok){const message="Could not open the detached view. Allow application windows and try again.";useEditor.getState().setHint(message);useEditor.getState().log("warning",message,{source:"Windows"});}
    return ok;
  }catch(error){
    useEditor.getState().log('warning','Could not detach view: '+String(error),{source:'Windows'});
    return false;
  }
}
/** Shared opener state: no second application, document copy or second undo stack. */
export function installDocking(api:DockviewApi){
  const disposables:{dispose():void}[]=[];
  const surfaces=new Map<Window,()=>void>();
  let disposed=false,dropVersion=0;
  const nativeClose=(w:Window)=>{const label=labelOf(w);if(label)void backend.dockCloseWindow(label).catch(()=>{});};
  const track=(surface:Window)=>{
    if(surfaces.has(surface))return;
    const onDrop=()=>{dropVersion++;};
    const returnTabs=()=>{for(const entry of api.getPopouts().filter(p=>p.window===surface))for(const panel of [...entry.group.panels])dockPanel(api,panel);};
    if(surface!==window)surface.addEventListener('beforeunload',returnTabs,true);
    surface.document.addEventListener('drop',onDrop,true);
    const theme=()=>{surface.document.documentElement.dataset.theme=useTheme.getState().resolved;};
    theme();const offTheme=useTheme.subscribe(theme);
    const offKeys=surface===window?()=>{}:installShortcuts(surface);
    surfaces.set(surface,()=>{
      surface.document.removeEventListener('drop',onDrop,true);
      if(surface!==window)surface.removeEventListener('beforeunload',returnTabs,true);
      offKeys();offTheme();setPending(null);useContextMenu.getState().close();
    });
  };
  track(window);
  disposables.push(api.onDidAddPopoutGroup(({window:surface})=>track(surface)));
  disposables.push(api.onWillClosePopoutWindow(({window:surface})=>{
    surfaces.get(surface)?.();surfaces.delete(surface);nativeClose(surface);
  }));
  const offToggle=bus.on('togglePanelWindow',({id})=>{
    const panel=api.getPanel(id);if(!panel)return;
    if(panel.api.location.type==='popout')dockPanel(api,panel);
    else void detachPanel(api,panel);
  });
  let unlisten:(()=>void)|undefined;
  void listen<string>('pcad-dock-close',({payload})=>{
    const popouts=api.getPopouts().filter(p=>labelOf(p.window)===payload);
    for(const entry of popouts)for(const panel of [...entry.group.panels])dockPanel(api,panel);
    if(popouts.length)nativeClose(popouts[0].window);
  }).then(off=>{if(disposed)off();else unlisten=off;}).catch(()=>{});

  // Native tab drag outside every app surface creates a window at the drop point.
  const drag=(native:DragEvent|PointerEvent,item:IDockviewPanel|DockviewGroupPanel)=>{
    if(!('dataTransfer' in native))return;
    const panel='panels' in item?item.activePanel:item;if(!panel||panel.id==='home')return;
    const owner=panel.api.getWindow(),group=panel.group,initialDrop=dropVersion;
    const finish=(event:DragEvent)=>{
      if(disposed||event.dataTransfer?.dropEffect!=='none'||initialDrop!==dropVersion||panel.group!==group)return;
      const {screenX:x,screenY:y}=event;if(!x&&!y)return;
      const inside=[...surfaces.keys()].some(w=>!w.closed&&x>=w.screenX&&y>=w.screenY&&x<=w.screenX+w.outerWidth&&y<=w.screenY+w.outerHeight);
      if(inside)return;
      if(panel.api.location.type==='popout')return;
      void detachPanel(api,panel,{left:x-180,top:y-20,width:Math.max(640,owner.innerWidth*.7),height:Math.max(480,owner.innerHeight*.8)});
    };
    owner.addEventListener('dragend',finish,{once:true});
  };
  disposables.push(api.onWillDragPanel(e=>drag(e.nativeEvent,e.panel)));
  disposables.push(api.onWillDragGroup(e=>drag(e.nativeEvent,e.group)));
  return ()=>{
    disposed=true;offToggle();unlisten?.();disposables.forEach(d=>d.dispose());
    for(const off of surfaces.values())off();surfaces.clear();
  };
}
