struct Params { strength: f32, radius: f32, softness: f32 }

// The light falling off towards the corners, as a lens's does. Distance is
// measured from the middle of the layer and doubled, so the edge of a side
// is 1 and a corner is sqrt(2). `radius` moves the start of the fall and
// `softness` its width, both as percentages. The fall is taken to the 2.4th
// power of the SDR display encoding, so a highlight in a corner darkens by
// the same share as the rest of the picture. Strength zero leaves every
// pixel exactly as it came in: 1.0 - 0.0 is 1.0, and 1.0^2.4 is 1.0.
//
// The CPU twin in concat-effects/src/vignette.rs mirrors these four lines
// expression for expression; keep the two in step.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let d = length((uv - vec2<f32>(0.5)) * vec2<f32>(2.0));
    let r = 0.35 + params.radius / 100.0 * 0.75;
    let w = 0.05 + params.softness / 100.0 * 0.6;
    let fall = smoothstep(r - w * 0.5, r + w * 0.5, d);
    let s = clamp(params.strength / 100.0, 0.0, 1.0);
    return vec4<f32>(c.rgb * pow(1.0 - fall * s, 2.4), c.a);
}
