//! What a `Ui` is configured with: theme, scale, fonts, weld and layout
//! settings, and the counters it exposes.
use super::*;

impl Ui {
    /// Something every subtree reads changed: no kept subtree, `Ui::memo`
    /// or automatic, is reused on the next frame.
    pub(super) fn invalidate_retained(&mut self) {
        self.retained_root = None;
        self.cold_all = true;
        self.auto_cold = true;
    }
    /// Compare each frame's tree with the last and lay out and paint every
    /// unchanged subtree from last frame's work. On by default; a frame
    /// built off produces the same scene, only slower. See the `retain`
    /// module docs for what counts as unchanged.
    pub fn set_auto_retain(&mut self, on: bool) {
        self.auto_retain = on;
    }
    /// The theme every frame resolves with.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    /// Resolve the next frame with `theme`: a light/dark switch, a new
    /// control size.
    pub fn set_theme(&mut self, theme: Theme) {
        if self.theme != theme {
            self.invalidate_retained();
        }
        self.theme = theme;
    }
    /// The budget used for this UI's trees, including expanded memo subtrees.
    /// New UIs use [`mui_layout::Limits::default`].
    pub fn layout_limits(&self) -> mui_layout::Limits {
        self.layout_limits
    }
    /// Resolve subsequent frames with this per-window layout budget.
    ///
    /// A larger node budget permits larger bounded interfaces; it does not
    /// reduce their construction or rendering cost. Depth remains independently
    /// bounded, and virtual collections keep their declared content allowance.
    /// Invalid budgets leave the current limits and last scene unchanged.
    /// A host changing the limits should request a redraw.
    pub fn set_layout_limits(
        &mut self,
        limits: mui_layout::Limits,
    ) -> Result<(), mui_layout::Error> {
        limits.validate()?;
        if self.layout_limits != limits {
            self.retained_root = None;
            self.layout_limits = limits;
            self.resolved = false;
        }
        Ok(())
    }
    /// Swap the primary font after construction; `None` measures with the
    /// 0.6em guess. [`Ui::font`] is the builder form.
    pub fn set_font(&mut self, font: Option<Font>) {
        if self.font != font {
            self.invalidate_retained();
        }
        self.font = font;
    }
    /// The window's device pixels per logical unit, if the host set one.
    pub fn scale(&self) -> Option<f64> {
        self.scale
    }
    /// Set the device pixels per logical unit, and every painted edge lands
    /// on a device pixel; `None` paints on layout's raw f64.
    ///
    /// ```
    /// # use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// ui.set_scale(Some(2.0));
    /// assert_eq!(ui.scale(), Some(2.0));
    /// ```
    pub fn set_scale(&mut self, scale: Option<f64>) {
        if self.scale != scale {
            self.invalidate_retained();
        }
        self.scale = scale;
    }
    /// Seconds between two presses on one target that still make a double
    /// click: [`DOUBLE_CLICK`] unless the host matches a platform setting.
    pub fn set_double_click(&mut self, seconds: f64) {
        self.double_click = seconds;
    }
    /// The tip policy: `TipPolicy::After(0.5)` unless set.
    pub fn tips(&self) -> TipPolicy {
        self.tips
    }
    /// Show hovered tips after another rest, or not at all: the user's
    /// "show tooltips" preference. Off, no tip is floated, painted or
    /// reported on [`Frame::tip`], and no wake-up is scheduled for one. A
    /// tip already up goes on the next frame; a hover already resting is
    /// measured against the new delay. Accessible descriptions are
    /// unaffected (a tip never was one; see [`TipPolicy`]).
    /// Non-finite delays are ignored, preserving the current policy.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// ui.set_tips(TipPolicy::Off);
    /// assert_eq!(ui.tips(), TipPolicy::Off);
    /// ui.set_tips(TipPolicy::After(1.0));
    /// ```
    pub fn set_tips(&mut self, tips: TipPolicy) {
        if let TipPolicy::After(seconds) = tips
            && !seconds.is_finite()
        {
            return;
        }
        self.tips = tips;
    }
    /// Seconds a hover must rest for a tip, or `None` with tips off.
    pub(super) fn tip_delay(&self) -> Option<f64> {
        match self.tips {
            TipPolicy::Off => None,
            TipPolicy::After(seconds) => Some(seconds),
        }
    }
    /// Read and write the host's clipboard through `board`: copy, cut and
    /// paste in a text field then need nothing from the host. See
    /// [`Clipboard`].
    pub fn clipboard(mut self, board: impl Clipboard + 'static) -> Self {
        self.board = Some(Box::new(board));
        self
    }
    /// Set the font.
    pub fn font(mut self, font: Font) -> Self {
        self.invalidate_retained();
        self.font = Some(font);
        self
    }
    /// Where to find a face for text the fonts have no glyph for, such as
    /// `mui_text::FontDatabase::fallbacks` (feature `system-fonts`). A face
    /// it finds joins the fallback fonts and the frame after reshapes with it.
    pub fn fallback_source(mut self, source: mui_scene::FallbackSource) -> Self {
        self.resolver.set_fallback_source(Some(source));
        self
    }
    /// Add a font used when the primary face has no glyph for a grapheme.
    /// The selected face is retained in the scene text payload, so CPU and
    /// GPU renderers use the same fallback choice.
    pub fn fallback_font(mut self, font: Font) -> Self {
        self.invalidate_retained();
        self.fallback_fonts.push(font);
        self.runs.set_fallbacks(self.fallback_fonts.clone());
        self
    }
    /// Run `.weld(..)` / `weld!` on the analytic GPU backend: this `Ui`'s
    /// [`SceneSpec::weld_backend`](mui_scene::SceneSpec::weld_backend).
    pub fn gpu_welding(mut self) -> Self {
        self.invalidate_retained();
        self.weld_backend = mui_scene::WeldBackend::AnalyticGpu;
        self
    }

    /// Native hosts disable GPU welding when using software presentation.
    /// Rebuild cached material scenes without changing the application's preference.
    pub fn set_gpu_welding_available(&mut self, available: bool) {
        if self.gpu_welding_available != available {
            self.retained_root = None;
            self.gpu_welding_available = available;
            self.scene = None;
            self.resolver = Resolver::default();
            self.cold_all = true;
            self.auto_cold = true;
        }
    }
    /// Morph-only render update: keep the application declaration synchronized.
    /// Input reads the live scene's analytic predicate, so no hit-mask or closure
    /// has to be recreated on every animation tick.
    pub fn set_weld_morph(&mut self, id: impl Into<Id>, progress: f64) -> Result<bool, SceneError> {
        self.retained_root = None;
        let id: Id = id.into();
        let id = id.as_str();
        self.scene
            .as_mut()
            .map(Arc::make_mut)
            .ok_or(SceneError::UnsupportedWeld("no resolved scene"))?
            .set_weld_morph(id, progress)
    }
    pub fn set_weld_solid_material(
        &mut self,
        id: impl Into<Id>,
        index: usize,
        fill: Option<Color>,
        border: Option<Color>,
        width: f64,
    ) -> Result<bool, SceneError> {
        self.retained_root = None;
        let id: Id = id.into();
        let id = id.as_str();
        self.scene
            .as_mut()
            .map(Arc::make_mut)
            .ok_or(SceneError::UnsupportedWeld("no resolved scene"))?
            .set_weld_solid_material(id, index, fill, border, width)
    }
    pub fn set_weld_material_blend(
        &mut self,
        id: impl Into<Id>,
        blend: f64,
    ) -> Result<bool, SceneError> {
        self.retained_root = None;
        let id: Id = id.into();
        let id = id.as_str();
        self.scene
            .as_mut()
            .map(Arc::make_mut)
            .ok_or(SceneError::UnsupportedWeld("no resolved scene"))?
            .set_weld_material_blend(id, blend)
    }
    /// What the last frame resolved to, for anything drawn on top of it.
    /// Cache hits, misses, and conservative retained bytes for material welding.
    pub fn weld_cache_stats(&self) -> (u64, u64, usize) {
        let (hits, misses) = self.resolver.welds.stats();
        (hits, misses, self.resolver.welds.bytes())
    }
    /// Drop retained weld assets without changing live interaction state.
    pub fn clear_weld_cache(&mut self) {
        self.resolver.welds.clear();
    }

    pub fn layout_stats(&self) -> mui_layout::LayoutStats {
        if self.retained_frame {
            mui_layout::LayoutStats::default()
        } else {
            self.resolver.layout_stats()
        }
    }
    /// Opt-in exact layout work tracing. Keep disabled for latency measurements.
    pub fn trace_layout_requests(&mut self, enabled: bool) {
        self.resolver.trace_layout_requests(enabled);
    }
    /// Export bounded owned work records and stop tracing.
    pub fn take_layout_request_trace(&mut self) -> Option<mui_layout::LayoutRequestTrace> {
        self.resolver.take_layout_request_trace()
    }
}
