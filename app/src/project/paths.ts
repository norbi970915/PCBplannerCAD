/** Folder names are single path components, including when used in restore/import. */
export function folderNameError(name:string):string|null {
  if(!name||name.trim()!==name||/[<>:"/\\|?*\x00-\x1f]/.test(name)||name.endsWith('.'))return 'Use a single folder name without reserved characters, trailing dots or spaces.';
  if(/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name))return 'That folder name is reserved.';
  return null;
}
