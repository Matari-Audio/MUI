@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
fn sample_at(p: vec2<f32>, dims: vec2<u32>) -> vec3<f32> {
    return textureLoad(source, vec2<i32>(clamp(p, vec2(0.0), vec2<f32>(dims)-vec2(1.0))), 0).rgb;
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims=textureDimensions(source);
    if any(id.xy>=dims) {return;}
    let p=vec2<f32>(id.xy)+vec2(0.5);
    let scale=f32(dims.x)/768.0;
    let center=vec2<f32>(dims)*vec2(0.5,0.51);
    let half_size=vec2(205.0,94.0)*scale;
    let radius=28.0*scale;
    let q=abs(p-center)-half_size+vec2(radius);
    let distance=length(max(q,vec2(0.0)))+min(max(q.x,q.y),0.0)-radius;
    let background=sample_at(p,dims);
    let mask=1.0-smoothstep(-0.8,0.8,distance);
    if mask<=0.0 {textureStore(output_texture,vec2<i32>(id.xy),vec4(background,1.0));return;}
    let normal=normalize((p-center)/half_size+vec2(0.00001));
    let bevel=1.0-smoothstep(0.0,18.0*scale,-distance);
    let refracted=p-normal*bevel*10.0*scale;
    var blurred=vec3(0.0);
    for(var y=-2;y<=2;y++){for(var x=-2;x<=2;x++){
        blurred+=sample_at(refracted+vec2(f32(x),f32(y))*1.5*scale,dims)/25.0;
    }}
    let highlight=pow(bevel,4.0)*max(0.0,dot(normal,normalize(vec2(-0.6,-0.8))))*0.45;
    let material=blurred*0.88+vec3(0.10,0.12,0.15)+vec3(highlight);
    textureStore(output_texture,vec2<i32>(id.xy),vec4(mix(background,material,mask),1.0));
}
