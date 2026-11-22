struct Params {
    red: array<vec4<f32>, 8>,
    green: array<vec4<f32>, 8>,
    blue: array<vec4<f32>, 8>,
    strength: f32,
}

// Apply the transfer curves learned from a reference picture, one per RGB
// channel, blended from the original by `strength`. The curves come from
// concat-effects' `color_match` module, which matches histograms; this
// package only applies them. All three curves default to the straight line
// black-to-white, for which `grade_curves` returns the picture untouched.
//
// A level past white is carried on by the curve rather than clipped, the
// same convention `grade_curves` uses everywhere else in the display space.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    // Each channel walks its own learned transfer. `curve_at` carries a level
    // past white on past the curve rather than clipping it, matching how the
    // rest of the display-space grading treats highlights.
    let matched = vec3<f32>(
        curve_at(params.red, c.r),
        curve_at(params.green, c.g),
        curve_at(params.blue, c.b),
    );
    let s = clamp(params.strength / 100.0, 0.0, 1.0);
    return vec4<f32>(mix(c.rgb, matched, s), c.a);
}
