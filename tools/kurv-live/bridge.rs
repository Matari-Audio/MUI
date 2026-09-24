// Kurv adapter only; common capture/transport/clock live in mui-motion-bridge.
pub fn motion_live() -> Result<(), String> {
    use truce::prelude::*;
    use serde_json::{Value,json};

    let meters=truce_core::meters::MeterStore::new();
    let transport_slot=truce_core::transport::TransportSlot::new();
    let context=motion_context(Arc::clone(&meters),Arc::clone(&transport_slot));let params=Arc::clone(context.params());
    // A real routed LFO: DSP publishes the phase/value consumed by the native card.
    let lfo_index=super::cards::modulator::add(&context,crate::modulators::state::SourceKind::Lfo).ok_or("no free LFO slot")?;
    {
        let index=lfo_index;
        let mut config=params.modulator_rack.config(index);config.rate_hz=1.;params.modulator_rack.set_config(index,config);
        use crate::modulators::routing::{ModulationRouteTarget as T,OscillatorControl as C,ResolvedRouteSource as S};
        if let Some((module,slot))=super::binding::oscillator_slot(&params,0) {
            crate::modulation::edit::assign(&context,S::Rack(index as u8),T::oscillator(module,slot,C::Level),0.7);
        }
    }
    params.voice_mode.set_value(8);params.oversampling.set_value(1);params.set_sample_rate(48_000.);params.snap_smoothers();
    let mut state=<crate::Kurv as PluginLogic>::init(&params,&InitContext::new(None));
    <crate::Kurv as PluginLogic>::reset(&mut state,&params,&AudioConfig::new(48_000.,mui_motion_bridge::BLOCK_FRAMES));
    let mut sample_position=0i64;
    let audio=move |notes:&[mui_motion_bridge::NoteEvent], samples:&mut [[f32;2]]| {
        let mut events=EventList::with_capacity(128);
        for event in notes {match *event {
            mui_motion_bridge::NoteEvent::On{note,velocity}=>events.push(Event::new(0,EventBody::NoteOn{group:0,channel:1,note,velocity})),
            mui_motion_bridge::NoteEvent::Off{note}=>events.push(Event::new(0,EventBody::NoteOff{group:0,channel:1,note,velocity:0})),
            mui_motion_bridge::NoteEvent::Panic=>{for note in 0..128 {events.push(Event::new(0,EventBody::NoteOff{group:0,channel:1,note,velocity:0}));}},
        }}
        let mut left=vec![0.;samples.len()];let mut right=vec![0.;samples.len()];let mut outputs:[&mut[f32];2]=[&mut left,&mut right];
        let mut buffer=AudioBuffer::from_slices_checked(&[],&mut outputs,samples.len());let mut out=EventList::with_capacity(0);let transport=TransportInfo {playing:true,tempo:120.,time_sig_num:4,time_sig_den:4,position_samples:sample_position,position_seconds:sample_position as f64/48_000.,position_beats:sample_position as f64/24_000.,..TransportInfo::default()};transport_slot.write(&transport);sample_position+=samples.len() as i64;
        let meter_sink=|id,value|meters.write(id,value);
        let mut ctx=ProcessContext::new(&transport,48_000.,samples.len(),&mut out).with_meters(&meter_sink);
        <crate::Kurv as PluginLogic>::process(&mut state,&params,&mut buffer,&events,&mut ctx);
        for (i,s) in samples.iter_mut().enumerate(){*s=[left[i],right[i]];}
    };
    let mut editor=mui_motion_bridge::Editor::new(theme::ui(theme::skin()));
    let mut racks=Racks::new(context.clone());
    let mut capture=mui_motion_bridge::CaptureStream::default();
    let view_context=context.clone();
    let snapshot=move |_revision:u64,frame:u64,commands:&[Value]|->Result<Value,String>{
        editor.advance(commands,frame,|ui,input,dt,sizes| {
            let tree=mui_motion_bridge::Editor::layout(racks.tree(ui,&input),sizes)?;
            ui.frame(tree,Some(SIZE),input,dt).map_err(|e|format!("{e:?}"))?;
            racks.after(ui);Ok(())
        })?;
        let ui=&editor.ui;
        let scene=ui.scene().ok_or("empty scene")?;
        let roots=if editor.selection.is_empty(){mui_motion_bridge::discover_parts(scene,SIZE.width,SIZE.height)}else{editor.selection.clone()};
        let mut value=capture.frame(scene,900,622,0.6,&roots)?;
        let patch=view_context.generator_stack.snapshot();
        let modules:Vec<Value>=patch.groups().iter().flat_map(|g|g.modules().iter()).map(|m|{
            if let Some(slot)=m.oscillator_slot(){let mut c=view_context.generator_stack.oscillator_config(slot);
                for (bank,target) in view_context.params().host_automation_targets.snapshot().into_iter().enumerate() {
                    if let Some(crate::modulators::routing::ModulationRouteTarget::Oscillator{module_id,slot:bound,control})=target {
                        if module_id==m.id().get()&&bound==slot&&control.supports_engine(c.engine){control.apply_normalized(&mut c,view_context.params().host_automation_normalized(bank) as f32);}
                    }
                }
                json!({"id":m.id().get(),"part":format!("osc/{}",slot.index()),"kind":format!("{:?}",c.engine),"level":c.level,"shape":c.shape,"transpose":c.transpose,"parameters":[{"id":"level","label":"Level","min":0,"max":2,"step":0.01,"value":c.level},{"id":"shape","label":"Wave shape","min":0,"max":3,"step":0.01,"value":c.shape},{"id":"transpose","label":"Transpose","min":-24,"max":24,"step":1,"value":c.transpose}]})}
            else {json!({"id":m.id().get(),"kind":format!("{:?}",m.kind())})}
        }).collect();
        value["modules"]=json!(modules);
        value["telemetry"]=json!({"lfo":view_context.params().modulator_rack.ui_snapshot(lfo_index)});
        Ok(value)
    };
    let edit=|command:&Value|->Result<(),String>{
            let op=command["op"].as_str().ok_or("missing operation")?;
            if op=="snapshot" {return Ok(());}
            let patch=context.generator_stack.snapshot();
            let group=patch.groups().first().ok_or("no group")?;
            if op=="add" {
                if group.modules().len()>=4{return Err("This motion proof supports at most four modules.".into());}
                let action=match command["kind"].as_str(){Some("va")=>GeneratorAddAction::Oscillator,Some("noise")=>GeneratorAddAction::Noise,Some("filter")=>GeneratorAddAction::Filter,_=>return Err("unknown module kind".into())};
                add(&context,0,action);
            } else {
                let id=command["id"].as_u64().ok_or("missing module id")?;
                let (index,module)=group.modules().iter().enumerate().find(|(_,m)|m.id().get()==id).ok_or("unknown module")?;
                match op {
                    "delete"=>{admission::remove_module(&context,module.id());},
                    "move"=>{let direction=command["direction"].as_i64().ok_or("missing direction")?;let next=(index as i64+direction).clamp(0,group.modules().len() as i64-1) as usize;context.generator_stack.try_edit(|p|p.move_module(module.id(),group.id(),next)).map_err(|e|format!("{e:?}"))?;},
                    "set"=>{
                        let slot=module.oscillator_slot().ok_or("select an oscillator")?;let mut config=context.generator_stack.oscillator_config(slot);
                        let value=command["value"].as_f64().filter(|v|v.is_finite()).ok_or("invalid value")? as f32;
                        match command["field"].as_str(){Some("level") if (0.0..=2.).contains(&value)=>config.level=value,Some("shape") if (0.0..=3.).contains(&value)=>config.shape=value,Some("transpose") if (-24.0..=24.).contains(&value)=>config.transpose=value,_=>return Err("invalid parameter or range".into())};
                        context.set_oscillator_config(slot,config).map_err(|e|format!("{e:?}"))?;
                    },
                    _=>return Err("unknown operation".into())
                }
                context.params().ensure_host_automation();
            }
            Ok(())
    };
    mui_motion_bridge::run_live(json!({"name":"Kurv","notes":true,"addKinds":[{"id":"va","label":"VA oscillator"},{"id":"noise","label":"Noise"},{"id":"filter","label":"Filter"}],"move":true,"delete":true}),audio,edit,snapshot)
}

fn motion_context(meters:Arc<truce_core::meters::MeterStore>,transport:Arc<truce_core::transport::TransportSlot>) -> PluginContext<KurvParams> {
    let params = Arc::new(KurvParams::default());
    let (set, get, plain, format) = (
        Arc::clone(&params),
        Arc::clone(&params),
        Arc::clone(&params),
        Arc::clone(&params),
    );
    let bridge = Arc::new(ClosureBridge {
        begin_edit: Box::new(|_| {}),
        set_param: Box::new(move |id, v| set.set_normalized(id, v)),
        end_edit: Box::new(|_| {}),
        request_resize: Box::new(|_, _| false),
        get_param: Box::new(move |id| get.get_normalized(id).unwrap_or_default()),
        get_param_plain: Box::new(move |id| plain.get_plain(id).unwrap_or_default()),
        format_param: Box::new(move |id| {
            let v = format.get_plain(id).unwrap_or_default();
            format.format_value(id, v).unwrap_or_default()
        }),
        get_meter: Box::new(move |id| meters.read(id)),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(move || transport.read()),
    });
    PluginContext::new(bridge, params)
}
