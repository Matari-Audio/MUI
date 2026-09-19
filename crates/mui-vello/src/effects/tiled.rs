//! Opt-in persistent tile renderer. Dirty tiles recompose ALL intersecting
//! content in paint order; the swapchain is fully covered by cached tiles each
//! presentation. This first version keeps paint-list diffing O(n), and unknown
//! text/effect extents conservatively retain their draw commands.
use crate::{Canvas as _,Gpu,ImageIds,PathCache,kurbo::Affine};
use mui_scene::{Layer,ResolvedScene};
use vello_common::{geometry::RectU16,peniko::ImageQuality};
use super::{Budget,EffectStats,Error,OutputEncoding,WeldTextures,damage::{self,Tile,DamageTracker}};
const SIDE:u32=256;
const GUARD:u32=2;
struct CachedTile{tile:Tile,_texture:wgpu::Texture,view:wgpu::TextureView}
#[derive(Clone,Debug,Default)]
pub struct TileStats{pub dirty_tiles:usize,pub total_tiles:usize,pub dirty_pixels:u64,pub tile_submissions:usize,pub replayed_ops:usize,pub culled_ops:usize}
pub struct TiledEffects {
    device:wgpu::Device,queue:wgpu::Queue,size:[u32;2],
    renderer:vello_hybrid::Renderer,resources:vello_hybrid::Resources,
    present_renderer:vello_hybrid::Renderer,present_resources:vello_hybrid::Resources,
    tile_scene:vello_hybrid::Scene,present_scene:vello_hybrid::Scene,
    images:ImageIds,paths:PathCache,effects:WeldTextures,
    tiles:Vec<CachedTile>,bindings:vello_hybrid::TextureBindings,
    damage:DamageTracker,limit:u64,stats:TileStats,
}
impl TiledEffects {
    pub async fn new(device:&wgpu::Device,queue:&wgpu::Queue,format:wgpu::TextureFormat,size:[u32;2],budget:Budget,tile_bytes:u64)->Result<Self,Error>{
        if !matches!(format,wgpu::TextureFormat::Rgba8Unorm|wgpu::TextureFormat::Bgra8Unorm){return Err(Error::Unsupported("tile output requires non-sRGB UNORM"));}
        let effects=WeldTextures::new(device,queue,budget,OutputEncoding::HybridPremultipliedSrgb).await?;
        let(renderer,resources)=vello_hybrid::Renderer::new(device,&vello_hybrid::RenderTargetConfig{format:wgpu::TextureFormat::Rgba8Unorm,width:SIDE+2*GUARD,height:SIDE+2*GUARD});
        let(present_renderer,present_resources)=vello_hybrid::Renderer::new(device,&vello_hybrid::RenderTargetConfig{format,width:size[0].max(1),height:size[1].max(1)});
        let mut s=Self{device:device.clone(),queue:queue.clone(),size:[0,0],renderer,resources,present_renderer,present_resources,
            tile_scene:vello_hybrid::Scene::new((SIDE+2*GUARD) as u16,(SIDE+2*GUARD) as u16),present_scene:vello_hybrid::Scene::new(1,1),images:Default::default(),paths:PathCache::new(),effects,tiles:Vec::new(),bindings:Default::default(),damage:Default::default(),limit:tile_bytes,stats:Default::default()};
        s.resize(size)?;Ok(s)
    }
    pub fn stats(&self)->&TileStats{&self.stats}
    pub fn invalidate(&mut self){self.damage.invalidate();}
    pub fn resize(&mut self,size:[u32;2])->Result<(),Error>{
        if size==self.size{return Ok(());}
        let max=self.device.limits().max_texture_dimension_2d.min(65535);
        if size.contains(&0)||size.iter().any(|s|*s>max){return Err(Error::Budget("tile viewport"));}
        let list=damage::tiles(size,SIDE);
        let bytes=list.iter().map(|t|u64::from(t.width+2*GUARD)*u64::from(t.height+2*GUARD)*4).sum::<u64>();
        if bytes>self.limit||list.len()>1024{return Err(Error::Budget("persistent tiles exceed explicit budget"));}
        self.tiles.clear();self.bindings=Default::default();self.size=size;
        self.present_scene.reset_and_resize(size[0] as u16,size[1] as u16);
        for (i,tile) in list.into_iter().enumerate(){
            let texture=self.device.create_texture(&wgpu::TextureDescriptor{label:Some("MUI retained tile"),size:wgpu::Extent3d{width:tile.width+2*GUARD,height:tile.height+2*GUARD,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[]});
            let view=texture.create_view(&Default::default());let id=vello_hybrid::TextureId(i as u64+1);
            self.bindings.insert(id,view.clone());
            // Guard pixels are rasterized but never overlap neighboring tiles.
            self.present_scene.draw_texture_rects(id,ImageQuality::Medium,[vello_hybrid::SampleRect{
                source_region:RectU16::new(GUARD as u16,GUARD as u16,(GUARD+tile.width) as u16,(GUARD+tile.height) as u16),
                transform:Affine::translate((tile.x as f64-GUARD as f64,tile.y as f64-GUARD as f64)),
            }]);
            self.tiles.push(CachedTile{tile,_texture:texture,view});
        }
        self.invalidate();Ok(())
    }
    pub fn render(&mut self,resolved:&ResolvedScene,xf:Affine,target:&wgpu::TextureView)->Result<EffectStats,Error>{
        if xf.as_coeffs().iter().any(|v|!v.is_finite()){return Err(Error::Unsupported("nonfinite tile transform"));}
        let list:Vec<_>=self.tiles.iter().map(|t|t.tile).collect();
        let plan=self.damage.plan(resolved,xf,&list);
        self.stats=TileStats{dirty_tiles:plan.dirty.len(),total_tiles:list.len(),dirty_pixels:plan.dirty_pixels,..Default::default()};
        // Prepare each effect ONCE, not per tile. Hidden effects are currently
        // retained too; admission is bounded by the effect pool budget.
        let mut stats=self.effects.begin(resolved.external_welds().map(|(k,e)|(k,&e.material)))?;
        let mut encoder=self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("MUI tile effects")});
        for(k,e)in resolved.external_welds(){if let Err(err)=self.effects.encode(k,&e.material,&mut encoder,&mut stats){self.effects.abort();self.invalidate();return Err(err);}}
        self.queue.submit([encoder.finish()]);self.effects.commit_submitted(&mut stats);
        let result=self.draw_tiles(resolved,xf,target,&plan.dirty,&mut stats);
        if result.is_ok(){self.damage.commit(resolved,xf);}else{self.invalidate();}
        result.map(|()|stats)
    }
    fn draw_tiles(&mut self,resolved:&ResolvedScene,xf:Affine,target:&wgpu::TextureView,dirty:&[usize],stats:&mut EffectStats)->Result<(),Error>{
        let bounds:Vec<_>=if dirty.is_empty(){Vec::new()}else{resolved.paint.iter().map(|p|damage::paint_bounds(p,xf)).collect()};
        self.paths.frame=self.paths.frame.wrapping_add(1);let epoch=self.paths.frame;
        for &i in dirty{
            let tile=self.tiles[i].tile;let region=tile.rect().inflate(GUARD as f64,GUARD as f64);
            let transform=Affine::translate((-region.x0,-region.y0))*xf;
            let size=[tile.width+2*GUARD,tile.height+2*GUARD];
            self.tile_scene.reset_and_resize(size[0] as u16,size[1] as u16);
            let mut canvas=Gpu{scene:&mut self.tile_scene,resources:&mut self.resources,atlas:Some(crate::Atlas{renderer:&mut self.renderer,device:&self.device,queue:&self.queue,ids:&mut self.images})};
            canvas.set_transform(transform);
            for (p,bounds) in resolved.paint.iter().zip(&bounds){
                if bounds.is_some_and(|b|!damage::intersects(b,region)){self.stats.culled_ops+=1;continue;}
                self.stats.replayed_ops+=1;
                if p.layer==Layer::External{
                    let e=resolved.external_weld(&p.key).ok_or_else(||Error::Missing(p.key.to_string()))?;
                    if !damage::intersects(damage::external_bounds(e,xf),region){continue;}
                    let id=self.effects.texture_id(&p.key).ok_or_else(||Error::Missing(p.key.to_string()))?;
                    let [w,h]=e.material.pixels();let b=e.bounds();
                    canvas.scene.draw_texture_rects(id,ImageQuality::Medium,[vello_hybrid::SampleRect{source_region:RectU16::new(0,0,w as u16,h as u16),transform:Affine::translate((b.min.x,b.min.y))*Affine::scale_non_uniform(b.width()/w as f64,b.height()/h as f64)}]);
                }else if !crate::layered(&mut canvas,p){let bez=self.paths.bez(p)?;crate::one(&mut canvas,p,&bez)?;}
            }
            let mut encoder=self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("MUI dirty tile")});
            self.renderer.render(&self.tile_scene,&mut self.resources,&self.device,&self.queue,&mut encoder,&vello_hybrid::RenderSize{width:size[0],height:size[1]},&self.tiles[i].view,self.effects.bindings()).map_err(|e|Error::Render(e.to_string()))?;
            // Submit before the same Vello renderer rewrites internal buffers
            // for another tile. No completion wait, mapping or readback occurs.
            self.queue.submit([encoder.finish()]);self.stats.tile_submissions+=1;stats.encoded_scenes+=1;
        }
        if !dirty.is_empty(){self.paths.entries.retain(|_,e|e.frame==epoch);}
        let mut encoder=self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("MUI retained tile presentation")});
        self.present_renderer.render(&self.present_scene,&mut self.present_resources,&self.device,&self.queue,&mut encoder,&vello_hybrid::RenderSize{width:self.size[0],height:self.size[1]},target,&self.bindings).map_err(|e|Error::Render(e.to_string()))?;
        self.queue.submit([encoder.finish()]);Ok(())
    }
}
