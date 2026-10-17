fn main() {
    // Emits the napi metadata the generated bindings need. Without this the
    // build compiles but the addon's symbols are not registered for Node.
    napi_build::setup();
}
