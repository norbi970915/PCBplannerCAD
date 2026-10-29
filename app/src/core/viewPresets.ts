import {normalizeView, type LayerMode, type ViewSettings} from './view';
import type {DocKind} from './types';

export type DisplaySettings = Omit<ViewSettings, 'grid' | 'snapToGrid'>;
export interface ViewPreset {name:string;display:DisplaySettings;layerMode:LayerMode;pourFills:boolean}
export const MAX_VIEW_PRESETS=24;
export const viewPresetsKey=(kind:DocKind)=>'pcbplannercad.viewPresets.'+kind;
export function displaySettings(view:unknown):DisplaySettings {
  const {grid:_grid,snapToGrid:_snap,...display}=normalizeView(view);
  return display;
}
export function normalizeViewPresets(value:unknown):ViewPreset[] {
  if(!Array.isArray(value))return [];
  const names=new Set<string>(),out:ViewPreset[]=[];
  for(const entry of value){
    if(!entry||typeof entry!=='object'||typeof entry.name!=='string'||!entry.display||typeof entry.display!=='object')continue;
    const name=entry.name.trim(),key=name.toLowerCase();
    if(!name||name.length>60||names.has(key))continue;
    names.add(key);out.push({name,display:displaySettings(entry.display),layerMode:entry.layerMode==='dim'||entry.layerMode==='only'?entry.layerMode:'all',pourFills:typeof entry.pourFills==='boolean'?entry.pourFills:true});
    if(out.length===MAX_VIEW_PRESETS)break;
  }
  return out;
}
export function readViewPresets(kind:DocKind,storage:Pick<Storage,'getItem'>=localStorage):ViewPreset[]{
  try{const raw=storage.getItem(viewPresetsKey(kind));return raw&&raw.length<=100000?normalizeViewPresets(JSON.parse(raw)):[];}catch{return [];}
}
export function saveViewPreset(list:ViewPreset[],preset:ViewPreset,replace=false):ViewPreset[]{
  const name=preset.name.trim();
  if(!name||name.length>60)throw Error('Use a preset name between 1 and 60 characters.');
  const existing=list.findIndex(p=>p.name.toLowerCase()===name.toLowerCase());
  if(existing>=0&&!replace)throw Error('That preset already exists. Select it and use Replace selected.');
  if(existing<0&&list.length>=MAX_VIEW_PRESETS)throw Error('Remove a preset before adding more (limit 24).');
  const saved=normalizeViewPresets([{...preset,name}])[0];
  if(!saved)throw Error('Invalid display preset.');
  return existing<0?[...list,saved]:list.map((p,i)=>i===existing?saved:p);
}
