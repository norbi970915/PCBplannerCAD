// Fixed invokers for the pinned model-engine bindings. Preserve the upstream
// module/license and data conversion; do not enable string-to-JavaScript eval.
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
function section(source,start,end,replacement){const a=source.indexOf(start),b=source.indexOf(end,a);if(a<0||b<=a||source.indexOf(start,a+start.length)>=0)throw Error('Model binding layout changed; review the CSP-safe adapter before updating the engine.');return source.slice(0,a)+replacement+source.slice(b);}
export function staticStepBindings(source){
  source=section(source,'function craftInvokerFunction(','var ensureOverloadTable=',String.raw`function craftInvokerFunction(humanName,argTypes,classType,cppInvokerFunc,cppTargetFunc,isAsync){
    if(argTypes.length<2)throwBindingError("Invalid model binding arguments");
    var isMethod=argTypes[1]!==null&&classType!==null;
    var needsStack=usesDestructorStack(argTypes);
    return createNamedFunction(humanName,function(...values){
      var destructors=needsStack?[]:null,wired=[cppTargetFunc],types=[];
      if(isMethod){wired.push(argTypes[1].toWireType(destructors,this));types.push(argTypes[1]);}
      for(var i=2;i<argTypes.length;i++){wired.push(argTypes[i].toWireType(destructors,values[i-2]));types.push(argTypes[i]);}
      var result=cppInvokerFunc(...wired);
      if(needsStack)runDestructors(destructors);else for(var i=0;i<types.length;i++)if(typeof types[i].destructorFunction==="function")types[i].destructorFunction(wired[i+1]);
      return argTypes[0].name!=="void"?argTypes[0].fromWireType(result):undefined;
    });
  }`);
  source=section(source,'var __emval_get_method_caller=','var __emval_get_property=',String.raw`var __emval_get_method_caller=(argCount,argTypes,kind)=>{
    var types=emval_lookupTypes(argCount,argTypes),retType=types.shift();
    var caller=function(obj,func,destructorsRef,pointer){
      var values=[],offset=0;for(var i=0;i<types.length;i++){values.push(types[i].readValueFromPointer(pointer+offset));offset+=types[i].argPackAdvance;}
      var result=kind===1?Reflect.construct(func,values):func.call(...(kind===0?[obj,...values]:values));
      if(!retType.isVoid)return emval_returnValue(retType,destructorsRef,result);
    };
    return emval_addMethodCaller(createNamedFunction("modelMethodCaller",caller));
  };`);
  source=section(source,'var emval_get_global=','var __emval_get_global=','var emval_get_global=()=>globalThis;');
  if(source.includes('newFunc(Function,'))throw Error('Unadapted dynamic model binding');
  return 'const module={exports:{}};const exports=module.exports;\n'+source+'\nexport default module.exports;\n';
}
export function stepBindingsPlugin(){return {name:'static-step-bindings',resolveId(id){if(id==='virtual:pcad-step-bindings')return '\0pcad-step-bindings';},load(id){if(id!=='\0pcad-step-bindings')return;const file=require.resolve('occt-import-js');const metadata=JSON.parse(readFileSync(require.resolve('occt-import-js/package.json'),'utf8'));if(metadata.version!=='0.0.23')throw Error('Review static model bindings for this engine version');return staticStepBindings(readFileSync(file,'utf8'));}};}
