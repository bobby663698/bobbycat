# External blend packages

Install a folder under the existing custom-effects directory, `AppDirs.config/effects`,
then restart BobbyCat. On a normal Windows installation this is
`%APPDATA%/com.bobbycat.editor/effects`; portable builds use `portable/effects`.
The window and headless API load the same directory. Blend installation, changes
and removal take effect on the next process start. Ordinary effects keep their
existing reload behavior.

An external blend uses the existing package manifest:

```toml
format = 2

[effect]
id = "plug.photocopy"
name = "Photocopy"
kind = "blend"

[blend]
file = "photocopy.wgsl"
entry = "blend"
```

The WGSL supplies an ordinary function, not a fragment entry point:
`fn blend(fg: vec4<f32>, bg: vec4<f32>) -> vec4<f32>`.
Both inputs are premultiplied RGBA in scene-linear light; the foreground already
includes the clip's opacity, fades and mask. Return the complete premultiplied
composite, including alpha. The host supplies texture bindings and the fragment
wrapper. The optional `reads_ground` and `non_linear` fields must be `true` in
this first version; partial-output blend states are not supported.

The installable example is in
`src/crates/concat-effects/tests/fixtures/blend-photocopy`. For opaque pixels its
formula is `min(1, 2 * fg.rgb * bg.rgb)`; it also handles transparent inputs.
Package sources cannot declare entry points, resource bindings, overrides or
loops. Loading checks the source and function signature, then compiles the
complete shader on a GPU before offering the package. A refused package is
reported without preventing other packages from loading.

The twelve built-in modes keep their order. External labels follow them, while
the project stores the stable package id. A missing package leaves that id
intact, displays a warning item, and draws as Normal with a diagnostic. A loaded
shader refused at draw time produces an explicit warning or error; it is not
reported as a complete render. The CPU fallback cannot reproduce external WGSL.
External ids have no Jianying mix-mode mapping.
