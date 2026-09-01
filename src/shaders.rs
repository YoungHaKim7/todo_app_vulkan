//! UI shaders, compiled from `assets/` at build time.

pub(crate) mod ui_vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        path: "../assets/ui_vs.vert",
    }
}

pub(crate) mod ui_fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        path: "../assets/ui_fs.frag",
    }
}
