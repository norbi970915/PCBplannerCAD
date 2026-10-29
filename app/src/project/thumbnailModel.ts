import * as THREE from 'three';
import {readModelAsset} from '../view3d/models';
let renderer:THREE.WebGLRenderer|null=null;
/** Model assets use a single shared renderer, and release each parsed model. */
export async function modelThumbnail(path:string,ui:'light'|'dark',alive:()=>boolean):Promise<string|null>{
  const model=await readModelAsset(path,true);if(!model)return null;
  try{if(!alive())return null;renderer??=new THREE.WebGLRenderer({antialias:true,alpha:false,preserveDrawingBuffer:true});renderer.setSize(224,128,false);renderer.setPixelRatio(1);
    const scene=new THREE.Scene();scene.background=new THREE.Color(ui==='dark'?'#1b1f28':'#f1f4f8');scene.add(new THREE.AmbientLight(0xffffff,1.4));const light=new THREE.DirectionalLight(0xffffff,2.4);light.position.set(3,-4,7);scene.add(light);scene.add(model);
    const box=new THREE.Box3().setFromObject(model);if(box.isEmpty())return null;const size=box.getSize(new THREE.Vector3()).length(),center=box.getCenter(new THREE.Vector3());if(!Number.isFinite(size)||size<=0)return null;
    const aspect=224/128,camera=new THREE.OrthographicCamera(-size*aspect*.6,size*aspect*.6,size*.6,-size*.6,size/10000,size*10);camera.up.set(0,0,1);camera.position.copy(center).add(new THREE.Vector3(1.3,-1.7,1.2).multiplyScalar(size));camera.lookAt(center);renderer.render(scene,camera);
    return alive()?renderer.domElement.toDataURL('image/png'):null;
  }finally{model.traverse(object=>{if(object instanceof THREE.Mesh){object.geometry.dispose();const materials=Array.isArray(object.material)?object.material:[object.material];for(const material of materials){for(const value of Object.values(material))if(value instanceof THREE.Texture)value.dispose();material.dispose();}}});renderer?.renderLists.dispose();}
}
