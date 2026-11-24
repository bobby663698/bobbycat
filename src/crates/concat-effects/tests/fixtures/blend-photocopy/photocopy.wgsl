// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

// Two premultiplied colours in linear light; return the complete composite.
fn blend(fg: vec4<f32>, bg: vec4<f32>) -> vec4<f32> {
    let foreground = fg.rgb / max(fg.a, 0.00001);
    let background = bg.rgb / max(bg.a, 0.00001);
    let ink = min(vec3<f32>(1.0), 2.0 * foreground * background);
    let colour = fg.rgb * (1.0 - bg.a) + bg.rgb * (1.0 - fg.a) + ink * fg.a * bg.a;
    return vec4<f32>(colour, fg.a + bg.a * (1.0 - fg.a));
}
