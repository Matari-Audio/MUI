/** Mount a capture manifest as independent, seek-safe CSS 3D planes.
 * Textures are presentation assets. Re-export the native scene for layout reflow.
 */
export function validateScene(scene) {
  if (!scene || scene.version !== 1 || !Array.isArray(scene.layers) || !scene.layers.length)
    throw new Error('Expected a version 1 MUI capture manifest with layers.');
  for (const key of ['width', 'height', 'scale'])
    if (!Number.isFinite(scene[key]) || scene[key] <= 0) throw new Error(`Invalid ${key}.`);
  const ids = new Set();
  for (const layer of scene.layers) {
    if (typeof layer.id !== 'string' || !layer.id || ids.has(layer.id)) throw new Error('Layer IDs must be unique.');
    ids.add(layer.id);
    if (layer.origin && (!Array.isArray(layer.origin) || layer.origin.length !== 2 || !layer.origin.every(Number.isFinite))) throw new Error('Invalid layer pivot.');
    if (layer.group !== undefined && (typeof layer.group !== 'string' || !layer.group)) throw new Error('Invalid group.');
    if (typeof layer.src !== 'string' || !/^[a-zA-Z0-9_.-]+\.png$/.test(layer.src)) throw new Error('Layer source must be a local PNG filename.');
    if (!Array.isArray(layer.rect) || layer.rect.length !== 4 || !layer.rect.every(Number.isFinite) || layer.rect[2] <= 0 || layer.rect[3] <= 0)
      throw new Error(`Invalid rectangle for ${layer.id}.`);
  }
  return scene;
}

export function poseTransform({ x = 0, y = 0, z = 0, rotateX = 0, rotateY = 0, rotateZ = 0, scaleX = 1, scaleY = 1 } = {}) {
  if (![x,y,z,rotateX,rotateY,rotateZ,scaleX,scaleY].every(Number.isFinite) || scaleX <= 0 || scaleY <= 0)
    throw new Error('Pose values must be finite and scales must be positive.');
  return `translate3d(${x}px,${y}px,${z}px) rotateX(${rotateX}deg) rotateY(${rotateY}deg) rotateZ(${rotateZ}deg) scale(${scaleX},${scaleY})`;
}

export function mountLayers(container, scene, assetBase, textures = {}, decoded = false) {
  validateScene(scene);
  const world = document.createElement('div');
  world.className = 'mui-world';
  Object.assign(world.style, { position:'relative', width:`${scene.width}px`, height:`${scene.height}px`, transformStyle:'preserve-3d' });
  const layers = new Map();
  const images = [];
  for (const layer of scene.layers) {
    const [x,y,w,h] = layer.rect;
    const plane = document.createElement('div');
    plane.className = 'mui-plane';
    const group = layer.group ?? layer.id;
    plane.dataset.muiId = group;
    plane.id = `mui-${layer.id}`;
    Object.assign(plane.style, { position:'absolute', left:`${x}px`, top:`${y}px`, width:`${w}px`, height:`${h}px`, transformStyle:'preserve-3d', transformOrigin:'50% 50%' });
    const img = document.createElement('img');
    img.alt = layer.id === 'background' ? 'Instrument editor chassis' : layer.id;
    img.src = textures[layer.src] || new URL(layer.src, assetBase).href;
    img.draggable = false;
    Object.assign(img.style, { display:'block', width:'100%', height:'100%' });
    if (layer.origin) plane.style.transformOrigin = `${layer.origin[0]-x}px ${layer.origin[1]-y}px`;
    plane.append(img); world.append(plane);
    if (!layers.has(group)) layers.set(group, []);
    layers.get(group).push(plane); images.push(img);
  }
  container.replaceChildren(world);
  return {
    world, layers,
    update(next, base, textures = {}) {
      validateScene(next);
      if(next.layers.length!==scene.layers.length || next.layers.some((l,i)=>l.id!==scene.layers[i].id || l.group!==scene.layers[i].group)) return false;
      next.layers.forEach((layer,i)=>{const plane=world.children[i],[x,y,w,h]=layer.rect;
        Object.assign(plane.style,{left:`${x}px`,top:`${y}px`,width:`${w}px`,height:`${h}px`,transformOrigin:layer.origin?`${layer.origin[0]-x}px ${layer.origin[1]-y}px`:'50% 50%'});
        const src=textures[layer.src]||new URL(layer.src,base).href;if(plane.firstChild.src!==src)plane.firstChild.src=src;
      });
      world.style.width=`${next.width}px`;world.style.height=`${next.height}px`;scene=next;return true;
    },
    ready: decoded ? Promise.resolve() : Promise.all(images.map(img => img.decode())),
    setPose(id, pose) {
      const planes = layers.get(id);
      if (!planes) throw new Error(`Unknown MUI layer: ${id}`);
      for (const plane of planes) plane.style.transform = poseTransform(pose);
    },
    reset() { for (const planes of layers.values()) for (const plane of planes) plane.style.transform = poseTransform(); },
    destroy() { world.remove(); layers.clear(); }
  };
}
