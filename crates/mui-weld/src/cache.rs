use std::collections::VecDeque;
use std::sync::Arc;
use crate::{bake,Baked,Error,Request};

#[derive(Debug)]
struct Entry {request:Request,baked:Arc<Baked>,bytes:usize}
/// Host/UI-owned byte-bounded LRU. No global mutex, pointer-only shape keys, or
/// hash-only equality. A moved group can reuse a bake when its sources are passed
/// in group-local coordinates (the scene adapter does this).
#[derive(Debug)]
pub struct WeldCache {analytic:VecDeque<crate::analytic::AnalyticWeld>,analytic_hits:u64,analytic_builds:u64,entries:VecDeque<Entry>,limit:usize,bytes:usize,hits:u64,misses:u64}
impl Default for WeldCache {fn default()->Self {Self::with_limit(32*1024*1024)}}
impl WeldCache {
    pub fn with_limit(bytes:usize)->Self {Self{analytic:VecDeque::new(),analytic_hits:0,analytic_builds:0,entries:VecDeque::new(),limit:bytes,bytes:0,hits:0,misses:0}}
    pub fn bytes(&self)->usize {self.bytes+self.analytic.len()*Self::ANALYTIC_SLOT_BYTES}
    pub fn stats(&self)->(u64,u64) {(self.hits,self.misses)}
    pub fn clear(&mut self) {self.entries.clear();self.analytic.clear();self.bytes=0;}
    // Conservative fixed payload accounting: up to 128 edges, uniforms, sources,
    // and Vec/Arc headers. Allocator/driver overhead is not represented.
    const ANALYTIC_SLOT_BYTES:usize=32*1024;
    pub fn analytic_stats(&self)->(u64,u64){(self.analytic_hits,self.analytic_builds)}
    pub fn get_analytic(&mut self,sources:&[crate::Source],weld:crate::Weld,scale:f64)->Result<crate::analytic::AnalyticWeld,Error>{
        weld.validate()?;
        if sources.is_empty()||sources.len()>crate::analytic::ANALYTIC_SOURCES{return Err(Error::Invalid("GPU source count"));}
        let shapes=sources.iter().map(crate::analytic::AnalyticSource::from_source).collect::<Result<Vec<_>,_>>()?;
        if let Some(i)=self.analytic.iter().position(|a|a.same_geometry(&shapes,weld,scale)){
            let mut a=self.analytic.remove(i).ok_or(Error::Invalid("analytic cache index"))?;
            a.retarget(shapes,weld)?;self.analytic.push_back(a.clone());self.analytic_hits+=1;return Ok(a);
        }
        let a=crate::analytic::AnalyticWeld::new(shapes,weld,scale)?;self.analytic_builds+=1;
        if Self::ANALYTIC_SLOT_BYTES<=self.limit {
            while self.bytes()+Self::ANALYTIC_SLOT_BYTES>self.limit||self.analytic.len()>=16 {
                if self.analytic.pop_front().is_none(){if let Some(e)=self.entries.pop_front(){self.bytes-=e.bytes;}else{break;}}
            }
            self.analytic.push_back(a.clone());
        }
        Ok(a)
    }
    pub fn get(&mut self,request:&Request)->Result<Arc<Baked>,Error> {
        // A cache never bypasses validation for a mutated request or quality.
        request.validate()?;
        if let Some(i)=self.entries.iter().position(|e|e.request==*request) {
            let e=self.entries.remove(i).ok_or(Error::Invalid("cache index"))?;
            let result=e.baked.clone();self.entries.push_back(e);self.hits=self.hits.saturating_add(1);return Ok(result);
        }
        let result=Arc::new(bake(request)?);self.misses=self.misses.saturating_add(1);
        // Conservative payload budget, not allocator RSS. Shared image buffers
        // are counted in full per entry rather than undercounting live assets.
        let mut key_bytes = std::mem::size_of::<Entry>()
            .saturating_add(request.sources.len().saturating_mul(std::mem::size_of::<crate::Source>()));
        for s in &request.sources {
            if let crate::Geometry::Contours(rings) = &s.shape {
                key_bytes = key_bytes.saturating_add(rings.len().saturating_mul(std::mem::size_of::<Vec<crate::Point>>()));
                for ring in rings {
                    key_bytes = key_bytes.saturating_add(ring.len().saturating_mul(std::mem::size_of::<crate::Point>()));
                }
            }
            for brush in [&s.fill, &s.border].into_iter().flatten() {
                key_bytes = key_bytes.saturating_add(brush.retained_bytes());
            }
        }
        let bytes=result.bytes().checked_add(key_bytes).ok_or(Error::Budget)?;
        // Oversized outputs can be used for this frame without evicting the whole
        // useful cache. The caller still owns its frame's Arc after an eviction.
        let available=self.limit.saturating_sub(self.analytic.len()*Self::ANALYTIC_SLOT_BYTES);
        if bytes<=available {
            while self.bytes>available-bytes || self.entries.len()>=32 {
                let Some(e)=self.entries.pop_front() else {break};self.bytes-=e.bytes;
            }
            self.bytes+=bytes;
            self.entries.push_back(Entry{request:request.clone(),baked:result.clone(),bytes});
        }
        Ok(result)
    }
}

#[cfg(test)]
mod analytic_tests {
    use super::*;
    use crate::{Brush,Color,Geometry,Rect,Source,Weld};
    fn sources()->Vec<Source>{vec![Source{shape:Geometry::RoundedRect{bounds:Rect::new(0.,0.,40.,25.),radius:4.},fill:Some(Brush::Solid(Color([1.0,0.0,0.0,1.0]))),border:None,width:0.}]}
    #[test]fn materials_reuse_prepared_boundary(){let mut c=WeldCache::default();let src=sources();let a=c.get_analytic(&src,Weld::crisp(),1.).unwrap();let b=c.get_analytic(&src,Weld::crisp().morph(0.5),1.).unwrap();assert!(Arc::ptr_eq(a.boundary_bytes(),b.boundary_bytes()));assert_eq!(c.analytic_stats(),(1,1));}
    #[test]fn clear_releases_boundary_cache(){let mut c=WeldCache::default();c.get_analytic(&sources(),Weld::crisp(),1.).unwrap();assert!(c.bytes()>0);c.clear();assert_eq!(c.bytes(),0);}
    #[test]fn zero_budget_never_retains_boundary(){let mut c=WeldCache::with_limit(0);c.get_analytic(&sources(),Weld::crisp(),1.).unwrap();assert_eq!(c.bytes(),0);}
    #[test]fn invalid_morph_cannot_bypass_a_hit(){let mut c=WeldCache::default();c.get_analytic(&sources(),Weld::crisp(),1.).unwrap();assert!(c.get_analytic(&sources(),Weld::crisp().morph(f64::NAN),1.).is_err());}
}
