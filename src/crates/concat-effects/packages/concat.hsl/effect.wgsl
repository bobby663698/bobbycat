struct Params {
    red_hue: f32, red_saturation: f32, red_lightness: f32,
    orange_hue: f32, orange_saturation: f32, orange_lightness: f32,
    yellow_hue: f32, yellow_saturation: f32, yellow_lightness: f32,
    green_hue: f32, green_saturation: f32, green_lightness: f32,
    cyan_hue: f32, cyan_saturation: f32, cyan_lightness: f32,
    blue_hue: f32, blue_saturation: f32, blue_lightness: f32,
    purple_hue: f32, purple_saturation: f32, purple_lightness: f32,
    magenta_hue: f32, magenta_saturation: f32, magenta_lightness: f32,
}

// Eight-band HSL, transcribed from Jianying Pro 11.6's
// DefaultAdjustBundle/hsl/HSL/xshader/fshader.frag. The band centres and
// half-widths below are that file's `hueAdjust` calls, verbatim: red at -12.5
// with an offset of 360 (so it wraps across 0), orange 25, yellow 57.5,
// green 120, cyan 180, blue 207.5, purple 275, magenta 310.
//
// Each band's three signed values shift hue (degrees), saturation and
// lightness (percent). The influence is a triangle: full at the centre,
// zero at the band's edge, so neighbouring bands meet without a seam and a
// pixel is touched by at most two. All values at zero return the picture
// untouched, which makes this a pure identity at its defaults.
//
// The CPU twin in concat-effects/src/hsl.rs mirrors this arithmetic; keep
// the two in step.

const PI: f32 = 3.14159265359;

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
    let mx = max(rgb.r, max(rgb.g, rgb.b));
    let mn = min(rgb.r, min(rgb.g, rgb.b));
    let d = mx - mn;
    var h = 0.0;
    var s = 0.0;
    if (mx != 0.0) { s = d / mx; }
    if (s != 0.0) {
        if (rgb.r == mx) { h = (rgb.g - rgb.b) / d; }
        else if (rgb.g == mx) { h = 2.0 + (rgb.b - rgb.r) / d; }
        else { h = 4.0 + (rgb.r - rgb.g) / d; }
        h = h * 60.0;
        if (h < 0.0) { h = h + 360.0; }
    }
    return vec3<f32>(h, s, mx);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let h = hsl.r;
    let s = hsl.g;
    let v = hsl.b;
    if (s == 0.0) { return vec3<f32>(v); }
    let sector = h / 60.0;
    let i = floor(sector);
    let f = sector - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    if (i < 1.0) { return vec3<f32>(v, t, p); }
    if (i < 2.0) { return vec3<f32>(q, v, p); }
    if (i < 3.0) { return vec3<f32>(p, v, t); }
    if (i < 4.0) { return vec3<f32>(p, q, v); }
    if (i < 5.0) { return vec3<f32>(t, p, v); }
    return vec3<f32>(v, p, q);
}

/// The distance in degrees between two hues, on the 0..360 circle.
fn hue_gap(a: f32, b: f32) -> f32 {
    var d = abs(a - b) % 360.0;
    if (d > 180.0) { d = 360.0 - d; }
    return d;
}

/// Apply one band. `hsl` is the running value, `base` the untouched one;
/// `centre` and `half` are the band's hue window; `p` its three values.
fn band(hsl: vec3<f32>, base: vec3<f32>, centre: f32, half: f32, p: vec3<f32>) -> vec3<f32> {
    if (p.x == 0.0 && p.y == 0.0 && p.z == 0.0) { return hsl; }
    let gap = hue_gap(base.x, centre);
    if (gap >= half) { return hsl; }
    let a = 1.0 - gap / half;              // triangular falloff
    var out = hsl;
    out.x = (out.x + p.x * a) % 360.0;     // hue, degrees
    if (out.x < 0.0) { out.x = out.x + 360.0; }
    out.y = clamp(out.y + out.y * (p.y / 100.0) * a, 0.0, 1.0);
    out.z = clamp(out.z + out.z * base.y * (p.z / 100.0) * a, 0.0, 1.0);
    return out;
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    // A light past the display's white is carried through untouched: a look
    // must not clip a highlight its caller still needs. HSL is defined on
    // 0..1 display levels, so it has nothing to say about a brighter light.
    if (any(c.rgb > vec3<f32>(1.0))) {
        return c;
    }
    let base = rgb_to_hsl(c.rgb);
    var h = base;
    h = band(h, base, 347.5, 32.5, vec3<f32>(params.red_hue, params.red_saturation, params.red_lightness));
    h = band(h, base, 25.0, 35.0, vec3<f32>(params.orange_hue, params.orange_saturation, params.orange_lightness));
    h = band(h, base, 57.5, 32.5, vec3<f32>(params.yellow_hue, params.yellow_saturation, params.yellow_lightness));
    h = band(h, base, 120.0, 70.0, vec3<f32>(params.green_hue, params.green_saturation, params.green_lightness));
    h = band(h, base, 180.0, 45.0, vec3<f32>(params.cyan_hue, params.cyan_saturation, params.cyan_lightness));
    h = band(h, base, 207.5, 62.5, vec3<f32>(params.blue_hue, params.blue_saturation, params.blue_lightness));
    h = band(h, base, 275.0, 40.0, vec3<f32>(params.purple_hue, params.purple_saturation, params.purple_lightness));
    h = band(h, base, 310.0, 55.0, vec3<f32>(params.magenta_hue, params.magenta_saturation, params.magenta_lightness));
    return vec4<f32>(clamp(hsl_to_rgb(h), vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
}
