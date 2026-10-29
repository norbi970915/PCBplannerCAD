/** A new package folder keeps each export separate from existing fabrication data. */
export function defaultPackageFolder(date=new Date()):string {return 'fabrication-'+date.toISOString().replace(/[-:]/g,'').replace(/\.\d{3}Z$/,'Z');}
export const packagePath=(parent:string,name:string)=>parent.replace(/[\\/]$/,'')+'/'+name;
/** ZIP is preferred; an unpacked package is viewed through its folder, never README.txt. */
export const packageViewerPath=(files:string[],folder:string)=>files.find(path=>path.toLowerCase().endsWith('.zip'))??folder;
