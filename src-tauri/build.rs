fn main() {
    // Emits the platform resource/manifest files and embeds `tauri.conf.json`.
    // Fails the build with a readable message when the configuration or the
    // front-end directory is missing.
    tauri_build::build()
}
