import type { DrcViolation, Item, ItemId } from '../core/types';
export interface DrcFilter { query:string; severity:'all'|'error'|'warning'; code:string; layer:number|null }
export function filterDrc(violations:readonly DrcViolation[],items:ReadonlyMap<ItemId,Item>,filter:DrcFilter):DrcViolation[]{
  const query=filter.query.trim().toLowerCase();
  return violations.filter(v=>{
    if(filter.severity!=='all'&&v.severity!==filter.severity||filter.code&&v.code!==filter.code||filter.layer!==null&&v.layer!==filter.layer)return false;
    if(!query)return true;
    const related=v.items.flatMap(id=>{const item=items.get(id);return [String(id),item?.kind==='footprint'?item.designator:'',item&&'net'in item?item.net:''];});
    return [v.code,v.rule,v.message,...related].some(text=>text.toLowerCase().includes(query));
  });
}
export function drcCsv(violations:readonly DrcViolation[]):string{
  const cell=(value:string|number)=>{
    let text=String(value);
    // Treat user-provided names as text when opened in a spreadsheet.
    if(typeof value==='string'&&/^[=+@-]/.test(text))text="'"+text;
    return '"'+text.replace(/"/g,'""')+'"';
  };
  const rows:(string|number)[][]=[['Severity','Code','Rule','Message','Item IDs','Layer ID','X (mm)','Y (mm)'],...violations.map(v=>[v.severity,v.code,v.rule,v.message,v.items.join(';'),v.layer??'',v.pos.x/1e6,v.pos.y/1e6])];
  return '\uFEFF'+rows.map(row=>row.map(cell).join(',')).join('\r\n')+'\r\n';
}
