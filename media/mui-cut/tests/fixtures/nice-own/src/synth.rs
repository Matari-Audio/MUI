// The export in a module: the type is public at the root.
nice_export_clap!(Synth);
fn spawn() {
    let window = mui_baseview::open(&parent, "Synth", (w, h), None, shared, requests);
}
