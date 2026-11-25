// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! External blends: pure functions over two scene-linear premultiplied colours.
//! Packages supply no bindings or fragment entry point; those belong to the host.

use std::hash::{Hash, Hasher};

use concat_core::BlendPass;

use crate::manifest::Manifest;

/// A blend is a short colour formula, not a general-purpose shader program.
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;

/// Parse, validate and bind one package's ordinary two-colour function.
pub fn compile(manifest: &Manifest, source: &str) -> Result<BlendPass, String> {
    let table = manifest.blend.as_ref().ok_or("missing [blend] table")?;
    let fail_at = |location: naga::SourceLocation, why: &str| {
        format!(
            "{}:{}:{}: {why}",
            table.file, location.line_number, location.line_position
        )
    };
    let fail = |span: naga::Span, why: &str| fail_at(span.location(source), why);
    if source.len() > MAX_SOURCE_BYTES {
        return Err(fail(naga::Span::UNDEFINED, "blend source exceeds 64 KiB"));
    }
    let module =
        naga::front::wgsl::parse_str(source).map_err(|error| match error.location(source) {
            Some(location) => fail_at(location, error.message()),
            None => format!("{}: {}", table.file, error.emit_to_string(source)),
        })?;
    if !module.entry_points.is_empty() {
        return Err(fail(
            naga::Span::UNDEFINED,
            "a blend supplies an ordinary function, not @fragment or other entry points",
        ));
    }
    if let Some((handle, _)) = module.global_variables.iter().next() {
        return Err(fail(
            module.global_variables.get_span(handle),
            "a blend cannot declare global variables or resource bindings",
        ));
    }
    if !module.overrides.is_empty() {
        return Err(fail(
            naga::Span::UNDEFINED,
            "a blend cannot declare pipeline overrides",
        ));
    }
    if module.functions.len() > 32 || module.types.len() > 128 {
        return Err(fail(
            naga::Span::UNDEFINED,
            "blend exceeds the function or type budget",
        ));
    }
    for (handle, ty) in module.types.iter() {
        if matches!(
            ty.inner,
            naga::TypeInner::Image { .. }
                | naga::TypeInner::Sampler { .. }
                | naga::TypeInner::Array { .. }
                | naga::TypeInner::BindingArray { .. }
        ) {
            return Err(fail(
                module.types.get_span(handle),
                "a blend cannot declare textures, samplers or arrays",
            ));
        }
    }
    let mut expressions = 0;
    for (handle, function) in module.functions.iter() {
        expressions += function.expressions.len();
        if function.local_variables.len() > 64 || expressions > 4096 {
            return Err(fail(
                module.functions.get_span(handle),
                "blend exceeds the local-variable or expression budget",
            ));
        }
        pure(&function.body).map_err(|why| fail(module.functions.get_span(handle), why))?;
    }
    let (handle, function) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some(table.entry.as_str()))
        .ok_or_else(|| fail(naga::Span::UNDEFINED, "the named blend function is missing"))?;
    let colour = |ty| {
        matches!(
            module.types[ty].inner,
            naga::TypeInner::Vector {
                size: naga::VectorSize::Quad,
                scalar: naga::Scalar {
                    kind: naga::ScalarKind::Float,
                    width: 4
                }
            }
        )
    };
    if function.arguments.len() != 2
        || !function
            .arguments
            .iter()
            .all(|argument| colour(argument.ty))
        || !function
            .result
            .as_ref()
            .is_some_and(|result| colour(result.ty))
    {
        return Err(fail(
            module.functions.get_span(handle),
            "expected fn blend(fg: vec4<f32>, bg: vec4<f32>) -> vec4<f32>",
        ));
    }
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| format!("{}: {}", table.file, error.emit_to_string(source)))?;
    let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut fingerprint);
    table.entry.hash(&mut fingerprint);
    Ok(BlendPass {
        id: manifest.effect.id.clone(),
        key: format!(
            "blend:{}@{}#{:016x}",
            manifest.effect.id,
            manifest.effect.version,
            fingerprint.finish()
        ),
        source: source.into(),
        entry: table.entry.clone(),
    })
}

fn pure(block: &naga::Block) -> Result<(), &'static str> {
    for statement in block.iter() {
        match statement {
            naga::Statement::Loop { .. } => {
                return Err("a blend colour formula cannot contain loops");
            }
            naga::Statement::Kill => {
                return Err("a blend must return a composite, not discard a fragment");
            }
            naga::Statement::Block(inner) => pure(inner)?,
            naga::Statement::If { accept, reject, .. } => {
                pure(accept)?;
                pure(reject)?;
            }
            naga::Statement::Switch { cases, .. } => {
                for case in cases {
                    pure(&case.body)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = include_str!("../tests/fixtures/blend-photocopy/effect.toml");
    const SOURCE: &str = include_str!("../tests/fixtures/blend-photocopy/photocopy.wgsl");

    #[test]
    fn photocopy_is_a_pure_function_with_a_source_bound_cache_key() {
        let manifest = Manifest::parse(MANIFEST).expect("a blend manifest");
        let pass = compile(&manifest, SOURCE).expect("a colour function");
        assert_eq!(pass.id, "plug.photocopy");
        assert_eq!(pass.entry, "blend");
        assert!(pass.source.contains("fn blend("));
        assert_ne!(
            pass.key,
            compile(&manifest, &format!("{SOURCE}\n// changed"))
                .unwrap()
                .key
        );
    }

    #[test]
    fn a_fragment_signature_and_resources_are_refused() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let fragment = "@fragment fn blend() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }";
        assert!(
            compile(&manifest, fragment)
                .unwrap_err()
                .contains("ordinary function")
        );
        let global = format!("@group(0) @binding(0) var image: texture_2d<f32>;\n{SOURCE}");
        assert!(
            compile(&manifest, &global)
                .unwrap_err()
                .contains("resource bindings")
        );
        let wrong = "\nfn blend(fg: vec4<f32>) -> vec4<f32> { return fg; }";
        let error = compile(&manifest, wrong).unwrap_err();
        assert!(error.contains("photocopy.wgsl:2:"), "{error}");
        assert!(error.contains("expected fn blend"), "{error}");
    }

    #[test]
    fn loops_and_invalid_wgsl_are_refused_at_the_original_source_line() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let endless = "fn blend(fg: vec4<f32>, bg: vec4<f32>) -> vec4<f32> { loop {} return fg; }";
        assert!(compile(&manifest, endless).unwrap_err().contains("loops"));
        let error = compile(&manifest, "\n\nfn blend( ?").unwrap_err();
        assert!(error.starts_with("photocopy.wgsl:3:"), "{error}");
        assert!(
            !error
                .split_whitespace()
                .any(|word| word.starts_with("wgsl:3:")),
            "{error}"
        );
    }
}
