pub fn open(params: Arc<GainParams>) -> Box<dyn Editor> {
    MuiEditor::new(params, ui(), (320, 200), |ui, bridge| view(ui, bridge)).into_editor()
}
