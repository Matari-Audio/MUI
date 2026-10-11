pub fn editor(params: Arc<P>) -> Box<dyn Editor> {
    MuiEditor::new(params, ui, SIZE, move |ui, bridge| build(ui, bridge)).into_editor()
}
truce::plugin! {
    logic: Relay,
    params: RelayParams,
}
