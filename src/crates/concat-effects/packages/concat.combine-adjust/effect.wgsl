struct Params {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    highlight: f32,
    shadow: f32,
    black: f32,
    white: f32,
}

// Jianying's combined tone panel, transcribed from its `colorAdjust.frag`:
// brightness (a pow), contrast (a gain about middle grey), saturation (a
// luminance-weighted matrix), highlights and shadows (pows around 1 and 0),
// then a black/white slope. The signed knobs come in as percentages; the
// transfer functions below are the panel's `SeekModeScript.lua` mapping.
//
// The CPU twin in concat-effects/src/combine.rs mirrors this arithmetic; keep
// the two in step. At every knob zero the shader returns the picture exactly.

const SAT_LUMA: vec3<f32> = vec3<f32>(0.208540, 0.702086, 0.089374);
const PIVOT: f32 = 0.435;

fn brightness_param(i: f32) -> f32 {
    if (i > 0.0 && i <= 0.7) { return 0.3 * i; }
    if (i > 0.7 && i <= 1.0) { return 0.6333 * i - 0.2333; }
    return i;
}

fn highlight_param(i: f32) -> f32 {
    return 1.0 + 0.503 * i + 0.183 * i * i + 0.147 * i * i * i + 0.067 * i * i * i * i;
}

fn shadow_param(i: f32) -> f32 {
    return 1.0 - 0.503 * i + 0.183 * i * i - 0.147 * i * i * i + 0.067 * i * i * i * i;
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    // A light past white is carried through untouched: a tone panel must not
    // clip a highlight its caller still needs. These knobs are defined on
    // display levels 0..1 and have nothing to say about a brighter light.
    if (any(c.rgb > vec3<f32>(1.0))) {
        return c;
    }
    var rgb = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let brightness = params.brightness / 100.0;
    let contrast = params.contrast / 100.0;
    let saturation = params.saturation / 100.0;
    let highlight = params.highlight / 100.0;
    let shadow = params.shadow / 100.0;
    let black = params.black / 100.0;
    let white = params.white / 100.0;

    if (abs(brightness) > 1e-6) {
        let p = max(1.0 + brightness_param(brightness) * 5.0, 0.001);
        rgb = vec3<f32>(1.0) - pow(vec3<f32>(1.0) - rgb, vec3<f32>(p));
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    if (abs(contrast) > 1e-6) {
        let gain = max(1.0 + contrast, 0.0);
        rgb = clamp(gain * (rgb - vec3<f32>(PIVOT)) + vec3<f32>(PIVOT), vec3<f32>(0.0), vec3<f32>(1.0));
    }

    if (abs(saturation) > 1e-6) {
        let s = saturation + 1.0;
        let base = SAT_LUMA * (1.0 - s);
        rgb = vec3<f32>(
            dot(rgb, base + vec3<f32>(s, 0.0, 0.0)),
            dot(rgb, base + vec3<f32>(0.0, s, 0.0)),
            dot(rgb, base + vec3<f32>(0.0, 0.0, s)),
        );
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    if (abs(highlight) > 1e-6) {
        let p = max(highlight_param(highlight), 0.001);
        rgb = vec3<f32>(1.0) - pow(vec3<f32>(1.0) - rgb, vec3<f32>(p));
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    if (abs(shadow) > 1e-6) {
        let p = max(shadow_param(shadow), 0.001);
        rgb = pow(rgb, vec3<f32>(p));
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    if (abs(black) > 1e-6 || abs(white) > 1e-6) {
        let xb = (black + 1.0) * 0.5;
        let xw = (white + 1.0) * 0.5;
        let yb = xb * 0.5;
        let yw = 0.5 + (xw - 0.5) * 0.5;
        let span = xw - xb;
        if (abs(span) > 1e-5) {
            let slope = (yw - yb) / span;
            let bias = yb - slope * xb;
            rgb = clamp(rgb * slope + vec3<f32>(bias), vec3<f32>(0.0), vec3<f32>(1.0));
        }
    }

    return vec4<f32>(rgb, c.a);
}
