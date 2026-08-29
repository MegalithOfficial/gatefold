fn main() {
    glib_build_tools::compile_resources(
        &["data/icons"],
        "data/gatefold.gresource.xml",
        "gatefold.gresource",
    );
}
