// The blend modes the compositor cannot ask the fixed-function blender for.
//
// wgpu's `BlendComponent` is a linear combination plus Min/Max: it can express
// source-over, multiply, screen, plus-lighter, lighten and darken, which is why
// those six are pipelines over `BlendState` in gpu.rs and cost nothing. Overlay,
// soft light, hard light, colour burn, colour dodge and linear burn are
// piecewise or non-linear in the *background* channel, so no state can name
// them: they have to be evaluated in the fragment shader, with the composite
// beneath read back as a texture.
//
// Everything here works in **premultiplied alpha**, which is what the
// compositor already produces. The formulas are the ones CapCut/Jianying ships
// in its own mix-mode shaders (Resources/MixMode/*/Library/ShaderData/**/*.frag),
// which matters more than elegance: they are the versions whose edge cases —
// a fully transparent foreground, a zero background, a divide by zero — have
// been seen by millions of exports.
//
// Paste this file into the compositor's shader source, or `include_str!` it and
// concatenate in front of the composite shader the way `COPY_SHADER` is.
//
// The mode numbers match `Blend` in concat-core, extended; keep them in step.

const BLEND_EPS: f32 = 1e-6;

// straights that the caller may hand over: undo a straight-alpha colour into
// the premultiplied one these formulas want.
fn blend_premultiply(c: vec4<f32>) -> vec4<f32> {
    if (c.a <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(c.rgb * c.a, c.a);
}

fn overlay_channel(fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    if (2.0 * bc < ba) {
        return fc * (1.0 - ba) + bc * (1.0 - fa) + 2.0 * fc * bc;
    }
    return fc * (1.0 + ba) + bc * (1.0 + fa) - 2.0 * fc * bc - fa * ba;
}

fn hard_light_channel(fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    if (2.0 * fc < fa) {
        return fc * (1.0 - ba) + bc * (1.0 - fa) + 2.0 * fc * bc;
    }
    return fc * (1.0 + ba) + bc * (1.0 + fa) - 2.0 * fc * bc - fa * ba;
}

fn soft_light_channel(fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    let t = bc / max(ba, BLEND_EPS);
    if (2.0 * fc <= fa) {
        return fc * (1.0 - ba) + bc * (1.0 - fa) + bc * (fa + (2.0 * fc - fa) * (1.0 - t));
    }
    return fc * (1.0 - ba) + bc * (1.0 + fa) + sqrt(t) * (2.0 * fc * ba - fa * ba) - 2.0 * fc * bc;
}

fn color_burn_channel(fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    if (fc == 0.0 && bc == ba) {
        return bc * (1.0 - fa) + fa * ba;
    }
    if (fc == 0.0) {
        return bc * (1.0 - fa);
    }
    let t = max(fc * ba, BLEND_EPS);
    return fc * (1.0 - ba) + bc * (1.0 - fa) + fa * ba * (1.0 - min(1.0, (ba - bc) * fa / t));
}

fn color_dodge_channel(fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    if (fc == fa && bc == 0.0) {
        return fc * (1.0 - ba) + fa * ba;
    }
    if (fc == fa) {
        return fc * (1.0 - ba) + bc * (1.0 - fa) + fa * ba;
    }
    let t = max(ba * (fa - fc), BLEND_EPS);
    return fc * (1.0 - ba) + bc * (1.0 - fa) + fa * ba * min(1.0, bc * fa / t);
}

fn linear_burn_channel(fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    return fc * (1.0 - ba) + bc * (1.0 - fa) + max(fc * ba + bc * fa - fa * ba, 0.0);
}

// Mode numbers: 0 normal, 1 multiply, 2 screen, 3 add, 4 lighten, 5 darken,
// 6 overlay, 7 soft light, 8 hard light, 9 colour burn, 10 colour dodge,
// 11 linear burn. 0..=5 are here only so a single shader can serve every mode
// when the fast fixed-function path is not taken; in the fast path they never
// reach this function.
fn blend_composite(mode: u32, fg: vec4<f32>, bg: vec4<f32>) -> vec4<f32> {
    let fa = fg.a;
    let ba = bg.a;
    let alpha = fa + ba - fa * ba;

    var c: vec3<f32>;
    switch mode {
        case 1u: {
            c = fg.rgb * (1.0 - ba) + bg.rgb * (1.0 - fa) + fg.rgb * bg.rgb;
        }
        case 2u: {
            c = fg.rgb * (1.0 - ba) + bg.rgb * (1.0 - fa) + (fg.rgb + bg.rgb - fg.rgb * bg.rgb) * fa * ba;
        }
        case 3u: {
            c = fg.rgb + bg.rgb;
        }
        case 4u: {
            c = max(fg.rgb * (1.0 - ba) + bg.rgb * (1.0 - fa) + max(fg.rgb, bg.rgb) * fa * ba, vec3<f32>(0.0));
        }
        case 5u: {
            c = min(fg.rgb * (1.0 - ba) + bg.rgb * (1.0 - fa) + min(fg.rgb, bg.rgb) * fa * ba, vec3<f32>(1.0));
        }
        case 6u: {
            c = vec3<f32>(
                overlay_channel(fg.r, bg.r, fa, ba),
                overlay_channel(fg.g, bg.g, fa, ba),
                overlay_channel(fg.b, bg.b, fa, ba),
            );
        }
        case 7u: {
            c = vec3<f32>(
                soft_light_channel(fg.r, bg.r, fa, ba),
                soft_light_channel(fg.g, bg.g, fa, ba),
                soft_light_channel(fg.b, bg.b, fa, ba),
            );
        }
        case 8u: {
            c = vec3<f32>(
                hard_light_channel(fg.r, bg.r, fa, ba),
                hard_light_channel(fg.g, bg.g, fa, ba),
                hard_light_channel(fg.b, bg.b, fa, ba),
            );
        }
        case 9u: {
            c = vec3<f32>(
                color_burn_channel(fg.r, bg.r, fa, ba),
                color_burn_channel(fg.g, bg.g, fa, ba),
                color_burn_channel(fg.b, bg.b, fa, ba),
            );
        }
        case 10u: {
            c = vec3<f32>(
                color_dodge_channel(fg.r, bg.r, fa, ba),
                color_dodge_channel(fg.g, bg.g, fa, ba),
                color_dodge_channel(fg.b, bg.b, fa, ba),
            );
        }
        case 11u: {
            c = vec3<f32>(
                linear_burn_channel(fg.r, bg.r, fa, ba),
                linear_burn_channel(fg.g, bg.g, fa, ba),
                linear_burn_channel(fg.b, bg.b, fa, ba),
            );
        }
        default: {
            c = fg.rgb + bg.rgb * (1.0 - fa);
        }
    }

    return vec4<f32>(c, alpha);
}

// The same modes for a caller holding straight-alpha textures. Kept as one
// function so the conversion cannot drift from the formulas above.
fn blend_composite_straight(mode: u32, fg: vec4<f32>, bg: vec4<f32>) -> vec4<f32> {
    let out = blend_composite(mode, blend_premultiply(fg), blend_premultiply(bg));
    if (out.a <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(out.rgb / out.a, out.a);
}
