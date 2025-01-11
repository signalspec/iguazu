use egui::Rangef;

/// 2D Affine transformation from screen pixel coordinates to texture coordinates.
///
/// Both screen and texture coordinates are expected to be normalized to 0-1.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Transformation(pub [f32; 6]);

impl Transformation {
    /// Construct a transformation matrix from a sample order and texture coordinate bounds.
    pub fn new(u_range: Rangef, v_range: Rangef, order: SampleOrder) -> Transformation {
        let Transformation([a, b, c, d, e, f]) = order.into();
        let sx = u_range.span();
        let sy = v_range.span();
        Transformation([
            sx * a, sx * b, sx * c + u_range.min,
            sy * d, sy * e, sy * f + v_range.min,
        ])
    }

    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a*x + b*y + c, d*x + e*y + f)
    }

    pub(crate) fn wgpu_matrix(self) -> [f32; 12] {
        let [a, b, c, d, e, f] = self.0;
        [a, d, 0., 0., b, e, 0., 0., c, f, 1., 0.]
    }
}

/// A mapping from buffer pixel order to screen orientation
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SampleOrder {
    /// Left-to-right, top-to-bottom
    RightDown,

    /// Right-to-left, top-to-bottom
    LeftDown,

    /// Left-to-right, bottom-to-top
    RightUp,

    /// Right-to-left, bottom-to-top
    LeftUp,

    /// Top-to-bottom, left-to-right
    DownRight,

    /// Bottom-to-top, left-to-right
    UpRight,

    /// Top-to-bottom, right-to-left
    DownLeft,

    /// Bottom-to-top, right-to-left
    UpLeft,
}

impl From<SampleOrder> for Transformation {
    fn from(value: SampleOrder) -> Self {
        Transformation(match value {
            SampleOrder::RightDown => [ 1.,  0., 0.,  0.,  1., 0.],
            SampleOrder::LeftDown  => [-1.,  0., 1.,  0.,  1., 0.],
            SampleOrder::RightUp   => [ 1.,  0., 0.,  0., -1., 1.],
            SampleOrder::LeftUp    => [-1.,  0., 1.,  0., -1., 1.],
            SampleOrder::DownRight => [ 0.,  1., 0.,  1.,  0., 0.],
            SampleOrder::UpRight   => [ 0., -1., 1.,  1.,  0., 0.],
            SampleOrder::DownLeft  => [ 0.,  1., 0., -1.,  0., 1.],
            SampleOrder::UpLeft    => [ 0., -1., 1., -1.,  0., 1.],
        })
    }
}

impl From<(Rangef, Rangef, SampleOrder)> for Transformation {
    fn from((u_range, v_range, order): (Rangef, Rangef, SampleOrder)) -> Self {
        Transformation::new(u_range, v_range, order)
    }
}

#[test]
fn test_transformation() {
    use SampleOrder::*;

    assert_eq!(Transformation::from(RightDown).apply(0., 0.), (0., 0.));
    assert_eq!(Transformation::from(RightDown).apply(1., 0.), (1., 0.));
    assert_eq!(Transformation::from(RightDown).apply(0., 1.), (0., 1.));
    assert_eq!(Transformation::from(RightDown).apply(1., 1.), (1., 1.));

    assert_eq!(Transformation::from(LeftDown).apply(0., 0.), (1., 0.));
    assert_eq!(Transformation::from(LeftDown).apply(1., 0.), (0., 0.));
    assert_eq!(Transformation::from(LeftDown).apply(0., 1.), (1., 1.));
    assert_eq!(Transformation::from(LeftDown).apply(1., 1.), (0., 1.));

    assert_eq!(Transformation::from(RightUp).apply(0., 0.), (0., 1.));
    assert_eq!(Transformation::from(LeftUp).apply(0., 0.), (1., 1.));
    
    assert_eq!(Transformation::from(DownRight).apply(0., 0.), (0., 0.));
    assert_eq!(Transformation::from(UpRight).apply(0., 0.), (1., 0.));
    assert_eq!(Transformation::from(DownLeft).apply(0., 0.), (0., 1.));
    assert_eq!(Transformation::from(UpLeft).apply(0., 0.), (1., 1.));

    let t1 = Transformation::from((
        Rangef::new(0.25, 0.5),
        Rangef::new(0.75, 1.0),
        RightDown
    ));
    assert_eq!(t1.apply(0., 0.), (0.25, 0.75));
    assert_eq!(t1.apply(1., 0.), (0.50, 0.75));
    assert_eq!(t1.apply(0., 1.), (0.25, 1.00));
    assert_eq!(t1.apply(1., 1.), (0.50, 1.00));
}
