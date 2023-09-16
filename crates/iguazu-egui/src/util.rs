#[allow(unused_macros)]
macro_rules! assert_approx_eq {
    ($l:expr, $r:expr, 1/$factor:expr) => {
        let l = $l;
        let r = $r;
        let diff = (l - r).abs();
        let threshold = r.abs() / $factor;
        assert!(diff <= threshold,
            "expected `{} ~= {}`: \
            left `{l:?}`, right `{r:?}`, \
            difference `{diff:?}`, threshold `{threshold:?}`",
            stringify!($l), stringify!($r)
        );
    };
    ($l:expr, $r:expr) => { assert_approx_eq!($l, $r, 1/1_000_000.0) }
}

#[allow(unused_imports)]
pub(crate) use assert_approx_eq;