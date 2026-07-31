fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".to_owned())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/nocturne_reference.slint", config)
        .expect("failed to compile Slint interface");
}
