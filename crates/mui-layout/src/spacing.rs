#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpacingToken {
    Xs,
    S,
    M,
    L,
    Xl,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacingScale {
    pub xs: f64,
    pub s: f64,
    pub m: f64,
    pub l: f64,
    pub xl: f64,
}
impl Default for SpacingScale {
    fn default() -> Self {
        Self {
            xs: 4.0,
            s: 8.0,
            m: 12.0,
            l: 18.0,
            xl: 28.0,
        }
    }
}
impl SpacingScale {
    pub const DEFAULT: Self = Self {
        xs: 4.,
        s: 8.,
        m: 12.,
        l: 18.,
        xl: 28.,
    };
    pub fn get(self, t: SpacingToken) -> f64 {
        match t {
            SpacingToken::Xs => self.xs,
            SpacingToken::S => self.s,
            SpacingToken::M => self.m,
            SpacingToken::L => self.l,
            SpacingToken::Xl => self.xl,
        }
    }
    pub fn valid(self) -> bool {
        [self.xs, self.s, self.m, self.l, self.xl]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    Px(f64),
    Token(SpacingToken),
}
impl Spacing {
    pub const fn px(v: f64) -> Self {
        Self::Px(v)
    }
    pub const fn xs() -> Self {
        Self::Token(SpacingToken::Xs)
    }
    pub const fn s() -> Self {
        Self::Token(SpacingToken::S)
    }
    pub const fn m() -> Self {
        Self::Token(SpacingToken::M)
    }
    pub const fn l() -> Self {
        Self::Token(SpacingToken::L)
    }
    pub const fn xl() -> Self {
        Self::Token(SpacingToken::Xl)
    }
    pub fn resolve(self, scale: &SpacingScale) -> Option<f64> {
        let v = match self {
            Self::Px(v) => v,
            Self::Token(t) => scale.get(t),
        };
        (v.is_finite() && v >= 0.0).then_some(v)
    }
}

impl From<f64> for Spacing {
    fn from(v: f64) -> Self {
        Self::Px(v)
    }
}
impl From<SpacingToken> for Spacing {
    fn from(v: SpacingToken) -> Self {
        Self::Token(v)
    }
}
pub type Gap = SpacingToken;
pub type Pad = SpacingToken;
