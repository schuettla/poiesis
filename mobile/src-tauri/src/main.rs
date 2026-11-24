// Desktop builds of the companion exist only for development and tests; the app is for phones.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    poiesis_companion_lib::run()
}
